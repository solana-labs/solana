use {
    super::{AccountStorageEntry, AccountsDb, BinnedHashData, LoadedAccount, SplitAncientStorages},
    crate::{
        accounts_hash::{
            AccountHash, CalcAccountsHashConfig, CalculateHashIntermediate, HashStats,
        },
        active_stats::ActiveStatItem,
        cache_hash_data::{CacheHashData, CacheHashDataFileReference},
        pubkey_bins::PubkeyBinCalculator24,
        sorted_storages::SortedStorages,
    },
    rayon::prelude::*,
    solana_measure::{measure::Measure, measure_us},
    solana_sdk::{account::ReadableAccount as _, clock::Slot, hash::Hash, pubkey::Pubkey},
    std::{
        hash::{DefaultHasher, Hash as _, Hasher as _},
        ops::Range,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    },
};

/// called on a struct while scanning append vecs
trait AppendVecScan: Send + Sync + Clone {
    /// return true if this pubkey should be included
    fn filter(&mut self, pubkey: &Pubkey) -> bool;
    /// set current slot of the scan
    fn set_slot(&mut self, slot: Slot, is_ancient: bool);
    /// found `account` in the append vec
    fn found_account(&mut self, account: &LoadedAccount);
    /// scanning is done
    fn scanning_complete(self) -> BinnedHashData;
    /// initialize accumulator
    fn init_accum(&mut self, count: usize);
}

#[derive(Clone)]
/// state to keep while scanning append vec accounts for hash calculation
/// These would have been captured in a fn from within the scan function.
/// Some of these are constant across all pubkeys, some are constant across a slot.
/// Some could be unique per pubkey.
struct ScanState<'a> {
    /// slot we're currently scanning
    current_slot: Slot,
    /// accumulated results
    accum: BinnedHashData,
    bin_calculator: &'a PubkeyBinCalculator24,
    bin_range: &'a Range<usize>,
    range: usize,
    sort_time: Arc<AtomicU64>,
    pubkey_to_bin_index: usize,
    is_ancient: bool,
    stats_num_zero_lamport_accounts_ancient: Arc<AtomicU64>,
}

impl<'a> AppendVecScan for ScanState<'a> {
    fn set_slot(&mut self, slot: Slot, is_ancient: bool) {
        self.current_slot = slot;
        self.is_ancient = is_ancient;
    }
    fn filter(&mut self, pubkey: &Pubkey) -> bool {
        self.pubkey_to_bin_index = self.bin_calculator.bin_from_pubkey(pubkey);
        self.bin_range.contains(&self.pubkey_to_bin_index)
    }
    fn init_accum(&mut self, count: usize) {
        if self.accum.is_empty() {
            self.accum.append(&mut vec![Vec::new(); count]);
        }
    }
    fn found_account(&mut self, loaded_account: &LoadedAccount) {
        let pubkey = loaded_account.pubkey();
        assert!(self.bin_range.contains(&self.pubkey_to_bin_index)); // get rid of this once we have confidence

        // when we are scanning with bin ranges, we don't need to use exact bin numbers.
        // Subtract to make first bin we care about at index 0.
        self.pubkey_to_bin_index -= self.bin_range.start;

        let balance = loaded_account.lamports();
        let mut account_hash = loaded_account.loaded_hash();

        let hash_is_missing = account_hash == AccountHash(Hash::default());
        if hash_is_missing {
            let computed_hash = AccountsDb::hash_account(loaded_account, loaded_account.pubkey());
            account_hash = computed_hash;
        }

        if balance == 0 && self.is_ancient {
            self.stats_num_zero_lamport_accounts_ancient
                .fetch_add(1, Ordering::Relaxed);
        }

        let source_item = CalculateHashIntermediate {
            hash: account_hash,
            lamports: balance,
            pubkey: *pubkey,
        };
        self.init_accum(self.range);
        self.accum[self.pubkey_to_bin_index].push(source_item);
    }
    fn scanning_complete(mut self) -> BinnedHashData {
        let timing = AccountsDb::sort_slot_storage_scan(&mut self.accum);
        self.sort_time.fetch_add(timing, Ordering::Relaxed);
        self.accum
    }
}

enum ScanAccountStorageResult {
    /// this data has already been scanned and cached
    CacheFileAlreadyExists(CacheHashDataFileReference),
    /// this data needs to be scanned and cached
    CacheFileNeedsToBeCreated((String, Range<Slot>)),
}

impl AccountsDb {
    /// scan 'storages', return a vec of 'CacheHashDataFileReference', one per pass
    pub(crate) fn scan_snapshot_stores_with_cache(
        &self,
        cache_hash_data: &CacheHashData,
        storages: &SortedStorages,
        stats: &mut HashStats,
        bins: usize,
        bin_range: &Range<usize>,
        config: &CalcAccountsHashConfig<'_>,
    ) -> Vec<CacheHashDataFileReference> {
        assert!(bin_range.start < bins);
        assert!(bin_range.end <= bins);
        assert!(bin_range.start < bin_range.end);
        let _guard = self.active_stats.activate(ActiveStatItem::HashScan);

        let bin_calculator = PubkeyBinCalculator24::new(bins);
        let mut time = Measure::start("scan all accounts");
        stats.num_snapshot_storage = storages.storage_count();
        stats.num_slots = storages.slot_count();
        let range = bin_range.end - bin_range.start;
        let sort_time = Arc::new(AtomicU64::new(0));

        let scanner = ScanState {
            current_slot: Slot::default(),
            accum: BinnedHashData::default(),
            bin_calculator: &bin_calculator,
            range,
            bin_range,
            sort_time: sort_time.clone(),
            pubkey_to_bin_index: 0,
            is_ancient: false,
            stats_num_zero_lamport_accounts_ancient: Arc::clone(
                &stats.num_zero_lamport_accounts_ancient,
            ),
        };

        let result = self.scan_account_storage_no_bank(
            cache_hash_data,
            config,
            storages,
            scanner,
            bin_range,
            stats,
        );

        stats.sort_time_total_us += sort_time.load(Ordering::Relaxed);

        time.stop();
        stats.scan_time_total_us += time.as_us();

        result
    }

    /// Scan through all the account storage in parallel.
    /// Returns a Vec of opened files.
    /// Each file has serialized hash info, sorted by pubkey and then slot, from scanning the append vecs.
    ///   A single pubkey could be in multiple entries. The pubkey found in the latest entry is the one to use.
    fn scan_account_storage_no_bank<S>(
        &self,
        cache_hash_data: &CacheHashData,
        config: &CalcAccountsHashConfig<'_>,
        snapshot_storages: &SortedStorages,
        scanner: S,
        bin_range: &Range<usize>,
        stats: &mut HashStats,
    ) -> Vec<CacheHashDataFileReference>
    where
        S: AppendVecScan,
    {
        let oldest_non_ancient_slot_for_split = self
            .get_oldest_non_ancient_slot_for_hash_calc_scan(
                snapshot_storages.max_slot_inclusive(),
                config,
            );
        let splitter =
            SplitAncientStorages::new(oldest_non_ancient_slot_for_split, snapshot_storages);
        let oldest_non_ancient_slot_for_identification = self
            .get_oldest_non_ancient_slot_from_slot(
                config.epoch_schedule,
                snapshot_storages.max_slot_inclusive(),
            );
        let slots_per_epoch = config
            .rent_collector
            .epoch_schedule
            .get_slots_in_epoch(config.rent_collector.epoch);
        let one_epoch_old = snapshot_storages
            .range()
            .end
            .saturating_sub(slots_per_epoch);

        stats.scan_chunks = splitter.chunk_count;

        let cache_files = (0..splitter.chunk_count)
            .into_par_iter()
            .filter_map(|chunk| {
                let range_this_chunk = splitter.get_slot_range(chunk)?;

                let mut load_from_cache = true;
                let mut hasher = DefaultHasher::new();
                bin_range.start.hash(&mut hasher);
                bin_range.end.hash(&mut hasher);
                let is_first_scan_pass = bin_range.start == 0;

                // calculate hash representing all storages in this chunk
                let mut empty = true;
                for (slot, storage) in snapshot_storages.iter_range(&range_this_chunk) {
                    empty = false;
                    if is_first_scan_pass && slot < one_epoch_old {
                        self.update_old_slot_stats(stats, storage);
                    }
                    if let Some(storage) = storage {
                        let ok = Self::hash_storage_info(&mut hasher, storage, slot);
                        if !ok {
                            load_from_cache = false;
                            break;
                        }
                    }
                }
                if empty {
                    return None;
                }
                // we have a hash value for the storages in this chunk
                // so, build a file name:
                let hash = hasher.finish();
                let file_name = format!(
                    "{}.{}.{}.{}.{:016x}",
                    range_this_chunk.start,
                    range_this_chunk.end,
                    bin_range.start,
                    bin_range.end,
                    hash
                );
                if load_from_cache {
                    if let Ok(mapped_file) =
                        cache_hash_data.get_file_reference_to_map_later(&file_name)
                    {
                        return Some(ScanAccountStorageResult::CacheFileAlreadyExists(
                            mapped_file,
                        ));
                    }
                }

                // fall through and load normally - we failed to load from a cache file but there are storages present
                Some(ScanAccountStorageResult::CacheFileNeedsToBeCreated((
                    file_name,
                    range_this_chunk,
                )))
            })
            .collect::<Vec<_>>();

        // Calculate the hits and misses of the hash data files cache.
        // This is outside of the parallel loop above so that we only need to
        // update each atomic stat value once.
        // There are approximately 173 items in the cache files list,
        // so should be very fast to iterate and compute.
        // (173 cache files == 432,000 slots / 2,500 slots-per-cache-file)
        let mut hits = 0;
        let mut misses = 0;
        for cache_file in &cache_files {
            match cache_file {
                ScanAccountStorageResult::CacheFileAlreadyExists(_) => hits += 1,
                ScanAccountStorageResult::CacheFileNeedsToBeCreated(_) => misses += 1,
            };
        }
        cache_hash_data
            .stats
            .hits
            .fetch_add(hits, Ordering::Relaxed);
        cache_hash_data
            .stats
            .misses
            .fetch_add(misses, Ordering::Relaxed);

        // deletes the old files that will not be used before creating new ones
        cache_hash_data.delete_old_cache_files();

        cache_files
            .into_par_iter()
            .map(|chunk| {
                match chunk {
                    ScanAccountStorageResult::CacheFileAlreadyExists(file) => Some(file),
                    ScanAccountStorageResult::CacheFileNeedsToBeCreated((
                        file_name,
                        range_this_chunk,
                    )) => {
                        let mut scanner = scanner.clone();
                        let mut init_accum = true;
                        // load from cache failed, so create the cache file for this chunk
                        for (slot, storage) in snapshot_storages.iter_range(&range_this_chunk) {
                            let ancient = slot < oldest_non_ancient_slot_for_identification;

                            let (_, scan_us) = measure_us!(if let Some(storage) = storage {
                                if init_accum {
                                    let range = bin_range.end - bin_range.start;
                                    scanner.init_accum(range);
                                    init_accum = false;
                                }
                                scanner.set_slot(slot, ancient);

                                Self::scan_single_account_storage(storage, &mut scanner);
                            });
                            if ancient {
                                stats
                                    .sum_ancient_scans_us
                                    .fetch_add(scan_us, Ordering::Relaxed);
                                stats.count_ancient_scans.fetch_add(1, Ordering::Relaxed);
                                stats
                                    .longest_ancient_scan_us
                                    .fetch_max(scan_us, Ordering::Relaxed);
                            }
                        }
                        (!init_accum)
                            .then(|| {
                                let r = scanner.scanning_complete();
                                assert!(!file_name.is_empty());
                                (!r.is_empty() && r.iter().any(|b| !b.is_empty())).then(|| {
                                    // error if we can't write this
                                    cache_hash_data.save(&file_name, &r).unwrap();
                                    cache_hash_data
                                        .get_file_reference_to_map_later(&file_name)
                                        .unwrap()
                                })
                            })
                            .flatten()
                    }
                }
            })
            .filter_map(|x| x)
            .collect()
    }

    /// iterate over a single storage, calling scanner on each item
    fn scan_single_account_storage<S>(storage: &AccountStorageEntry, scanner: &mut S)
    where
        S: AppendVecScan,
    {
        storage.accounts.scan_accounts(|account| {
            if scanner.filter(account.pubkey()) {
                scanner.found_account(&LoadedAccount::Stored(account))
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            accounts_db::{
                get_temp_accounts_paths,
                tests::{
                    append_single_account_with_default_hash, define_accounts_db_test,
                    get_storage_refs, sample_storage_with_entries,
                    sample_storages_and_account_in_slot, sample_storages_and_accounts,
                },
                MAX_ITEMS_PER_CHUNK,
            },
            accounts_file::{AccountsFile, AccountsFileProvider},
            append_vec::AppendVec,
            cache_hash_data::{CacheHashDataFile, DeletionPolicy as CacheHashDeletionPolicy},
        },
        solana_sdk::account::AccountSharedData,
        tempfile::TempDir,
        test_case::test_case,
    };

    impl AccountsDb {
        fn scan_snapshot_stores(
            &self,
            storage: &SortedStorages,
            stats: &mut crate::accounts_hash::HashStats,
            bins: usize,
            bin_range: &Range<usize>,
        ) -> Vec<CacheHashDataFile> {
            let temp_dir = TempDir::new().unwrap();
            let accounts_hash_cache_path = temp_dir.path().to_path_buf();
            self.scan_snapshot_stores_with_cache(
                &CacheHashData::new(accounts_hash_cache_path, CacheHashDeletionPolicy::AllUnused),
                storage,
                stats,
                bins,
                bin_range,
                &CalcAccountsHashConfig::default(),
            )
            .iter()
            .map(CacheHashDataFileReference::map)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        }
    }

    #[derive(Clone)]
    struct TestScan {
        calls: Arc<AtomicU64>,
        pubkey: Pubkey,
        slot_expected: Slot,
        accum: BinnedHashData,
        current_slot: Slot,
        value_to_use_for_lamports: u64,
    }

    impl AppendVecScan for TestScan {
        fn filter(&mut self, _pubkey: &Pubkey) -> bool {
            true
        }
        fn set_slot(&mut self, slot: Slot, _is_ancient: bool) {
            self.current_slot = slot;
        }
        fn init_accum(&mut self, _count: usize) {}
        fn found_account(&mut self, loaded_account: &LoadedAccount) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert_eq!(loaded_account.pubkey(), &self.pubkey);
            assert_eq!(self.slot_expected, self.current_slot);
            self.accum.push(vec![CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: self.value_to_use_for_lamports,
                pubkey: self.pubkey,
            }]);
        }
        fn scanning_complete(self) -> BinnedHashData {
            self.accum
        }
    }

    #[derive(Clone)]
    struct TestScanSimple {
        current_slot: Slot,
        slot_expected: Slot,
        calls: Arc<AtomicU64>,
        accum: BinnedHashData,
        pubkey1: Pubkey,
        pubkey2: Pubkey,
    }

    impl AppendVecScan for TestScanSimple {
        fn set_slot(&mut self, slot: Slot, _is_ancient: bool) {
            self.current_slot = slot;
        }
        fn filter(&mut self, _pubkey: &Pubkey) -> bool {
            true
        }
        fn init_accum(&mut self, _count: usize) {}
        fn found_account(&mut self, loaded_account: &LoadedAccount) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let first = loaded_account.pubkey() == &self.pubkey1;
            assert!(first || loaded_account.pubkey() == &self.pubkey2);
            assert_eq!(self.slot_expected, self.current_slot);
            if first {
                assert!(self.accum.is_empty());
            } else {
                assert_eq!(self.accum.len(), 1);
            }
            self.accum.push(vec![CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: loaded_account.lamports(),
                pubkey: Pubkey::default(),
            }]);
        }
        fn scanning_complete(self) -> BinnedHashData {
            self.accum
        }
    }

    #[test]
    #[should_panic(expected = "bin_range.start < bins")]
    fn test_accountsdb_scan_snapshot_stores_illegal_range_start() {
        let mut stats = HashStats::default();
        let bounds = Range { start: 2, end: 2 };
        let accounts_db = AccountsDb::new_single_for_tests();

        accounts_db.scan_snapshot_stores(&empty_storages(), &mut stats, 2, &bounds);
    }
    #[test]
    #[should_panic(expected = "bin_range.end <= bins")]
    fn test_accountsdb_scan_snapshot_stores_illegal_range_end() {
        let mut stats = HashStats::default();
        let bounds = Range { start: 1, end: 3 };

        let accounts_db = AccountsDb::new_single_for_tests();
        accounts_db.scan_snapshot_stores(&empty_storages(), &mut stats, 2, &bounds);
    }

    #[test]
    #[should_panic(expected = "bin_range.start < bin_range.end")]
    fn test_accountsdb_scan_snapshot_stores_illegal_range_inverse() {
        let mut stats = HashStats::default();
        let bounds = Range { start: 1, end: 0 };

        let accounts_db = AccountsDb::new_single_for_tests();
        accounts_db.scan_snapshot_stores(&empty_storages(), &mut stats, 2, &bounds);
    }

    #[test_case(AccountsFileProvider::AppendVec)]
    #[test_case(AccountsFileProvider::HotStorage)]
    fn test_accountsdb_scan_account_storage_no_bank(accounts_file_provider: AccountsFileProvider) {
        solana_logger::setup();

        let expected = 1;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_accountsdb_scan_account_storage_no_bank",
        );
        let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
        let slot_expected: Slot = 0;
        let size: usize = 123;
        let mut data = AccountStorageEntry::new(
            &paths[0],
            slot_expected,
            0,
            size as u64,
            accounts_file_provider,
        );
        let av = AccountsFile::AppendVec(AppendVec::new(&tf.path, true, 1024 * 1024));
        data.accounts = av;

        let storage = Arc::new(data);
        let pubkey = solana_sdk::pubkey::new_rand();
        let acc = AccountSharedData::new(1, 48, AccountSharedData::default().owner());
        let mark_alive = false;
        append_single_account_with_default_hash(&storage, &pubkey, &acc, mark_alive, None);

        let calls = Arc::new(AtomicU64::new(0));
        let temp_dir = TempDir::new().unwrap();
        let accounts_hash_cache_path = temp_dir.path().to_path_buf();
        let accounts_db = AccountsDb::new_single_for_tests();

        let test_scan = TestScan {
            calls: calls.clone(),
            pubkey,
            slot_expected,
            accum: Vec::default(),
            current_slot: 0,
            value_to_use_for_lamports: expected,
        };

        let result = accounts_db.scan_account_storage_no_bank(
            &CacheHashData::new(accounts_hash_cache_path, CacheHashDeletionPolicy::AllUnused),
            &CalcAccountsHashConfig::default(),
            &get_storage_refs(&[storage]),
            test_scan,
            &Range { start: 0, end: 1 },
            &mut HashStats::default(),
        );
        let result2 = result
            .iter()
            .map(|file| file.map().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_scan(
            result2,
            vec![vec![vec![CalculateHashIntermediate {
                hash: AccountHash(Hash::default()),
                lamports: expected,
                pubkey,
            }]]],
            1,
            0,
            1,
        );
    }

    #[test]
    fn test_accountsdb_scan_multiple_account_storage_no_bank_one_slot() {
        solana_logger::setup();

        let slot_expected: Slot = 0;
        let tf = crate::append_vec::test_utils::get_append_vec_path(
            "test_accountsdb_scan_account_storage_no_bank",
        );
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let mark_alive = false;
        let storage = sample_storage_with_entries(&tf, slot_expected, &pubkey1, mark_alive);
        let lamports = storage
            .accounts
            .get_account_shared_data(0)
            .unwrap()
            .lamports();
        let calls = Arc::new(AtomicU64::new(0));
        let mut scanner = TestScanSimple {
            current_slot: 0,
            slot_expected,
            pubkey1,
            pubkey2,
            accum: Vec::default(),
            calls: calls.clone(),
        };
        AccountsDb::scan_single_account_storage(&storage, &mut scanner);
        let accum = scanner.scanning_complete();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            accum
                .iter()
                .flatten()
                .map(|a| a.lamports)
                .collect::<Vec<_>>(),
            vec![lamports]
        );
    }

    define_accounts_db_test!(
        test_accountsdb_scan_account_storage_no_bank_one_slot,
        |db| {
            solana_logger::setup();
            let accounts_file_provider = db.accounts_file_provider;

            let expected = 1;
            let (_temp_dirs, paths) = get_temp_accounts_paths(1).unwrap();
            let slot_expected: Slot = 0;
            let data = AccountStorageEntry::new(
                &paths[0],
                slot_expected,
                0,
                1024 * 1024,
                accounts_file_provider,
            );
            let storage = Arc::new(data);
            let pubkey = solana_sdk::pubkey::new_rand();
            let acc = AccountSharedData::new(1, 48, AccountSharedData::default().owner());
            let mark_alive = false;
            append_single_account_with_default_hash(&storage, &pubkey, &acc, mark_alive, None);

            let calls = Arc::new(AtomicU64::new(0));

            let mut test_scan = TestScan {
                calls: calls.clone(),
                pubkey,
                slot_expected,
                accum: Vec::default(),
                current_slot: 0,
                value_to_use_for_lamports: expected,
            };

            AccountsDb::scan_single_account_storage(&storage, &mut test_scan);
            let accum = test_scan.scanning_complete();
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert_eq!(
                accum
                    .iter()
                    .flatten()
                    .map(|a| a.lamports)
                    .collect::<Vec<_>>(),
                vec![expected]
            );
        }
    );

    define_accounts_db_test!(
        test_accountsdb_scan_snapshot_stores_hash_not_stored,
        |accounts_db| {
            let (storages, raw_expected) = sample_storages_and_accounts(&accounts_db);
            storages.iter().for_each(|storage| {
                accounts_db.storage.remove(&storage.slot(), false);
            });

            // replace the sample storages, storing default hash values so that we rehash during scan
            let storages = storages
                .iter()
                .map(|storage| {
                    let slot = storage.slot();
                    let copied_storage = accounts_db.create_and_insert_store(slot, 10000, "test");
                    let mut all_accounts = Vec::default();
                    storage.accounts.scan_accounts(|acct| {
                        all_accounts.push((*acct.pubkey(), acct.to_account_shared_data()));
                    });
                    let accounts = all_accounts
                        .iter()
                        .map(|stored| (&stored.0, &stored.1))
                        .collect::<Vec<_>>();
                    let slice = &accounts[..];
                    let storable_accounts = (slot, slice);
                    copied_storage
                        .accounts
                        .append_accounts(&storable_accounts, 0);
                    copied_storage
                })
                .collect::<Vec<_>>();

            assert_test_scan(accounts_db, storages, raw_expected);
        }
    );

    #[test]
    fn test_accountsdb_scan_snapshot_stores_check_hash() {
        solana_logger::setup();
        let accounts_db = AccountsDb::new_single_for_tests();
        let (storages, _raw_expected) = sample_storages_and_accounts(&accounts_db);
        let max_slot = storages.iter().map(|storage| storage.slot()).max().unwrap();

        // replace the sample storages, storing default hash values so that we rehash during scan
        let storages = storages
            .iter()
            .map(|storage| {
                let slot = storage.slot() + max_slot;
                let copied_storage = accounts_db.create_and_insert_store(slot, 10000, "test");
                let mut all_accounts = Vec::default();
                storage.accounts.scan_accounts(|acct| {
                    all_accounts.push((*acct.pubkey(), acct.to_account_shared_data()));
                });
                let accounts = all_accounts
                    .iter()
                    .map(|stored| (&stored.0, &stored.1))
                    .collect::<Vec<_>>();
                let slice = &accounts[..];
                let storable_accounts = (slot, slice);
                copied_storage
                    .accounts
                    .append_accounts(&storable_accounts, 0);
                copied_storage
            })
            .collect::<Vec<_>>();

        let bins = 1;
        let mut stats = HashStats::default();

        accounts_db.scan_snapshot_stores(
            &get_storage_refs(&storages),
            &mut stats,
            bins,
            &Range {
                start: 0,
                end: bins,
            },
        );
    }

    define_accounts_db_test!(test_accountsdb_scan_snapshot_stores, |accounts_db| {
        let (storages, raw_expected) = sample_storages_and_accounts(&accounts_db);

        assert_test_scan(accounts_db, storages, raw_expected);
    });

    define_accounts_db_test!(
        test_accountsdb_scan_snapshot_stores_2nd_chunk,
        |accounts_db| {
            // enough stores to get to 2nd chunk
            let bins = 1;
            let slot = MAX_ITEMS_PER_CHUNK as Slot;
            let (storages, raw_expected) = sample_storages_and_account_in_slot(slot, &accounts_db);
            let storage_data = [(&storages[0], slot)];

            let sorted_storages =
                SortedStorages::new_debug(&storage_data[..], 0, MAX_ITEMS_PER_CHUNK as usize + 1);

            let mut stats = HashStats::default();
            let result = accounts_db.scan_snapshot_stores(
                &sorted_storages,
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: bins,
                },
            );

            assert_scan(result, vec![vec![raw_expected]], bins, 0, bins);
        }
    );

    define_accounts_db_test!(
        test_accountsdb_scan_snapshot_stores_binning,
        |accounts_db| {
            let mut stats = HashStats::default();
            let (storages, raw_expected) = sample_storages_and_accounts(&accounts_db);

            // just the first bin of 2
            let bins = 2;
            let half_bins = bins / 2;
            let result = accounts_db.scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 0,
                    end: half_bins,
                },
            );
            let mut expected = vec![Vec::new(); half_bins];
            expected[0].push(raw_expected[0]);
            expected[0].push(raw_expected[1]);
            assert_scan(result, vec![expected], bins, 0, half_bins);

            // just the second bin of 2
            let accounts_db = AccountsDb::new_single_for_tests();
            let result = accounts_db.scan_snapshot_stores(
                &get_storage_refs(&storages),
                &mut stats,
                bins,
                &Range {
                    start: 1,
                    end: bins,
                },
            );

            let mut expected = vec![Vec::new(); half_bins];
            let starting_bin_index = 0;
            expected[starting_bin_index].push(raw_expected[2]);
            expected[starting_bin_index].push(raw_expected[3]);
            assert_scan(result, vec![expected], bins, 1, bins - 1);

            // 1 bin at a time of 4
            let bins = 4;
            let accounts_db = AccountsDb::new_single_for_tests();

            for (bin, expected_item) in raw_expected.iter().enumerate().take(bins) {
                let result = accounts_db.scan_snapshot_stores(
                    &get_storage_refs(&storages),
                    &mut stats,
                    bins,
                    &Range {
                        start: bin,
                        end: bin + 1,
                    },
                );
                let mut expected = vec![Vec::new(); 1];
                expected[0].push(*expected_item);
                assert_scan(result, vec![expected], bins, bin, 1);
            }

            let bins = 256;
            let bin_locations = [0, 127, 128, 255];
            let range = 1;
            for bin in 0..bins {
                let accounts_db = AccountsDb::new_single_for_tests();
                let result = accounts_db.scan_snapshot_stores(
                    &get_storage_refs(&storages),
                    &mut stats,
                    bins,
                    &Range {
                        start: bin,
                        end: bin + range,
                    },
                );
                let mut expected = vec![];
                if let Some(index) = bin_locations.iter().position(|&r| r == bin) {
                    expected = vec![Vec::new(); range];
                    expected[0].push(raw_expected[index]);
                }
                let mut result2 = (0..range).map(|_| Vec::default()).collect::<Vec<_>>();
                if let Some(m) = result.first() {
                    m.load_all(&mut result2, bin, &PubkeyBinCalculator24::new(bins));
                } else {
                    result2 = vec![];
                }

                assert_eq!(result2, expected);
            }
        }
    );

    define_accounts_db_test!(
        test_accountsdb_scan_snapshot_stores_binning_2nd_chunk,
        |accounts_db| {
            // enough stores to get to 2nd chunk
            // range is for only 1 bin out of 256.
            let bins = 256;
            let slot = MAX_ITEMS_PER_CHUNK as Slot;
            let (storages, raw_expected) = sample_storages_and_account_in_slot(slot, &accounts_db);
            let storage_data = [(&storages[0], slot)];

            let sorted_storages =
                SortedStorages::new_debug(&storage_data[..], 0, MAX_ITEMS_PER_CHUNK as usize + 1);

            let mut stats = HashStats::default();
            let range = 1;
            let start = 127;
            let result = accounts_db.scan_snapshot_stores(
                &sorted_storages,
                &mut stats,
                bins,
                &Range {
                    start,
                    end: start + range,
                },
            );
            assert_eq!(result.len(), 1); // 2 chunks, but 1 is empty so not included
            let mut expected = vec![Vec::new(); range];
            expected[0].push(raw_expected[1]);
            let mut result2 = (0..range).map(|_| Vec::default()).collect::<Vec<_>>();
            result[0].load_all(&mut result2, 0, &PubkeyBinCalculator24::new(range));
            assert_eq!(result2.len(), 1);
            assert_eq!(result2, expected);
        }
    );

    fn assert_test_scan(
        accounts_db: AccountsDb,
        storages: Vec<Arc<AccountStorageEntry>>,
        raw_expected: Vec<CalculateHashIntermediate>,
    ) {
        let bins = 1;
        let mut stats = HashStats::default();

        let result = accounts_db.scan_snapshot_stores(
            &get_storage_refs(&storages),
            &mut stats,
            bins,
            &Range {
                start: 0,
                end: bins,
            },
        );
        assert_scan(result, vec![vec![raw_expected.clone()]], bins, 0, bins);

        let bins = 2;
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db.scan_snapshot_stores(
            &get_storage_refs(&storages),
            &mut stats,
            bins,
            &Range {
                start: 0,
                end: bins,
            },
        );
        let mut expected = vec![Vec::new(); bins];
        expected[0].push(raw_expected[0]);
        expected[0].push(raw_expected[1]);
        expected[bins - 1].push(raw_expected[2]);
        expected[bins - 1].push(raw_expected[3]);
        assert_scan(result, vec![expected], bins, 0, bins);

        let bins = 4;
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db.scan_snapshot_stores(
            &get_storage_refs(&storages),
            &mut stats,
            bins,
            &Range {
                start: 0,
                end: bins,
            },
        );
        let mut expected = vec![Vec::new(); bins];
        expected[0].push(raw_expected[0]);
        expected[1].push(raw_expected[1]);
        expected[2].push(raw_expected[2]);
        expected[bins - 1].push(raw_expected[3]);
        assert_scan(result, vec![expected], bins, 0, bins);

        let bins = 256;
        let accounts_db = AccountsDb::new_single_for_tests();
        let result = accounts_db.scan_snapshot_stores(
            &get_storage_refs(&storages),
            &mut stats,
            bins,
            &Range {
                start: 0,
                end: bins,
            },
        );
        let mut expected = vec![Vec::new(); bins];
        expected[0].push(raw_expected[0]);
        expected[127].push(raw_expected[1]);
        expected[128].push(raw_expected[2]);
        expected[bins - 1].push(*raw_expected.last().unwrap());
        assert_scan(result, vec![expected], bins, 0, bins);
    }

    /// helper to compare expected binned data with scan result in cache files
    /// result: return from scanning
    /// expected: binned data expected
    /// bins: # bins total to divide pubkeys into
    /// start_bin_index: bin # that was the minimum # we were scanning for 0<=start_bin_index<bins
    /// bin_range: end_exclusive-start_bin_index passed to scan
    fn assert_scan(
        result: Vec<CacheHashDataFile>,
        expected: Vec<BinnedHashData>,
        bins: usize,
        start_bin_index: usize,
        bin_range: usize,
    ) {
        assert_eq!(expected.len(), result.len());

        for cache_file in &result {
            let mut result2 = (0..bin_range).map(|_| Vec::default()).collect::<Vec<_>>();
            cache_file.load_all(
                &mut result2,
                start_bin_index,
                &PubkeyBinCalculator24::new(bins),
            );
            assert_eq!(
                convert_to_slice(&[result2]),
                expected,
                "bins: {bins}, start_bin_index: {start_bin_index}"
            );
        }
    }

    fn empty_storages<'a>() -> SortedStorages<'a> {
        SortedStorages::new(&[])
    }

    fn convert_to_slice(
        input: &[Vec<Vec<CalculateHashIntermediate>>],
    ) -> Vec<Vec<&[CalculateHashIntermediate]>> {
        input
            .iter()
            .map(|v| v.iter().map(|v| &v[..]).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    }
}
