use ahash::AHasher;
use rand::thread_rng;
use rand::Rng;
use solana_sdk::pubkey::Pubkey;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hasher;

#[derive(Default, Clone, PartialEq, Debug)]
pub struct Item {
    pub priority: u64,
    pub units: u64,
    pub time: u64,
    pub id: (u32, u32),
}

#[derive(Default, Clone)]
pub struct Pool {
    pub max_age: u64,
    pub max_bucket_cu: u64,
    pub max_block_cu: u64,
    pub count: usize,
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
        self.count += 1;
    }
    pub fn pop_block<T: Table>(&mut self, time: u64, table: &T) -> VecDeque<Item> {
        let mut rv = VecDeque::new();
        let mut total_cu: u64 = 0;
        let mut block_full = false;
        let seed: (u128, u128) = thread_rng().gen();
        let mut buckets = vec![0u64; u16::MAX.into()];
        let mut gc = vec![];
        for (k, v) in &mut self.items.iter_mut() {
            let mut retry = VecDeque::new();
            while v.front().is_some() {
                let item = v.front().unwrap();
                if time > self.max_age.saturating_add(item.time) {
                    v.pop_front();
                    self.count -= 1;
                    break;
                }
                if total_cu.saturating_add(item.units) > self.max_block_cu {
                    block_full = true;
                    break;
                }
                let mut bucket_full = false;
                let keys: Vec<usize> = table
                    .keys(item)
                    .iter()
                    .map(|x| {
                        let mut hasher = AHasher::new_with_keys(seed.0, seed.1);
                        hasher.write(x.as_ref());
                        (hasher.finish() % u64::from(u16::MAX)).try_into().unwrap()
                    })
                    .collect();

                for k in &keys {
                    if buckets[*k].saturating_add(item.units) > self.max_bucket_cu {
                        bucket_full = true;
                        break;
                    }
                }
                if bucket_full {
                    retry.push_back(v.pop_front().unwrap());
                    continue;
                }
                for k in &keys {
                    let ix = buckets[*k] = buckets[*k].saturating_add(item.units);
                }
                total_cu = total_cu.saturating_add(item.units);
                rv.push_back(v.pop_front().unwrap());
                self.count -= 1;
            }
            v.append(&mut retry);
            if v.front().is_none() {
                gc.push(*k);
            }
            if block_full {
                break;
            }
        }
        for k in &gc {
            self.items.remove(k);
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
    #[test]
    fn test_pool_1() {
        let mut pool = Pool::default();
        for i in 0..1000 {
            pool.insert(Item {
                priority: thread_rng().gen_range(1, 100),
                units: thread_rng().gen_range(1, 100),
                time: thread_rng().gen_range(1, 100),
                id: (i, i),
            });
        }
        //10x smaller then a block
        //ave units is 32
        pool.max_block_cu = 10_000;
        pool.max_bucket_cu = 2_500;
        pool.max_age = 101;
        let keys: &[&Pubkey] = &[
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
        ];
        let test = Test { keys };
        assert_eq!(pool.count, 1000);
        let rv = pool.pop_block(0, &test);
        assert!(rv.len() > 0);
        assert_eq!(pool.count + rv.len(), 1000);
    }
}
