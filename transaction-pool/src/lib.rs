use solana_sdk::pubkey::Pubkey;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;

#[derive(Default, Clone, PartialEq, Debug)]
pub struct Item {
    pub priority: u64,
    pub units: u64,
    pub time: u64,
    pub id: (u32, u32),
}

#[derive(Default)]
pub struct Pool {
    pub max_age: u64,
    pub max_bucket_cu: u64,
    pub max_block_cu: u64,
    items: BTreeMap<u64, VecDeque<Item>>,
}

pub trait Table {
    fn keys(&self, item: &Item) -> &[&Pubkey];
}

impl Pool {
    pub fn insert(&mut self, item: Item) {
        let bucket = self
            .items
            .entry(item.priority)
            .or_insert_with(VecDeque::new);
        bucket.push_back(item);
    }
    pub fn pop_block<T: Table>(&mut self, time: u64, table: &T) -> VecDeque<Item> {
        let mut rv = VecDeque::new();
        let mut total_cu: u64 = 0;
        let mut block_full = false;
        let mut buckets: HashMap<Pubkey, u64> = HashMap::new();
        let mut gc = vec![];
        for (k, v) in &mut self.items.iter_mut() {
            while v.front().is_some() {
                let item = v.front().unwrap();
                if time > self.max_age.saturating_add(item.time) {
                    v.pop_front();
                    break;
                }
                if total_cu.saturating_add(item.units) > self.max_block_cu {
                    block_full = true;
                    break;
                }
                let mut bucket_full = false;
                let keys = table.keys(item);
                for k in keys {
                    if buckets.get(k).is_none() {
                        continue;
                    }
                    if buckets[k].saturating_add(item.units) > self.max_bucket_cu {
                        bucket_full = true;
                        break;
                    }
                }
                if bucket_full {
                    continue;
                }
                for k in keys {
                    *buckets.entry(**k).or_insert(0) += item.units;
                }
                total_cu += item.units;
                rv.push_back(v.pop_front().unwrap());
            }
            if v.front().is_none() {
                gc.push(k);
            }
            if block_full {
                break;
            }
        }
        rv
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    struct Test<'a> {
        keys: &'a [&'a Pubkey],
    }

    impl Table for Test<'_> {
        fn keys(&self, item: &Item) -> &[&Pubkey] {
            &self.keys[item.priority as usize % 5..self.keys.len()]
        }
    }

    #[test]
    fn test_pool() {
        let mut pool = Pool::default();
        pool.max_block_cu = 1;
        pool.max_bucket_cu = 1;
        let item = Item {
            priority: 0,
            units: 1,
            time: 2,
            id: (3, 4),
        };
        pool.insert(item.clone());
        let test = Test { keys: &[] };
        let block = pool.pop_block(0, &test);
        assert_eq!(item, block[0]);
    }
}
