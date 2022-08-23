/// ExecuteCostTable is aggregated by Cost Model, it keeps each program's
/// average cost in its HashMap, with fixed capacity to avoid from growing
/// unchecked.
/// When its capacity limit is reached, it prunes old and less-used programs
/// to make room for new ones.
use {
    log::*, solana_program_runtime::compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
    solana_sdk::pubkey::Pubkey, std::collections::HashMap,
};

// prune is rather expensive op, free up bulk space in each operation
// would be more efficient. PRUNE_RATIO defines that after prune, table
// size will be original_size * PRUNE_RATIO. The value is defined in
// scale of 100.
const PRUNE_RATIO: usize = 75;
// with 50_000 TPS as norm, weights occurrences '100' per microsec
const OCCURRENCES_WEIGHT: i64 = 100;

const DEFAULT_CAPACITY: usize = 1024;

#[derive(AbiExample, Debug)]
pub struct ExecuteCostTable {
    capacity: usize,
    table: HashMap<Pubkey, u64>,
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

    pub fn get_default_compute_unit_limit(&self) -> u64 {
        DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64
    }

    /// average cost of all recorded programs
    pub fn get_global_average_program_cost(&self) -> u64 {
        if self.table.is_empty() {
            self.get_default_compute_unit_limit()
        } else {
            self.table.iter().map(|(_, value)| value).sum::<u64>() / self.get_count() as u64
        }
    }

    /// the most frequently occurring program's cost
    pub fn get_statistical_mode_program_cost(&self) -> u64 {
        if self.occurrences.is_empty() {
            self.get_default_compute_unit_limit()
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
    /// `get_default_compute_unit_limit()`, `get_global_average_program_cost()`
    /// or `get_statistical_mode_program_cost()` can be used to assign a value
    /// to new program.
    pub fn get_cost(&self, key: &Pubkey) -> Option<&u64> {
        self.table.get(key)
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

        let program_cost = self.table.entry(*key).or_insert(value);
        *program_cost = (*program_cost + value) / 2;

        let (count, timestamp) = self
            .occurrences
            .entry(*key)
            .or_insert((0, Self::micros_since_epoch()));
        *count += 1;
        *timestamp = Self::micros_since_epoch();
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
        assert_eq!(cost1, testee.get_global_average_program_cost());
        assert_eq!(cost1, testee.get_statistical_mode_program_cost());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());

        // insert 2nd record
        testee.upsert(&key2, cost2);
        assert_eq!(2, testee.get_count());
        assert_eq!(
            (cost1 + cost2) / 2_u64,
            testee.get_global_average_program_cost()
        );
        assert_eq!(cost2, testee.get_statistical_mode_program_cost());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());

        // update 1st record
        testee.upsert(&key1, cost2);
        assert_eq!(2, testee.get_count());
        assert_eq!(
            ((cost1 + cost2) / 2 + cost2) / 2_u64,
            testee.get_global_average_program_cost()
        );
        assert_eq!(
            (cost1 + cost2) / 2,
            testee.get_statistical_mode_program_cost()
        );
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
        testee.upsert(&key1, cost1);
        assert_eq!(1, testee.get_count());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());

        // insert 2nd record
        testee.upsert(&key2, cost2);
        assert_eq!(2, testee.get_count());
        assert_eq!(&cost1, testee.get_cost(&key1).unwrap());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());

        // insert 3rd record, pushes out the oldest (eg 1st) record
        testee.upsert(&key3, cost3);
        assert_eq!(2, testee.get_count());
        assert_eq!(
            (cost2 + cost3) / 2_u64,
            testee.get_global_average_program_cost()
        );
        assert_eq!(cost3, testee.get_statistical_mode_program_cost());
        assert!(testee.get_cost(&key1).is_none());
        assert_eq!(&cost2, testee.get_cost(&key2).unwrap());
        assert_eq!(&cost3, testee.get_cost(&key3).unwrap());

        // update 2nd record, so the 3rd becomes the oldest
        // add 4th record, pushes out 3rd key
        testee.upsert(&key2, cost1);
        testee.upsert(&key4, cost4);
        assert_eq!(
            ((cost1 + cost2) / 2 + cost4) / 2_u64,
            testee.get_global_average_program_cost()
        );
        assert_eq!(
            (cost1 + cost2) / 2,
            testee.get_statistical_mode_program_cost()
        );
        assert_eq!(2, testee.get_count());
        assert!(testee.get_cost(&key1).is_none());
        assert_eq!(&((cost1 + cost2) / 2), testee.get_cost(&key2).unwrap());
        assert!(testee.get_cost(&key3).is_none());
        assert_eq!(&cost4, testee.get_cost(&key4).unwrap());
    }
}
