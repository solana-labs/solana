use solana_sdk::pubkey::Pubkey;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;

#[derive(Default)]
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
        let mut total_cu = 0;
        let mut block_full = false;
        let mut buckets: HashMap<Pubkey, u64> = HashMap::new();
        let mut gc = vec![];
        for (k, v) in &mut self.items.iter_mut() {
            while v.front().is_some() {
                let item = v.front().unwrap();
                if time - item.time > self.max_age {
                    v.pop_front();
                    break;
                }
                if total_cu + item.units > self.max_block_cu {
                    block_full = true;
                    break;
                }
                let mut bucket_full = false;
                let keys = table.keys(item);
                for k in keys {
                    if buckets.get(k).is_none() {
                        continue;
                    }
                    if buckets[k] + item.units > self.max_bucket_cu {
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
