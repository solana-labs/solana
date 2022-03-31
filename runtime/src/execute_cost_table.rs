/// ExecuteCostTable is aggregated by Cost Model, it keeps each program's
/// average cost in its HashMap, with fixed capacity to avoid from growing
/// unchecked.
/// When its capacity limit is reached, it prunes old and less-used programs
/// to make room for new ones.
use {
    log::*,
    solana_program_runtime::compute_budget,
    solana_sdk::pubkey::Pubkey,
    std::collections::{hash_map::Entry, HashMap},
};

// prune is rather expensive op, free up bulk space in each operation
// would be more efficient. PRUNE_RATIO defines that after prune, table
// size will be original_size * PRUNE_RATIO. The value is defined in
// scale of 100.
const PRUNE_RATIO: usize = 75;
// with 50_000 TPS as norm, weights occurrences '100' per microsec
const OCCURRENCES_WEIGHT: i64 = 100;

const DEFAULT_CAPACITY: usize = 1024;

// The EMA_ALPHA represents the degree of weighting decrease in EMA,
// a constant smoothing factor between 0 and 1. A higher alpha
// discounts older observations faster.
// Estimate it to 0.01 by `2/(N+1)` where N is 200 samples
const EMA_ALPHA: i128 = 10;
const EMA_SCALE: i128 = 1000;

// exponential moving average algorithm
// https://en.wikipedia.org/wiki/Moving_average#Exponentially_weighted_moving_variance_and_standard_deviation
#[derive(Debug, Default)]
struct AggregatedVarianceStats {
    ema: u64,
    ema_var: u64,
}

impl AggregatedVarianceStats {
    fn get_ema(&self) -> u64 {
        self.ema
    }

    fn get_stddev(&self) -> u64 {
        (self.ema_var as f64).sqrt().ceil() as u64
    }

    fn aggregate_ema(&mut self, theta: i128) {
        self.ema = u64::try_from(
            i128::from(self.ema)
                .saturating_mul(EMA_SCALE)
                .saturating_add(theta.saturating_mul(EMA_ALPHA))
                .saturating_div(EMA_SCALE),
        )
        .ok()
        .unwrap_or(self.ema);
    }

    fn aggregate_variance(&mut self, theta: i128) {
        self.ema_var = u64::try_from(
            i128::from(self.ema_var)
                .saturating_mul(EMA_SCALE)
                .saturating_add(theta.saturating_pow(2).saturating_mul(EMA_ALPHA))
                .saturating_mul(EMA_SCALE - EMA_ALPHA)
                .saturating_div(EMA_SCALE.saturating_pow(2)),
        )
        .ok()
        .unwrap_or(self.ema_var);
    }
}

#[derive(Debug)]
pub struct ExecuteCostTable {
    capacity: usize,
    table: HashMap<Pubkey, AggregatedVarianceStats>,
    occurrences: HashMap<Pubkey, (usize, u128)>,
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
            self.table
                .iter()
                .map(|(_, value)| value.get_ema())
                .sum::<u64>()
                / self.get_count() as u64
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

            self.table.get(key).unwrap().get_ema()
        }
    }

    /// return program average cost units, or None if program doesn't exist in table.
    pub fn get_average_program_units(&self, key: &Pubkey) -> Option<u64> {
        let aggregated_variance_stats = self.table.get(key)?;
        Some(aggregated_variance_stats.get_ema())
    }

    /// returns inflated program cost units, which is `ema + 2*stddev`, or None
    /// if program doesn't exist in table. In this case,
    /// `get_default_units()`, `get_average_units()` or `get_statistical_mode_units()`
    /// can be used to assign a value to new program.
    pub fn get_inflated_program_units(&self, key: &Pubkey) -> Option<u64> {
        let aggregated_variance_stats = self.table.get(key)?;
        Some(aggregated_variance_stats.get_ema() + 2 * aggregated_variance_stats.get_stddev())
    }

    /// update-or-insert should be infallible. Query the result of upsert,
    /// often requires additional calculation, should be lazy.
    pub fn upsert(&mut self, key: &Pubkey, value: u64) {
        let need_to_add = !self.table.contains_key(key);
        let current_size = self.get_count();
        if current_size >= self.capacity && need_to_add {
            let prune_to_size = current_size
                .checked_mul(PRUNE_RATIO)
                .and_then(|v| v.checked_div(100))
                .unwrap_or(self.capacity);
            self.prune_to(&prune_to_size);
        }

        match self.table.entry(*key) {
            Entry::Occupied(mut entry) => {
                let aggregated_variance_stats = entry.get_mut();
                let theta = i128::from(value) - i128::from(aggregated_variance_stats.get_ema());
                aggregated_variance_stats.aggregate_ema(theta);
                aggregated_variance_stats.aggregate_variance(theta);
            }
            Entry::Vacant(entry) => {
                // the starting values
                entry.insert(AggregatedVarianceStats {
                    ema: value,
                    ema_var: 0u64,
                });
            }
        }

        let (count, timestamp) = self
            .occurrences
            .entry(*key)
            .or_insert((0, Self::micros_since_epoch()));
        *count += 1;
        *timestamp = Self::micros_since_epoch();
    }

    /// prune the old programs so the table contains `new_size` of records,
    /// where `old` is defined as weighted age, which is negatively correlated
    /// with program's age and how frequently the program is occurrences.
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

    #[test]
    fn test_execute_cost_table_prune_simple_table() {
        solana_logger::setup();
        let capacity: usize = 3;
        let mut execute_cost_table = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();

        execute_cost_table.upsert(&key1, 1);
        execute_cost_table.upsert(&key2, 2);
        execute_cost_table.upsert(&key3, 3);

        execute_cost_table.prune_to(&(capacity - 1));

        // the oldest, key1, should be pruned
        assert!(execute_cost_table
            .get_inflated_program_units(&key1)
            .is_none());
        assert!(execute_cost_table
            .get_inflated_program_units(&key2)
            .is_some());
        assert!(execute_cost_table
            .get_inflated_program_units(&key2)
            .is_some());
    }

    #[test]
    fn test_execute_cost_table_prune_weighted_table() {
        solana_logger::setup();
        let capacity: usize = 3;
        let mut execute_cost_table = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();

        // simulate a lot of occurrences to key1, so even there're longer than
        // usual delay between upsert(key1..) and upsert(key2, ..), test
        // would still satisfy as key1 has enough occurrences to compensate
        // its age.
        for i in 0..1000 {
            execute_cost_table.upsert(&key1, i);
        }
        execute_cost_table.upsert(&key2, 2);
        execute_cost_table.upsert(&key3, 3);

        execute_cost_table.prune_to(&(capacity - 1));

        // the oldest, key1, has many counts; 2nd oldest Key2 has 1 count;
        // expect key2 to be pruned.
        assert!(execute_cost_table
            .get_inflated_program_units(&key1)
            .is_some());
        assert!(execute_cost_table
            .get_inflated_program_units(&key2)
            .is_none());
        assert!(execute_cost_table
            .get_inflated_program_units(&key3)
            .is_some());
    }

    #[test]
    fn test_execute_cost_table_upsert_within_capacity() {
        solana_logger::setup();
        let mut execute_cost_table = ExecuteCostTable::default();

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let cost1: u64 = 100;
        let cost2: u64 = 110;

        // query empty table
        assert!(execute_cost_table
            .get_inflated_program_units(&key1)
            .is_none());

        // insert one record
        execute_cost_table.upsert(&key1, cost1);
        assert_eq!(1, execute_cost_table.get_count());
        assert_eq!(cost1, execute_cost_table.get_average_units());
        assert_eq!(cost1, execute_cost_table.get_statistical_mode_units());
        assert_eq!(
            cost1,
            execute_cost_table
                .get_inflated_program_units(&key1)
                .unwrap()
        );

        // insert 2nd record
        execute_cost_table.upsert(&key2, cost2);
        assert_eq!(2, execute_cost_table.get_count());
        assert_eq!(
            (cost1 + cost2) / 2_u64,
            execute_cost_table.get_average_units()
        );
        assert_eq!(cost2, execute_cost_table.get_statistical_mode_units());
        assert_eq!(
            cost1,
            execute_cost_table
                .get_inflated_program_units(&key1)
                .unwrap()
        );
        assert_eq!(
            cost2,
            execute_cost_table
                .get_inflated_program_units(&key2)
                .unwrap()
        );

        // update 1st record
        execute_cost_table.upsert(&key1, cost1);
        assert_eq!(2, execute_cost_table.get_count());
        assert_eq!(
            (cost1 + cost2) / 2_u64,
            execute_cost_table.get_average_units()
        );
        assert_eq!(cost1, execute_cost_table.get_statistical_mode_units());
        assert_eq!(
            cost1,
            execute_cost_table
                .get_inflated_program_units(&key1)
                .unwrap()
        );
        assert_eq!(
            cost2,
            execute_cost_table
                .get_inflated_program_units(&key2)
                .unwrap()
        );
    }

    #[test]
    fn test_execute_cost_table_upsert_exceeds_capacity() {
        solana_logger::setup();
        let capacity: usize = 2;
        let mut execute_cost_table = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let cost1: u64 = 100;
        let cost2: u64 = 110;
        let cost3: u64 = 120;
        let cost4: u64 = 130;

        // insert one record
        execute_cost_table.upsert(&key1, cost1);
        assert_eq!(1, execute_cost_table.get_count());
        assert_eq!(
            cost1,
            execute_cost_table
                .get_inflated_program_units(&key1)
                .unwrap()
        );

        // insert 2nd record
        execute_cost_table.upsert(&key2, cost2);
        assert_eq!(2, execute_cost_table.get_count());
        assert_eq!(
            cost1,
            execute_cost_table
                .get_inflated_program_units(&key1)
                .unwrap()
        );
        assert_eq!(
            cost2,
            execute_cost_table
                .get_inflated_program_units(&key2)
                .unwrap()
        );

        // insert 3rd record, pushes out the oldest (eg 1st) record
        execute_cost_table.upsert(&key3, cost3);
        assert_eq!(2, execute_cost_table.get_count());
        assert_eq!(
            (cost2 + cost3) / 2_u64,
            execute_cost_table.get_average_units()
        );
        assert_eq!(cost3, execute_cost_table.get_statistical_mode_units());
        assert!(execute_cost_table
            .get_inflated_program_units(&key1)
            .is_none());
        assert_eq!(
            cost2,
            execute_cost_table
                .get_inflated_program_units(&key2)
                .unwrap()
        );
        assert_eq!(
            cost3,
            execute_cost_table
                .get_inflated_program_units(&key3)
                .unwrap()
        );

        // update 2nd record, so the 3rd becomes the oldest
        // add 4th record, pushes out 3rd key
        execute_cost_table.upsert(&key2, cost2);
        execute_cost_table.upsert(&key4, cost4);
        assert_eq!(
            (cost2 + cost4) / 2_u64,
            execute_cost_table.get_average_units()
        );
        assert_eq!(cost2, execute_cost_table.get_statistical_mode_units());
        assert_eq!(2, execute_cost_table.get_count());
        assert!(execute_cost_table
            .get_inflated_program_units(&key1)
            .is_none());
        assert_eq!(
            cost2,
            execute_cost_table
                .get_inflated_program_units(&key2)
                .unwrap()
        );
        assert!(execute_cost_table
            .get_inflated_program_units(&key3)
            .is_none());
        assert_eq!(
            cost4,
            execute_cost_table
                .get_inflated_program_units(&key4)
                .unwrap()
        );
    }

    #[test]
    fn test_aggregate_variance_stats() {
        solana_logger::setup();
        let cost: u64 = u64::MAX;

        // set initial value at MAX
        let mut aggregated_variance_stats = AggregatedVarianceStats {
            ema: cost,
            ema_var: 0,
        };
        assert_eq!(cost, aggregated_variance_stats.get_ema());
        assert_eq!(0, aggregated_variance_stats.get_stddev());

        // expected variance should be same as theta after rounding
        let expected_var = 100u64;
        // insert a positive theta, ema will be saturated, hence remain at its MAX value
        {
            let theta = 100i128;
            aggregated_variance_stats.aggregate_ema(theta);
            aggregated_variance_stats.aggregate_variance(theta);
            assert_eq!(cost, aggregated_variance_stats.get_ema());
            assert_eq!(expected_var, aggregated_variance_stats.get_stddev().pow(2));
        }

        // insert a negative theta, would reduce ema, increase variance
        {
            let theta = -100i128;
            aggregated_variance_stats.aggregate_ema(theta);
            aggregated_variance_stats.aggregate_variance(theta);
            assert!(cost > aggregated_variance_stats.get_ema());
            assert!(expected_var < aggregated_variance_stats.get_stddev().pow(2));
        }
    }

    #[test]
    fn test_get_inflated_program_units() {
        solana_logger::setup();
        let mut execute_cost_table = ExecuteCostTable::default();
        let key = Pubkey::new_unique();

        // init datum
        let cost1 = 1_000u64;
        execute_cost_table.upsert(&key, cost1);
        assert_eq!(
            cost1,
            execute_cost_table.get_average_program_units(&key).unwrap()
        );
        assert_eq!(
            cost1,
            execute_cost_table.get_inflated_program_units(&key).unwrap()
        );

        // 2nd datum, higher than last average
        {
            let theta = 500u64;
            let cost2 = execute_cost_table.get_average_program_units(&key).unwrap() + theta;
            let expected_ema = 1005u64; // ema+theta*EMA_ALPHA
            let expected_inflated_units = 1105u64; //ema+2*stddev
            execute_cost_table.upsert(&key, cost2);
            assert_eq!(
                expected_ema,
                execute_cost_table.get_average_program_units(&key).unwrap()
            );
            assert_eq!(
                expected_inflated_units,
                execute_cost_table.get_inflated_program_units(&key).unwrap()
            );
        }

        // 3rd datum, lower than last average
        {
            let theta = 500u64;
            let cost3 = execute_cost_table.get_average_program_units(&key).unwrap() - theta;
            let expected_ema = 1000u64; // ema+theta*EMA_ALPHA
            let expected_inflated_units = 1142u64; //ema+2*stddev
            execute_cost_table.upsert(&key, cost3);
            assert_eq!(
                expected_ema,
                execute_cost_table.get_average_program_units(&key).unwrap()
            );
            assert_eq!(
                expected_inflated_units,
                execute_cost_table.get_inflated_program_units(&key).unwrap()
            );
        }
    }
}
