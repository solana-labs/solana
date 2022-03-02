/// ExecuteCostTable is aggregated by Cost Model, it keeps each program's
/// average cost in its HashMap, with fixed capacity to avoid from growing
/// unchecked.
/// When its capacity limit is reached, it prunes old and less-used programs
/// to make room for new ones.
use log::*;
use {
    solana_sdk::pubkey::Pubkey,
    std::collections::{hash_map::Entry, HashMap},
};

// prune is rather expensive op, free up bulk space in each operation
// would be more efficient. PRUNE_RATIO defines the after prune table
// size will be original_size * PRUNE_RATIO.
const PRUNE_RATIO: f64 = 0.75;
// with 50_000 TPS as norm, weights occurrences '100' per microsec
const OCCURRENCES_WEIGHT: i64 = 100;

const DEFAULT_CAPACITY: usize = 1024;

// The coefficient represents the degree of weighting decrease in EMA,
// a constant smoothing factor between 0 and 1. A higher alpha
// discounts older observations faster.
// Setting it using `2/(N+1)` where N is 200 samples
const COEFFICIENT: f64 = 0.01;

#[derive(Debug, Default)]
struct AggregatedVarianceStats {
    ema: f64,
    ema_var: f64,
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

    // number of programs in table
    pub fn get_count(&self) -> usize {
        self.table.len()
    }

    // default program cost to max
    pub fn get_default(&self) -> u64 {
        // default max compute units per program
        200_000u64
    }

    // returns None if program doesn't exist in table. In this case,
    // it is advised to call `get_default()` for default program cost.
    // Program cost is estimated as 2 standard deviations above mean, eg
    // cost = (mean + 2 * std)
    pub fn get_cost(&self, key: &Pubkey) -> Option<u64> {
        let aggregated = self.table.get(key)?;
        let cost_f64 = (aggregated.ema + 2.0 * aggregated.ema_var.sqrt()).ceil();

        // check if cost:f64 can be losslessly convert to u64, otherwise return None
        let cost_u64 = cost_f64 as u64;
        if cost_f64 == cost_u64 as f64 {
            Some(cost_u64)
        } else {
            None
        }
    }

    pub fn upsert(&mut self, key: &Pubkey, value: u64) {
        let need_to_add = !self.table.contains_key(key);
        let current_size = self.get_count();
        if current_size == self.capacity && need_to_add {
            self.prune_to(&((current_size as f64 * PRUNE_RATIO) as usize));
        }

        // exponential moving average algorithm
        // https://en.wikipedia.org/wiki/Moving_average#Exponentially_weighted_moving_variance_and_standard_deviation
        match self.table.entry(*key) {
            Entry::Occupied(mut entry) => {
                let aggregated = entry.get_mut();
                let theta = value as f64 - aggregated.ema;
                aggregated.ema += theta * COEFFICIENT;
                aggregated.ema_var =
                    (1.0 - COEFFICIENT) * (aggregated.ema_var + COEFFICIENT * theta * theta);
            }
            Entry::Vacant(entry) => {
                // the starting values
                entry.insert(AggregatedVarianceStats {
                    ema: value as f64,
                    ema_var: 0.0,
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

    pub fn get_program_keys(&self) -> Vec<&Pubkey> {
        self.table.keys().collect()
    }

    // prune the old programs so the table contains `new_size` of records,
    // where `old` is defined as weighted age, which is negatively correlated
    // with program's age and
    // positively correlated with how frequently the program
    // is executed (eg. occurrence),
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
        let mut testee = ExecuteCostTable::new(capacity);

        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();

        testee.upsert(&key1, 1);
        testee.upsert(&key2, 2);
        testee.upsert(&key3, 3);

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
            testee.upsert(&key1, i);
        }
        testee.upsert(&key2, 2);
        testee.upsert(&key3, 3);

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
        testee.upsert(&key1, cost1);
        assert_eq!(1, testee.get_count());
        assert_eq!(cost1, testee.get_cost(&key1).unwrap());

        // insert 2nd record
        testee.upsert(&key2, cost2);
        assert_eq!(2, testee.get_count());
        assert_eq!(cost1, testee.get_cost(&key1).unwrap());
        assert_eq!(cost2, testee.get_cost(&key2).unwrap());

        // update 1st record
        testee.upsert(&key1, cost2);
        assert_eq!(2, testee.get_count());
        // expected key1 cost is EMA of [100, 110] with alpha=0.01 => 103
        let expected_cost = 103;
        assert_eq!(expected_cost, testee.get_cost(&key1).unwrap());
        assert_eq!(cost2, testee.get_cost(&key2).unwrap());
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
        testee.upsert(&key1, cost1);
        assert_eq!(1, testee.get_count());
        assert_eq!(cost1, testee.get_cost(&key1).unwrap());

        // insert 2nd record
        testee.upsert(&key2, cost2);
        assert_eq!(2, testee.get_count());
        assert_eq!(cost1, testee.get_cost(&key1).unwrap());
        assert_eq!(cost2, testee.get_cost(&key2).unwrap());

        // insert 3rd record, pushes out the oldest (eg 1st) record
        testee.upsert(&key3, cost3);
        assert_eq!(2, testee.get_count());
        assert!(testee.get_cost(&key1).is_none());
        assert_eq!(cost2, testee.get_cost(&key2).unwrap());
        assert_eq!(cost3, testee.get_cost(&key3).unwrap());

        // update 2nd record, so the 3rd becomes the oldest
        // add 4th record, pushes out 3rd key
        testee.upsert(&key2, cost1);
        testee.upsert(&key4, cost4);
        assert_eq!(2, testee.get_count());
        assert!(testee.get_cost(&key1).is_none());
        // expected key2 cost = (mean + 2*std) of [110, 100] => 112
        let expected_cost_2 = 112;
        assert_eq!(expected_cost_2, testee.get_cost(&key2).unwrap());
        assert!(testee.get_cost(&key3).is_none());
        assert_eq!(cost4, testee.get_cost(&key4).unwrap());
    }

    #[test]
    fn test_get_cost_overflow_u64() {
        solana_logger::setup();
        let mut testee = ExecuteCostTable::default();

        let key1 = Pubkey::new_unique();
        let cost1: u64 = f64::MAX as u64;
        let cost2: u64 = u64::MAX / 2; // create large variance so the final result will overflow

        // insert one record
        testee.upsert(&key1, cost1);
        assert_eq!(1, testee.get_count());
        assert_eq!(cost1, testee.get_cost(&key1).unwrap());

        // update cost
        testee.upsert(&key1, cost2);
        assert!(testee.get_cost(&key1).is_none());
    }
}
