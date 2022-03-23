/// ExecuteCostTable is aggregated by Cost Model, it keeps each program's
/// average cost in its HashMap, with fixed capacity to avoid from growing
/// unchecked.
/// When its capacity limit is reached, it prunes old and less-used programs
/// to make room for new ones.
use {
    crate::cost_calculation_metrics::CostCalculationMetrics,
    log::*,
    solana_program_runtime::{compute_budget, timings::ProgramTiming},
    solana_sdk::pubkey::Pubkey,
    std::{
        collections::{hash_map::Entry, HashMap},
        time::Duration,
    },
};

// prune is rather expensive op, free up bulk space in each operation
// would be more efficient. PRUNE_RATIO defines the after prune table
// size will be original_size * PRUNE_RATIO.
const PRUNE_RATIO: f64 = 0.75;
// with 50_000 TPS as norm, weights occurrences '100' per microsec
const OCCURRENCES_WEIGHT: i64 = 100;

const DEFAULT_CAPACITY: usize = 1024;
const MAX_METRICS_REPORT_PER_SEC: usize = 10;

#[derive(Debug)]
pub struct ExecuteCostTable {
    capacity: usize,
    table: HashMap<Pubkey, u64>,
    occurrences: HashMap<Pubkey, (usize, u128)>,
    cost_calculation_metrics: CostCalculationMetrics,
}

impl Default for ExecuteCostTable {
    fn default() -> Self {
        ExecuteCostTable::new(DEFAULT_CAPACITY)
    }
}

impl ExecuteCostTable {
    pub fn new(cap: usize) -> Self {
        Self {
            capacity: cap,
            table: HashMap::with_capacity(cap),
            occurrences: HashMap::with_capacity(cap),
            cost_calculation_metrics: CostCalculationMetrics::new(
                MAX_METRICS_REPORT_PER_SEC,
                Duration::from_secs(1),
            ),
        }
    }

    pub fn get_count(&self) -> usize {
        self.table.len()
    }

    /// default program cost, set to ComputeBudget::DEFAULT_UNITS
    pub fn get_default_units(&self) -> u64 {
        compute_budget::DEFAULT_UNITS as u64
    }

    /// average cost of all recorded programs
    pub fn get_average_units(&self) -> u64 {
        if self.table.is_empty() {
            self.get_default_units()
        } else {
            self.table.iter().map(|(_, value)| value).sum::<u64>() / self.get_count() as u64
        }
    }

    /// the most frequently occurring program's cost
    pub fn get_statistical_mode_units(&self) -> u64 {
        if self.occurrences.is_empty() {
            self.get_default_units()
        } else {
            let key = self
                .occurrences
                .iter()
                .max_by_key(|&(_, count)| count)
                .map(|(key, _)| key)
                .expect("cannot find mode from cost table");

            *self.table.get(key).unwrap()
        }
    }

    /// returns None if program doesn't exist in table. In this case,
    /// `get_default_units()`, `get_average_units()` or `get_statistical_mode_units()`
    /// can be used to assign a value to new program.
    pub fn get_cost(&self, key: &Pubkey) -> Option<&u64> {
        self.table.get(key)
    }

    pub fn initialize(&mut self, key: &Pubkey, cost: &u64) {
        self.table.insert(*key, *cost);
    }

    /// update-or-insert should be infallible. Query the result of upsert,
    /// often requires additional calculation, should be lazy.
    pub fn upsert(&mut self, key: &Pubkey, program_timing: &ProgramTiming) {
        let need_to_add = !self.table.contains_key(key);
        let current_size = self.get_count();
        if current_size == self.capacity && need_to_add {
            self.prune_to(&((current_size as f64 * PRUNE_RATIO) as usize));
        }

        let value = program_timing.accumulated_units / program_timing.count as u64;
        match self.table.entry(*key) {
            Entry::Occupied(mut entry) => {
                let result = entry.get_mut();
                *result = (*result + value) / 2;

                if Self::is_large_change(value, *result) {
                    self.cost_calculation_metrics
                        .report(key, program_timing, *result);
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
        };

        let (count, timestamp) = self
            .occurrences
            .entry(*key)
            .or_insert((0, Self::micros_since_epoch()));
        *count += 1;
        *timestamp = Self::micros_since_epoch();
    }

    /// Simple calculation anomaly check, return True if calculation result is
    /// LARGE_CHANGE_THRESHOLD times larger than input data itself;
    fn is_large_change(input: u64, calculated_value: u64) -> bool {
        const LARGE_CHANGE_THRESHOLD: u64 = 20;
        const PERCENTAGE_OF_MAX_UNITS: f64 = 0.95;
        let has_oversized_impact = input != 0 && calculated_value / input > LARGE_CHANGE_THRESHOLD;
        let is_large_value =
            calculated_value as f64 / compute_budget::MAX_UNITS as f64 > PERCENTAGE_OF_MAX_UNITS;
        has_oversized_impact || is_large_value
    }

    /// prune the old programs so the table contains `new_size` of records,
    /// where `old` is defined as weighted age, which is negatively correlated
    /// with program's age and how frequently the program is occurrenced.
    fn prune_to(&mut self, new_size: &usize) {
        debug!(
            "prune cost table, current size {}, new size {}",
            self.get_count(),
            new_size
        );

        if *new_size == self.get_count() {
            return;
        }

        if *new_size == 0 {
            self.table.clear();
            self.occurrences.clear();
            return;
        }

        let now = Self::micros_since_epoch();
        let mut sorted_by_weighted_age: Vec<_> = self
            .occurrences
            .iter()
            .map(|(key, (count, timestamp))| {
                let age = now - timestamp;
                let weighted_age = *count as i64 * OCCURRENCES_WEIGHT + -(age as i64);
                (weighted_age, *key)
            })
            .collect();
        sorted_by_weighted_age.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

        for i in sorted_by_weighted_age.iter() {
            self.table.remove(&i.1);
            self.occurrences.remove(&i.1);
            if *new_size == self.get_count() {
                break;
            }
        }
    }

    fn micros_since_epoch() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_program_timing_for_cost(cost: u64) -> ProgramTiming {
        ProgramTiming {
            accumulated_us: 0,
            accumulated_units: cost,
            count: 1,
            errored_txs_compute_consumed: vec![],
            total_errored_units: 0,
        }
    }

    #[test]
    fn test_execute_cost_table_prune_simple_table() {
        solana_logger::setup();
        let capacity: usize = 3;
        let mut testee = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();

        testee.upsert(&key1, &make_program_timing_for_cost(1));
        testee.upsert(&key2, &make_program_timing_for_cost(2));
        testee.upsert(&key3, &make_program_timing_for_cost(3));

        testee.prune_to(&(capacity - 1));

        // the oldest, key1, should be pruned
        assert!(testee.get_cost(&key1).is_none());
        assert!(testee.get_cost(&key2).is_some());
        assert!(testee.get_cost(&key2).is_some());
    }

    #[test]
    fn test_execute_cost_table_prune_weighted_table() {
        solana_logger::setup();
        let capacity: usize = 3;
        let mut testee = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();

        // simulate a lot of occurrences to key1, so even there're longer than
        // usual delay between upsert(key1..) and upsert(key2, ..), test
        // would still satisfy as key1 has enough occurrences to compensate
        // its age.
        for i in 0..1000 {
            testee.upsert(&key1, &make_program_timing_for_cost(i));
        }
        testee.upsert(&key2, &make_program_timing_for_cost(2));
        testee.upsert(&key3, &make_program_timing_for_cost(3));

        testee.prune_to(&(capacity - 1));

        // the oldest, key1, has many counts; 2nd oldest Key2 has 1 count;
        // expect key2 to be pruned.
        assert!(testee.get_cost(&key1).is_some());
        assert!(testee.get_cost(&key2).is_none());
        assert!(testee.get_cost(&key3).is_some());
    }

    #[test]
    fn test_execute_cost_table_upsert_within_capacity() {
        solana_logger::setup();
        let mut testee = ExecuteCostTable::default();

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let cost1: u64 = 100;
        let cost2: u64 = 110;

        // query empty table
        assert!(testee.get_cost(&key1).is_none());

        // insert one record
        testee.upsert(&key1, &make_program_timing_for_cost(cost1));
        assert_eq!(1, testee.get_count());
        assert_eq!(cost1, testee.get_average_units());
        assert_eq!(cost1, testee.get_statistical_mode_units());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());

        // insert 2nd record
        testee.upsert(&key2, &make_program_timing_for_cost(cost2));
        assert_eq!(2, testee.get_count());
        assert_eq!((cost1 + cost2) / 2_u64, testee.get_average_units());
        assert_eq!(cost2, testee.get_statistical_mode_units());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());

        // update 1st record
        testee.upsert(&key1, &make_program_timing_for_cost(cost2));
        assert_eq!(2, testee.get_count());
        assert_eq!(
            ((cost1 + cost2) / 2 + cost2) / 2_u64,
            testee.get_average_units()
        );
        assert_eq!((cost1 + cost2) / 2, testee.get_statistical_mode_units());
        assert_eq!(&((cost1 + cost2) / 2), testee.get_cost(&key1).unwrap());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());
    }

    #[test]
    fn test_execute_cost_table_upsert_exceeds_capacity() {
        solana_logger::setup();
        let capacity: usize = 2;
        let mut testee = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let cost1: u64 = 100;
        let cost2: u64 = 110;
        let cost3: u64 = 120;
        let cost4: u64 = 130;

        // insert one record
        testee.upsert(&key1, &make_program_timing_for_cost(cost1));
        assert_eq!(1, testee.get_count());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());

        // insert 2nd record
        testee.upsert(&key2, &make_program_timing_for_cost(cost2));
        assert_eq!(2, testee.get_count());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());

        // insert 3rd record, pushes out the oldest (eg 1st) record
        testee.upsert(&key3, &make_program_timing_for_cost(cost3));
        assert_eq!(2, testee.get_count());
        assert_eq!((cost2 + cost3) / 2_u64, testee.get_average_units());
        assert_eq!(cost3, testee.get_statistical_mode_units());
        assert!(testee.get_cost(&key1).is_none());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());
        assert_eq!(&cost3, testee.get_cost(&key3).unwrap());

        // update 2nd record, so the 3rd becomes the oldest
        // add 4th record, pushes out 3rd key
        testee.upsert(&key2, &make_program_timing_for_cost(cost1));
        testee.upsert(&key4, &make_program_timing_for_cost(cost4));
        assert_eq!(
            ((cost1 + cost2) / 2 + cost4) / 2_u64,
            testee.get_average_units()
        );
        assert_eq!((cost1 + cost2) / 2, testee.get_statistical_mode_units());
        assert_eq!(2, testee.get_count());
        assert!(testee.get_cost(&key1).is_none());
        assert_eq!(&((cost1 + cost2) / 2), testee.get_cost(&key2).unwrap());
        assert!(testee.get_cost(&key3).is_none());
        assert_eq!(&cost4, testee.get_cost(&key4).unwrap());
    }

    #[test]
    fn test_is_large_change() {
        // input data `0` is for initializing, skip is_oversized_impact check
        assert!(!ExecuteCostTable::is_large_change(0, 100));
        assert!(ExecuteCostTable::is_large_change(
            0,
            compute_budget::MAX_UNITS as u64
        ));

        // check when is_oversized_impact is false
        assert!(!ExecuteCostTable::is_large_change(10, 10));
        assert!(ExecuteCostTable::is_large_change(
            10,
            compute_budget::MAX_UNITS as u64
        ));

        // check when is_oversized_impact is true
        assert!(ExecuteCostTable::is_large_change(10, 500));
        assert!(ExecuteCostTable::is_large_change(
            10,
            compute_budget::MAX_UNITS as u64
        ));
    }
}
