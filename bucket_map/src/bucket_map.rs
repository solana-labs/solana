//! BucketMap is a mostly contention free concurrent map backed by MmapMut

use {
    crate::{
        bucket_api::BucketApi, bucket_stats::BucketMapStats, restart::Restart, MaxSearch, RefCount,
    },
    solana_sdk::pubkey::Pubkey,
    std::{
        convert::TryInto,
        fmt::Debug,
        fs::{self},
        path::PathBuf,
        sync::{Arc, Mutex},
    },
    tempfile::TempDir,
};

#[derive(Debug, Default, Clone)]
pub struct BucketMapConfig {
    pub max_buckets: usize,
    pub drives: Option<Vec<PathBuf>>,
    pub max_search: Option<MaxSearch>,
    /// A file with a known path where the current state of the bucket files on disk is saved as the index is running.
    /// This file can be used to restore the index files as they existed prior to the process being stopped.
    pub restart_config_file: Option<PathBuf>,
}

impl BucketMapConfig {
    /// Create a new BucketMapConfig
    /// NOTE: BucketMap requires that max_buckets is a power of two
    pub fn new(max_buckets: usize) -> BucketMapConfig {
        BucketMapConfig {
            max_buckets,
            ..BucketMapConfig::default()
        }
    }
}

pub struct BucketMap<T: Clone + Copy + Debug + PartialEq + 'static> {
    buckets: Vec<Arc<BucketApi<T>>>,
    drives: Arc<Vec<PathBuf>>,
    max_buckets_pow2: u8,
    pub stats: Arc<BucketMapStats>,
    pub temp_dir: Option<TempDir>,
    /// true if dropping self removes all folders.
    /// This is primarily for test environments.
    pub erase_drives_on_drop: bool,
}

impl<T: Clone + Copy + Debug + PartialEq> Drop for BucketMap<T> {
    fn drop(&mut self) {
        if self.temp_dir.is_none() && self.erase_drives_on_drop {
            BucketMap::<T>::erase_previous_drives(&self.drives);
        }
    }
}

impl<T: Clone + Copy + Debug + PartialEq> Debug for BucketMap<T> {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

// this should be <= 1 << DEFAULT_CAPACITY or we end up searching the same items over and over - probably not a big deal since it is so small anyway
pub(crate) const MAX_SEARCH_DEFAULT: MaxSearch = 32;

/// used to communicate resize necessary and current size.
#[derive(Debug)]
pub enum BucketMapError {
    /// (bucket_index, current_capacity_pow2)
    /// Note that this is specific to data buckets, which grow in powers of 2
    DataNoSpace((u64, u8)),

    /// current_capacity_entries
    /// Note that this is specific to index buckets, which can be 'Actual' sizes
    IndexNoSpace(u64),
}

impl<T: Clone + Copy + Debug + PartialEq> BucketMap<T> {
    pub fn new(config: BucketMapConfig) -> Self {
        assert_ne!(
            config.max_buckets, 0,
            "Max number of buckets must be non-zero"
        );
        assert!(
            config.max_buckets.is_power_of_two(),
            "Max number of buckets must be a power of two"
        );
        let max_search = config.max_search.unwrap_or(MAX_SEARCH_DEFAULT);

        let mut restart = Restart::get_restart_file(&config);

        if restart.is_none() {
            // If we were able to load a restart file from the previous run, then don't wipe the accounts index drives from last time.
            // Unused files will be wiped by `get_restartable_buckets`
            if let Some(drives) = config.drives.as_ref() {
                Self::erase_previous_drives(drives);
            }
        }

        let stats = Arc::default();

        if restart.is_none() {
            restart = Restart::new(&config);
        }

        let mut temp_dir = None;
        let drives = config.drives.unwrap_or_else(|| {
            temp_dir = Some(TempDir::new().unwrap());
            vec![temp_dir.as_ref().unwrap().path().to_path_buf()]
        });
        let drives = Arc::new(drives);

        let restart = restart.map(|restart| Arc::new(Mutex::new(restart)));

        let restartable_buckets =
            Restart::get_restartable_buckets(restart.as_ref(), &drives, config.max_buckets);

        let buckets = restartable_buckets
            .into_iter()
            .map(|restartable_bucket| {
                Arc::new(BucketApi::new(
                    Arc::clone(&drives),
                    max_search,
                    Arc::clone(&stats),
                    restartable_bucket,
                ))
            })
            .collect();

        // A simple log2 function that is correct if x is a power of two
        let log2 = |x: usize| usize::BITS - x.leading_zeros() - 1;

        Self {
            buckets,
            drives,
            max_buckets_pow2: log2(config.max_buckets) as u8,
            stats,
            temp_dir,
            // if we are keeping track of restart, then don't wipe the drives on drop
            erase_drives_on_drop: restart.is_none(),
        }
    }

    fn erase_previous_drives(drives: &[PathBuf]) {
        drives.iter().for_each(|folder| {
            let _ = fs::remove_dir_all(folder);
            let _ = fs::create_dir_all(folder);
        })
    }

    pub fn num_buckets(&self) -> usize {
        self.buckets.len()
    }

    /// Get the values for Pubkey `key`
    pub fn read_value(&self, key: &Pubkey) -> Option<(Vec<T>, RefCount)> {
        self.get_bucket(key).read_value(key)
    }

    /// Delete the Pubkey `key`
    pub fn delete_key(&self, key: &Pubkey) {
        self.get_bucket(key).delete_key(key);
    }

    /// Update Pubkey `key`'s value with 'value'
    pub fn insert(&self, key: &Pubkey, value: (&[T], RefCount)) {
        self.get_bucket(key).insert(key, value)
    }

    /// Update Pubkey `key`'s value with 'value'
    pub fn try_insert(&self, key: &Pubkey, value: (&[T], RefCount)) -> Result<(), BucketMapError> {
        self.get_bucket(key).try_write(key, value)
    }

    /// Update Pubkey `key`'s value with function `updatefn`
    pub fn update<F>(&self, key: &Pubkey, updatefn: F)
    where
        F: FnMut(Option<(&[T], RefCount)>) -> Option<(Vec<T>, RefCount)>,
    {
        self.get_bucket(key).update(key, updatefn)
    }

    pub fn get_bucket(&self, key: &Pubkey) -> &Arc<BucketApi<T>> {
        self.get_bucket_from_index(self.bucket_ix(key))
    }

    pub fn get_bucket_from_index(&self, ix: usize) -> &Arc<BucketApi<T>> {
        &self.buckets[ix]
    }

    /// Get the bucket index for Pubkey `key`
    pub fn bucket_ix(&self, key: &Pubkey) -> usize {
        if self.max_buckets_pow2 > 0 {
            let location = read_be_u64(key.as_ref());
            (location >> (u64::BITS - self.max_buckets_pow2 as u32)) as usize
        } else {
            0
        }
    }
}

/// Look at the first 8 bytes of the input and reinterpret them as a u64
fn read_be_u64(input: &[u8]) -> u64 {
    assert!(input.len() >= std::mem::size_of::<u64>());
    u64::from_be_bytes(input[0..std::mem::size_of::<u64>()].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::index_entry::MAX_LEGAL_REFCOUNT,
        rand::{thread_rng, Rng},
        std::{collections::HashMap, sync::RwLock},
    };

    #[test]
    fn bucket_map_test_insert() {
        let key = Pubkey::new_unique();
        let config = BucketMapConfig::new(1 << 1);
        let index = BucketMap::new(config);
        index.update(&key, |_| Some((vec![0], 0)));
        assert_eq!(index.read_value(&key), Some((vec![0], 0)));
    }

    #[test]
    fn bucket_map_test_insert2() {
        for pass in 0..3 {
            let key = Pubkey::new_unique();
            let config = BucketMapConfig::new(1 << 1);
            let index = BucketMap::new(config);
            let bucket = index.get_bucket(&key);
            if pass == 0 {
                index.insert(&key, (&[0], 0));
            } else {
                let result = index.try_insert(&key, (&[0], 0));
                assert!(result.is_err());
                assert_eq!(index.read_value(&key), None);
                if pass == 2 {
                    // another call to try insert again - should still return an error
                    let result = index.try_insert(&key, (&[0], 0));
                    assert!(result.is_err());
                    assert_eq!(index.read_value(&key), None);
                }
                bucket.grow(result.unwrap_err());
                let result = index.try_insert(&key, (&[0], 0));
                assert!(result.is_ok());
            }
            assert_eq!(index.read_value(&key), Some((vec![0], 0)));
        }
    }

    #[test]
    fn bucket_map_test_update2() {
        let key = Pubkey::new_unique();
        let config = BucketMapConfig::new(1 << 1);
        let index = BucketMap::new(config);
        index.insert(&key, (&[0], 0));
        assert_eq!(index.read_value(&key), Some((vec![0], 0)));
        index.insert(&key, (&[1], 0));
        assert_eq!(index.read_value(&key), Some((vec![1], 0)));
    }

    #[test]
    fn bucket_map_test_update() {
        let key = Pubkey::new_unique();
        let config = BucketMapConfig::new(1 << 1);
        let index = BucketMap::new(config);
        index.update(&key, |_| Some((vec![0], 0)));
        assert_eq!(index.read_value(&key), Some((vec![0], 0)));
        index.update(&key, |_| Some((vec![1], 0)));
        assert_eq!(index.read_value(&key), Some((vec![1], 0)));
    }

    #[test]
    fn bucket_map_test_update_to_0_len() {
        solana_logger::setup();
        let key = Pubkey::new_unique();
        let config = BucketMapConfig::new(1 << 1);
        let index = BucketMap::new(config);
        index.update(&key, |_| Some((vec![0], 1)));
        assert_eq!(index.read_value(&key), Some((vec![0], 1)));
        // sets len to 0, updates in place
        index.update(&key, |_| Some((vec![], 1)));
        assert_eq!(index.read_value(&key), Some((vec![], 1)));
        // sets len to 0, doesn't update in place - finds a new place, which causes us to no longer have an allocation in data
        index.update(&key, |_| Some((vec![], 2)));
        assert_eq!(index.read_value(&key), Some((vec![], 2)));
        // sets len to 1, doesn't update in place - finds a new place
        index.update(&key, |_| Some((vec![1], 2)));
        assert_eq!(index.read_value(&key), Some((vec![1], 2)));
    }

    #[test]
    fn bucket_map_test_delete() {
        let config = BucketMapConfig::new(1 << 1);
        let index = BucketMap::new(config);
        for i in 0..10 {
            let key = Pubkey::new_unique();
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(&key), Some((vec![i], 0)));

            index.delete_key(&key);
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(&key), Some((vec![i], 0)));
            index.delete_key(&key);
        }
    }

    #[test]
    fn bucket_map_test_delete_2() {
        let config = BucketMapConfig::new(1 << 2);
        let index = BucketMap::new(config);
        for i in 0..100 {
            let key = Pubkey::new_unique();
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(&key), Some((vec![i], 0)));

            index.delete_key(&key);
            assert_eq!(index.read_value(&key), None);

            index.update(&key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(&key), Some((vec![i], 0)));
            index.delete_key(&key);
        }
    }

    #[test]
    fn bucket_map_test_n_drives() {
        let config = BucketMapConfig::new(1 << 2);
        let index = BucketMap::new(config);
        for i in 0..100 {
            let key = Pubkey::new_unique();
            index.update(&key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(&key), Some((vec![i], 0)));
        }
    }
    #[test]
    fn bucket_map_test_grow_read() {
        let config = BucketMapConfig::new(1 << 2);
        let index = BucketMap::new(config);
        let keys: Vec<Pubkey> = (0..100).map(|_| Pubkey::new_unique()).collect();
        for k in 0..keys.len() {
            let key = &keys[k];
            let i = read_be_u64(key.as_ref());
            index.update(key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(key), Some((vec![i], 0)));
            for (ix, key) in keys.iter().enumerate() {
                let i = read_be_u64(key.as_ref());
                //debug!("READ: {:?} {}", key, i);
                let expected = if ix <= k { Some((vec![i], 0)) } else { None };
                assert_eq!(index.read_value(key), expected);
            }
        }
    }

    #[test]
    fn bucket_map_test_n_delete() {
        let config = BucketMapConfig::new(1 << 2);
        let index = BucketMap::new(config);
        let keys: Vec<Pubkey> = (0..20).map(|_| Pubkey::new_unique()).collect();
        for key in keys.iter() {
            let i = read_be_u64(key.as_ref());
            index.update(key, |_| Some((vec![i], 0)));
            assert_eq!(index.read_value(key), Some((vec![i], 0)));
        }
        for key in keys.iter() {
            let i = read_be_u64(key.as_ref());
            //debug!("READ: {:?} {}", key, i);
            assert_eq!(index.read_value(key), Some((vec![i], 0)));
        }
        for k in 0..keys.len() {
            let key = &keys[k];
            index.delete_key(key);
            assert_eq!(index.read_value(key), None);
            for key in keys.iter().skip(k + 1) {
                let i = read_be_u64(key.as_ref());
                assert_eq!(index.read_value(key), Some((vec![i], 0)));
            }
        }
    }

    #[test]
    fn hashmap_compare() {
        use std::sync::Mutex;
        solana_logger::setup();
        for mut use_batch_insert in [true, false] {
            let maps = (0..2)
                .map(|max_buckets_pow2| {
                    let config = BucketMapConfig::new(1 << max_buckets_pow2);
                    BucketMap::new(config)
                })
                .collect::<Vec<_>>();
            let hash_map = RwLock::new(HashMap::<Pubkey, (Vec<(usize, usize)>, RefCount)>::new());
            let max_slot_list_len = 5;
            let all_keys = Mutex::new(vec![]);

            let gen_rand_value = || {
                let count = thread_rng().gen_range(0..max_slot_list_len);
                let v = (0..count)
                    .map(|x| (x as usize, x as usize /*thread_rng().gen::<usize>()*/))
                    .collect::<Vec<_>>();
                let range = thread_rng().gen_range(0..100);
                // pick ref counts that are useful and common
                let rc = if range < 50 {
                    1
                } else if range < 60 {
                    0
                } else if range < 70 {
                    2
                } else {
                    thread_rng().gen_range(0..MAX_LEGAL_REFCOUNT)
                };

                (v, rc)
            };

            let get_key = || {
                let mut keys = all_keys.lock().unwrap();
                if keys.is_empty() {
                    return None;
                }
                let len = keys.len();
                Some(keys.remove(thread_rng().gen_range(0..len)))
            };
            let return_key = |key| {
                let mut keys = all_keys.lock().unwrap();
                keys.push(key);
            };

            let verify = || {
                let expected_count = hash_map.read().unwrap().len();
                let mut maps = maps
                    .iter()
                    .map(|map| {
                        let total_entries = (0..map.num_buckets())
                            .map(|bucket| map.get_bucket_from_index(bucket).bucket_len() as usize)
                            .sum::<usize>();
                        assert_eq!(total_entries, expected_count);
                        let mut r = vec![];
                        for bin in 0..map.num_buckets() {
                            r.append(
                                &mut map.buckets[bin]
                                    .items_in_range(&None::<&std::ops::RangeInclusive<Pubkey>>),
                            );
                        }
                        r
                    })
                    .collect::<Vec<_>>();
                let hm = hash_map.read().unwrap();
                for (k, v) in hm.iter() {
                    for map in maps.iter_mut() {
                        for i in 0..map.len() {
                            if k == &map[i].pubkey {
                                assert_eq!(map[i].slot_list, v.0);
                                assert_eq!(map[i].ref_count, v.1);
                                map.remove(i);
                                break;
                            }
                        }
                    }
                }
                for map in maps.iter() {
                    assert!(map.is_empty());
                }
            };
            let mut initial: usize = 100; // put this many items in to start
            if use_batch_insert {
                // insert a lot more when inserting with batch to make sure we hit resizing during batch
                initial *= 3;
            }

            // do random operations: insert, update, delete, add/unref in random order
            // verify consistency between hashmap and all bucket maps
            for i in 0..10000 {
                initial = initial.saturating_sub(1);
                if initial > 0 || thread_rng().gen_range(0..5) == 0 {
                    // insert
                    let mut to_add = 1;
                    if initial > 1 && use_batch_insert {
                        to_add = thread_rng().gen_range(1..(initial / 4).max(2));
                        initial -= to_add;
                    }

                    let additions = (0..to_add)
                        .map(|_| {
                            let k = solana_sdk::pubkey::new_rand();
                            let mut v = gen_rand_value();
                            if use_batch_insert {
                                // refcount has to be 1 to use batch insert
                                v.1 = 1;
                                // len has to be 1 to use batch insert
                                if v.0.len() > 1 {
                                    v.0.truncate(1);
                                } else if v.0.is_empty() {
                                    loop {
                                        let mut new_v = gen_rand_value();
                                        if !new_v.0.is_empty() {
                                            v.0 = vec![new_v.0.pop().unwrap()];
                                            break;
                                        }
                                    }
                                }
                            }
                            (k, v)
                        })
                        .collect::<Vec<_>>();

                    additions.clone().into_iter().for_each(|(k, v)| {
                        hash_map.write().unwrap().insert(k, v);
                        return_key(k);
                    });
                    let insert = thread_rng().gen_range(0..2) == 0;
                    maps.iter().for_each(|map| {
                        // batch insert can only work for the map with only 1 bucket so that we can batch add to a single bucket
                        let batch_insert_now = map.buckets.len() == 1
                            && use_batch_insert
                            && thread_rng().gen_range(0..2) == 0;
                        if batch_insert_now {
                            // batch insert into the map with 1 bucket 50% of the time
                            let mut batch_additions = additions
                                .clone()
                                .into_iter()
                                .map(|(k, mut v)| (k, v.0.pop().unwrap()))
                                .collect::<Vec<_>>();
                            let mut duplicates = 0;
                            if batch_additions.len() > 1 && thread_rng().gen_range(0..2) == 0 {
                                // insert a duplicate sometimes
                                let item_to_duplicate =
                                    thread_rng().gen_range(0..batch_additions.len());
                                let where_to_insert_duplicate =
                                    thread_rng().gen_range(0..batch_additions.len());
                                batch_additions.insert(
                                    where_to_insert_duplicate,
                                    batch_additions[item_to_duplicate],
                                );
                                duplicates += 1;
                            }
                            assert_eq!(
                                map.get_bucket_from_index(0)
                                    .batch_insert_non_duplicates(&batch_additions,)
                                    .len(),
                                duplicates
                            );
                        } else {
                            additions.clone().into_iter().for_each(|(k, v)| {
                                if insert {
                                    map.insert(&k, (&v.0, v.1))
                                } else {
                                    map.update(&k, |current| {
                                        assert!(current.is_none());
                                        Some(v.clone())
                                    })
                                }
                            });
                        }
                    });

                    if use_batch_insert && initial == 1 {
                        // done using batch insert once we have added the initial entries
                        // now, the test can remove, update, addref, etc.
                        use_batch_insert = false;
                    }
                }
                if use_batch_insert && initial > 0 {
                    // if we are using batch insert, it is illegal to update, delete, or addref/unref an account until all batch inserts are complete
                    continue;
                }
                if thread_rng().gen_range(0..10) == 0 {
                    // update
                    if let Some(k) = get_key() {
                        let hm = hash_map.read().unwrap();
                        let (v, rc) = gen_rand_value();
                        let v_old = hm.get(&k);
                        let insert = thread_rng().gen_range(0..2) == 0;
                        maps.iter().for_each(|map| {
                            if insert {
                                map.insert(&k, (&v, rc))
                            } else {
                                map.update(&k, |current| {
                                    assert_eq!(current, v_old.map(|(v, rc)| (&v[..], *rc)), "{k}");
                                    Some((v.clone(), rc))
                                })
                            }
                        });
                        drop(hm);
                        hash_map.write().unwrap().insert(k, (v, rc));
                        return_key(k);
                    }
                }
                if thread_rng().gen_range(0..20) == 0 {
                    // delete
                    if let Some(k) = get_key() {
                        let mut hm = hash_map.write().unwrap();
                        hm.remove(&k);
                        maps.iter().for_each(|map| {
                            map.delete_key(&k);
                        });
                    }
                }
                if thread_rng().gen_range(0..10) == 0 {
                    // add/unref
                    if let Some(k) = get_key() {
                        let mut inc = thread_rng().gen_range(0..2) == 0;
                        let mut hm = hash_map.write().unwrap();
                        let (v, mut rc) = hm.get(&k).map(|(v, rc)| (v.to_vec(), *rc)).unwrap();
                        if !inc && rc == 0 {
                            // can't decrement rc=0
                            inc = true;
                        }
                        rc = if inc { rc + 1 } else { rc - 1 };
                        hm.insert(k, (v.to_vec(), rc));
                        maps.iter().for_each(|map| {
                            map.update(&k, |current| Some((current.unwrap().0.to_vec(), rc)))
                        });

                        return_key(k);
                    }
                }
                if i % 1000 == 0 {
                    verify();
                }
            }
            verify();
        }
    }
}
