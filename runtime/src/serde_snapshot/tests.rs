#[cfg(test)]
mod serde_snapshot_tests {
    use {
        crate::{
            serde_snapshot::{
                newer, reconstruct_accountsdb_from_fields, SerdeStyle, SerializableAccountsDb,
                SnapshotAccountsDbFields, TypeContext,
            },
            snapshot_utils::{get_storages_to_serialize, StorageAndNextAppendVecId},
        },
        bincode::{serialize_into, Error},
        log::info,
        rand::{thread_rng, Rng},
        solana_accounts_db::{
            account_storage::{AccountStorageMap, AccountStorageReference},
            accounts::Accounts,
            accounts_db::{
                get_temp_accounts_paths, test_utils::create_test_accounts, AccountShrinkThreshold,
                AccountStorageEntry, AccountsDb, AtomicAppendVecId,
                VerifyAccountsHashAndLamportsConfig,
            },
            accounts_file::{AccountsFile, AccountsFileError},
            accounts_hash::AccountsHash,
            accounts_index::AccountSecondaryIndexes,
            ancestors::Ancestors,
            rent_collector::RentCollector,
        },
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            clock::Slot,
            epoch_schedule::EpochSchedule,
            genesis_config::{ClusterType, GenesisConfig},
            hash::Hash,
            pubkey::Pubkey,
        },
        std::{
            io::{BufReader, Cursor, Read, Write},
            ops::RangeFull,
            path::{Path, PathBuf},
            sync::{atomic::Ordering, Arc},
        },
        tempfile::TempDir,
    };

    fn linear_ancestors(end_slot: u64) -> Ancestors {
        let mut ancestors: Ancestors = vec![(0, 0)].into_iter().collect();
        for i in 1..end_slot {
            ancestors.insert(i, (i - 1) as usize);
        }
        ancestors
    }

    fn context_accountsdb_from_stream<'a, C, R>(
        stream: &mut BufReader<R>,
        account_paths: &[PathBuf],
        storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    ) -> Result<AccountsDb, Error>
    where
        C: TypeContext<'a>,
        R: Read,
    {
        // read and deserialise the accounts database directly from the stream
        let accounts_db_fields = C::deserialize_accounts_db_fields(stream)?;
        let snapshot_accounts_db_fields = SnapshotAccountsDbFields {
            full_snapshot_accounts_db_fields: accounts_db_fields,
            incremental_snapshot_accounts_db_fields: None,
        };
        reconstruct_accountsdb_from_fields(
            snapshot_accounts_db_fields,
            account_paths,
            storage_and_next_append_vec_id,
            &GenesisConfig {
                cluster_type: ClusterType::Development,
                ..GenesisConfig::default()
            },
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            Some(solana_accounts_db::accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
            None,
            (u64::default(), None),
            None,
        )
        .map(|(accounts_db, _)| accounts_db)
    }

    fn accountsdb_from_stream<R>(
        serde_style: SerdeStyle,
        stream: &mut BufReader<R>,
        account_paths: &[PathBuf],
        storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    ) -> Result<AccountsDb, Error>
    where
        R: Read,
    {
        match serde_style {
            SerdeStyle::Newer => context_accountsdb_from_stream::<newer::Context, R>(
                stream,
                account_paths,
                storage_and_next_append_vec_id,
            ),
        }
    }

    fn accountsdb_to_stream<W>(
        serde_style: SerdeStyle,
        stream: &mut W,
        accounts_db: &AccountsDb,
        slot: Slot,
        account_storage_entries: &[Vec<Arc<AccountStorageEntry>>],
    ) -> Result<(), Error>
    where
        W: Write,
    {
        match serde_style {
            SerdeStyle::Newer => serialize_into(
                stream,
                &SerializableAccountsDb::<newer::Context> {
                    accounts_db,
                    slot,
                    account_storage_entries,
                    phantom: std::marker::PhantomData,
                },
            ),
        }
    }

    /// Simulates the unpacking & storage reconstruction done during snapshot unpacking
    fn copy_append_vecs<P: AsRef<Path>>(
        accounts_db: &AccountsDb,
        output_dir: P,
    ) -> Result<StorageAndNextAppendVecId, AccountsFileError> {
        let storage_entries = accounts_db.get_snapshot_storages(RangeFull).0;
        let storage: AccountStorageMap = AccountStorageMap::with_capacity(storage_entries.len());
        let mut next_append_vec_id = 0;
        for storage_entry in storage_entries.into_iter() {
            // Copy file to new directory
            let storage_path = storage_entry.get_path();
            let file_name =
                AccountsFile::file_name(storage_entry.slot(), storage_entry.append_vec_id());
            let output_path = output_dir.as_ref().join(file_name);
            std::fs::copy(storage_path, &output_path)?;

            // Read new file into append-vec and build new entry
            let (accounts_file, num_accounts) =
                AccountsFile::new_from_file(output_path, storage_entry.accounts.len())?;
            let new_storage_entry = AccountStorageEntry::new_existing(
                storage_entry.slot(),
                storage_entry.append_vec_id(),
                accounts_file,
                num_accounts,
            );
            next_append_vec_id = next_append_vec_id.max(new_storage_entry.append_vec_id());
            storage.insert(
                new_storage_entry.slot(),
                AccountStorageReference {
                    id: new_storage_entry.append_vec_id(),
                    storage: Arc::new(new_storage_entry),
                },
            );
        }

        Ok(StorageAndNextAppendVecId {
            storage,
            next_append_vec_id: AtomicAppendVecId::new(next_append_vec_id + 1),
        })
    }

    fn reconstruct_accounts_db_via_serialization(accounts: &AccountsDb, slot: Slot) -> AccountsDb {
        let mut writer = Cursor::new(vec![]);
        let snapshot_storages = accounts.get_snapshot_storages(..=slot).0;
        accountsdb_to_stream(
            SerdeStyle::Newer,
            &mut writer,
            accounts,
            slot,
            &get_storages_to_serialize(&snapshot_storages),
        )
        .unwrap();

        let buf = writer.into_inner();
        let mut reader = BufReader::new(&buf[..]);
        let copied_accounts = TempDir::new().unwrap();

        // Simulate obtaining a copy of the AppendVecs from a tarball
        let storage_and_next_append_vec_id =
            copy_append_vecs(accounts, copied_accounts.path()).unwrap();
        let mut accounts_db = accountsdb_from_stream(
            SerdeStyle::Newer,
            &mut reader,
            &[],
            storage_and_next_append_vec_id,
        )
        .unwrap();

        // The append vecs will be used from `copied_accounts` directly by the new AccountsDb so keep
        // its TempDir alive
        accounts_db
            .temp_paths
            .as_mut()
            .unwrap()
            .push(copied_accounts);

        accounts_db
    }

    fn check_accounts_local(accounts: &Accounts, pubkeys: &[Pubkey], num: usize) {
        for _ in 1..num {
            let idx = thread_rng().gen_range(0..num - 1);
            let ancestors = vec![(0, 0)].into_iter().collect();
            let account = accounts.load_without_fixed_root(&ancestors, &pubkeys[idx]);
            let account1 = Some((
                AccountSharedData::new((idx + 1) as u64, 0, AccountSharedData::default().owner()),
                0,
            ));
            assert_eq!(account, account1);
        }
    }

    fn test_accounts_serialize_style(serde_style: SerdeStyle) {
        solana_logger::setup();
        let (_accounts_dir, paths) = get_temp_accounts_paths(4).unwrap();
        let accounts = Accounts::new_with_config_for_tests(
            paths,
            &ClusterType::Development,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        );

        let slot = 0;
        let mut pubkeys: Vec<Pubkey> = vec![];
        create_test_accounts(&accounts, &mut pubkeys, 100, slot);
        check_accounts_local(&accounts, &pubkeys, 100);
        accounts.add_root(slot);
        let accounts_delta_hash = accounts.accounts_db.calculate_accounts_delta_hash(slot);
        let accounts_hash = AccountsHash(Hash::new_unique());
        accounts
            .accounts_db
            .set_accounts_hash(slot, (accounts_hash, u64::default()));

        let mut writer = Cursor::new(vec![]);
        accountsdb_to_stream(
            serde_style,
            &mut writer,
            &accounts.accounts_db,
            slot,
            &get_storages_to_serialize(&accounts.accounts_db.get_snapshot_storages(..=slot).0),
        )
        .unwrap();

        let copied_accounts = TempDir::new().unwrap();

        // Simulate obtaining a copy of the AppendVecs from a tarball
        let storage_and_next_append_vec_id =
            copy_append_vecs(&accounts.accounts_db, copied_accounts.path()).unwrap();

        let buf = writer.into_inner();
        let mut reader = BufReader::new(&buf[..]);
        let (_accounts_dir, daccounts_paths) = get_temp_accounts_paths(2).unwrap();
        let daccounts = Accounts::new_empty(
            accountsdb_from_stream(
                serde_style,
                &mut reader,
                &daccounts_paths,
                storage_and_next_append_vec_id,
            )
            .unwrap(),
        );
        check_accounts_local(&daccounts, &pubkeys, 100);
        let daccounts_delta_hash = daccounts.accounts_db.calculate_accounts_delta_hash(slot);
        assert_eq!(accounts_delta_hash, daccounts_delta_hash);
        let daccounts_hash = daccounts.accounts_db.get_accounts_hash(slot).unwrap().0;
        assert_eq!(accounts_hash, daccounts_hash);
    }

    #[test]
    fn test_accounts_serialize_newer() {
        test_accounts_serialize_style(SerdeStyle::Newer)
    }

    #[test]
    fn test_remove_unrooted_slot_snapshot() {
        solana_logger::setup();
        let unrooted_slot = 9;
        let unrooted_bank_id = 9;
        let db = AccountsDb::new(Vec::new(), &ClusterType::Development);
        let key = solana_sdk::pubkey::new_rand();
        let account0 = AccountSharedData::new(1, 0, &key);
        db.store_for_tests(unrooted_slot, &[(&key, &account0)]);

        // Purge the slot
        db.remove_unrooted_slots(&[(unrooted_slot, unrooted_bank_id)]);

        // Add a new root
        let key2 = solana_sdk::pubkey::new_rand();
        let new_root = unrooted_slot + 1;
        db.store_for_tests(new_root, &[(&key2, &account0)]);
        db.add_root_and_flush_write_cache(new_root);

        db.calculate_accounts_delta_hash(new_root);
        db.update_accounts_hash_for_tests(new_root, &linear_ancestors(new_root), false, false);

        // Simulate reconstruction from snapshot
        let db = reconstruct_accounts_db_via_serialization(&db, new_root);

        // Check root account exists
        db.assert_load_account(new_root, key2, 1);

        // Check purged account stays gone
        let unrooted_slot_ancestors = vec![(unrooted_slot, 1)].into_iter().collect();
        assert!(db
            .load_without_fixed_root(&unrooted_slot_ancestors, &key)
            .is_none());
    }

    #[test]
    fn test_accounts_db_serialize1() {
        for pass in 0..2 {
            solana_logger::setup();
            let accounts = AccountsDb::new_single_for_tests();
            let mut pubkeys: Vec<Pubkey> = vec![];

            // Create 100 accounts in slot 0
            accounts.create_account(&mut pubkeys, 0, 100, 0, 0);
            if pass == 0 {
                accounts.add_root_and_flush_write_cache(0);
                accounts.check_storage(0, 100);
                accounts.clean_accounts_for_tests();
                accounts.check_accounts(&pubkeys, 0, 100, 1);
                // clean should have done nothing
                continue;
            }

            // do some updates to those accounts and re-check
            accounts.modify_accounts(&pubkeys, 0, 100, 2);
            accounts.add_root_and_flush_write_cache(0);
            accounts.check_storage(0, 100);
            accounts.check_accounts(&pubkeys, 0, 100, 2);
            accounts.calculate_accounts_delta_hash(0);

            let mut pubkeys1: Vec<Pubkey> = vec![];

            // CREATE SLOT 1
            let latest_slot = 1;

            // Modify the first 10 of the accounts from slot 0 in slot 1
            accounts.modify_accounts(&pubkeys, latest_slot, 10, 3);
            // Overwrite account 30 from slot 0 with lamports=0 into slot 1.
            // Slot 1 should now have 10 + 1 = 11 accounts
            let account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
            accounts.store_for_tests(latest_slot, &[(&pubkeys[30], &account)]);

            // Create 10 new accounts in slot 1, should now have 11 + 10 = 21
            // accounts
            accounts.create_account(&mut pubkeys1, latest_slot, 10, 0, 0);

            accounts.calculate_accounts_delta_hash(latest_slot);
            accounts.add_root_and_flush_write_cache(latest_slot);
            accounts.check_storage(1, 21);

            // CREATE SLOT 2
            let latest_slot = 2;
            let mut pubkeys2: Vec<Pubkey> = vec![];

            // Modify first 20 of the accounts from slot 0 in slot 2
            accounts.modify_accounts(&pubkeys, latest_slot, 20, 4);
            accounts.clean_accounts_for_tests();
            // Overwrite account 31 from slot 0 with lamports=0 into slot 2.
            // Slot 2 should now have 20 + 1 = 21 accounts
            let account = AccountSharedData::new(0, 0, AccountSharedData::default().owner());
            accounts.store_for_tests(latest_slot, &[(&pubkeys[31], &account)]);

            // Create 10 new accounts in slot 2. Slot 2 should now have
            // 21 + 10 = 31 accounts
            accounts.create_account(&mut pubkeys2, latest_slot, 10, 0, 0);

            accounts.calculate_accounts_delta_hash(latest_slot);
            accounts.add_root_and_flush_write_cache(latest_slot);
            accounts.check_storage(2, 31);

            let ancestors = linear_ancestors(latest_slot);
            accounts.update_accounts_hash_for_tests(latest_slot, &ancestors, false, false);

            accounts.clean_accounts_for_tests();
            // The first 20 accounts of slot 0 have been updated in slot 2, as well as
            // accounts 30 and  31 (overwritten with zero-lamport accounts in slot 1 and
            // slot 2 respectively), so only 78 accounts are left in slot 0's storage entries.
            accounts.check_storage(0, 78);
            // 10 of the 21 accounts have been modified in slot 2, so only 11
            // accounts left in slot 1.
            accounts.check_storage(1, 11);
            accounts.check_storage(2, 31);

            let daccounts = reconstruct_accounts_db_via_serialization(&accounts, latest_slot);

            assert_eq!(
                daccounts.write_version.load(Ordering::Acquire),
                accounts.write_version.load(Ordering::Acquire)
            );

            // Get the hashes for the latest slot, which should be the only hashes in the
            // map on the deserialized AccountsDb
            assert_eq!(daccounts.accounts_delta_hashes().lock().unwrap().len(), 1);
            assert_eq!(daccounts.accounts_hashes().lock().unwrap().len(), 1);
            assert_eq!(
                daccounts.get_accounts_delta_hash(latest_slot).unwrap(),
                accounts.get_accounts_delta_hash(latest_slot).unwrap(),
            );
            assert_eq!(
                daccounts.get_accounts_hash(latest_slot).unwrap().0,
                accounts.get_accounts_hash(latest_slot).unwrap().0,
            );

            daccounts.print_count_and_status("daccounts");

            // Don't check the first 35 accounts which have not been modified on slot 0
            daccounts.check_accounts(&pubkeys[35..], 0, 65, 37);
            daccounts.check_accounts(&pubkeys1, 1, 10, 1);
            daccounts.check_storage(0, 100);
            daccounts.check_storage(1, 21);
            daccounts.check_storage(2, 31);

            assert_eq!(
                daccounts.update_accounts_hash_for_tests(latest_slot, &ancestors, false, false,),
                accounts.update_accounts_hash_for_tests(latest_slot, &ancestors, false, false,)
            );
        }
    }

    #[test]
    fn test_accounts_db_serialize_zero_and_free() {
        solana_logger::setup();

        let some_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);
        let pubkey = solana_sdk::pubkey::new_rand();
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let account2 = AccountSharedData::new(some_lamport + 1, no_data, &owner);
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let filler_account = AccountSharedData::new(some_lamport, no_data, &owner);
        let filler_account_pubkey = solana_sdk::pubkey::new_rand();

        let accounts = AccountsDb::new_single_for_tests();

        let mut current_slot = 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.add_root(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account2)]);

        // Store the account a few times.
        // use to be: store enough accounts such that an additional store for slot 2 is created.
        // but we use the write cache now
        for _ in 0..3 {
            accounts.store_for_tests(current_slot, &[(&filler_account_pubkey, &filler_account)]);
        }
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.assert_load_account(current_slot, pubkey, zero_lamport);

        accounts.print_accounts_stats("accounts");

        accounts.clean_accounts_for_tests();

        accounts.print_accounts_stats("accounts_post_purge");

        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.update_accounts_hash_for_tests(
            current_slot,
            &linear_ancestors(current_slot),
            false,
            false,
        );
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);

        accounts.print_accounts_stats("reconstructed");

        accounts.assert_load_account(current_slot, pubkey, zero_lamport);
    }

    fn with_chained_zero_lamport_accounts<F>(f: F)
    where
        F: Fn(AccountsDb, Slot) -> AccountsDb,
    {
        let some_lamport = 223;
        let zero_lamport = 0;
        let dummy_lamport = 999;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(some_lamport, no_data, &owner);
        let account2 = AccountSharedData::new(some_lamport + 100_001, no_data, &owner);
        let account3 = AccountSharedData::new(some_lamport + 100_002, no_data, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let pubkey = solana_sdk::pubkey::new_rand();
        let purged_pubkey1 = solana_sdk::pubkey::new_rand();
        let purged_pubkey2 = solana_sdk::pubkey::new_rand();

        let dummy_account = AccountSharedData::new(dummy_lamport, no_data, &owner);
        let dummy_pubkey = Pubkey::default();

        let accounts = AccountsDb::new_single_for_tests();

        let mut current_slot = 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_accounts_stats("pre_f");
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.update_accounts_hash_for_tests(4, &Ancestors::default(), false, false);

        let accounts = f(accounts, current_slot);

        accounts.print_accounts_stats("post_f");

        accounts.assert_load_account(current_slot, pubkey, some_lamport);
        accounts.assert_load_account(current_slot, purged_pubkey1, 0);
        accounts.assert_load_account(current_slot, purged_pubkey2, 0);
        accounts.assert_load_account(current_slot, dummy_pubkey, dummy_lamport);

        let ancestors = Ancestors::default();
        let epoch_schedule = EpochSchedule::default();
        let rent_collector = RentCollector::default();
        let config = VerifyAccountsHashAndLamportsConfig::new_for_test(
            &ancestors,
            &epoch_schedule,
            &rent_collector,
        );

        accounts
            .verify_accounts_hash_and_lamports(4, 1222, None, config)
            .unwrap();
    }

    #[test]
    fn test_accounts_purge_chained_purge_before_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            accounts.clean_accounts_for_tests();
            reconstruct_accounts_db_via_serialization(&accounts, current_slot)
        });
    }

    #[test]
    fn test_accounts_purge_chained_purge_after_snapshot_restore() {
        solana_logger::setup();
        with_chained_zero_lamport_accounts(|accounts, current_slot| {
            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts.print_accounts_stats("after_reconstruct");
            accounts.clean_accounts_for_tests();
            reconstruct_accounts_db_via_serialization(&accounts, current_slot)
        });
    }

    #[test]
    fn test_accounts_purge_long_chained_after_snapshot_restore() {
        solana_logger::setup();
        let old_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(old_lamport, no_data, &owner);
        let account2 = AccountSharedData::new(old_lamport + 100_001, no_data, &owner);
        let account3 = AccountSharedData::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = AccountSharedData::new(99_999_999, no_data, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let pubkey = solana_sdk::pubkey::new_rand();
        let dummy_pubkey = solana_sdk::pubkey::new_rand();
        let purged_pubkey1 = solana_sdk::pubkey::new_rand();
        let purged_pubkey2 = solana_sdk::pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDb::new_single_for_tests();

        // create intermediate updates to purged_pubkey1 so that
        // generate_index must add slots as root last at once
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey, &account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey1, &zero_lamport_account)]);
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &account3)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&purged_pubkey2, &zero_lamport_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);

        accounts.print_count_and_status("before reconstruct");
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.update_accounts_hash_for_tests(
            current_slot,
            &linear_ancestors(current_slot),
            false,
            false,
        );
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.print_count_and_status("before purge zero");
        accounts.clean_accounts_for_tests();
        accounts.print_count_and_status("after purge zero");

        accounts.assert_load_account(current_slot, pubkey, old_lamport);
        accounts.assert_load_account(current_slot, purged_pubkey1, 0);
        accounts.assert_load_account(current_slot, purged_pubkey2, 0);
    }

    #[test]
    fn test_accounts_clean_after_snapshot_restore_then_old_revives() {
        solana_logger::setup();
        let old_lamport = 223;
        let zero_lamport = 0;
        let no_data = 0;
        let dummy_lamport = 999_999;
        let owner = *AccountSharedData::default().owner();

        let account = AccountSharedData::new(old_lamport, no_data, &owner);
        let account2 = AccountSharedData::new(old_lamport + 100_001, no_data, &owner);
        let account3 = AccountSharedData::new(old_lamport + 100_002, no_data, &owner);
        let dummy_account = AccountSharedData::new(dummy_lamport, no_data, &owner);
        let zero_lamport_account = AccountSharedData::new(zero_lamport, no_data, &owner);

        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let dummy_pubkey = solana_sdk::pubkey::new_rand();

        let mut current_slot = 0;
        let accounts = AccountsDb::new_single_for_tests();

        // A: Initialize AccountsDb with pubkey1 and pubkey2
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account)]);
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // B: Test multiple updates to pubkey1 in a single slot/storage
        current_slot += 1;
        assert_eq!(0, accounts.alive_account_count_in_slot(current_slot));
        accounts.add_root_and_flush_write_cache(current_slot - 1);
        assert_eq!(1, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account2)]);
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account2)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        assert_eq!(1, accounts.alive_account_count_in_slot(current_slot));
        // Stores to same pubkey, same slot only count once towards the
        // ref count
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.calculate_accounts_delta_hash(current_slot);

        // C: Yet more update to trigger lazy clean of step A
        current_slot += 1;
        assert_eq!(2, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &account3)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root_and_flush_write_cache(current_slot);

        // D: Make pubkey1 0-lamport; also triggers clean of step B
        current_slot += 1;
        assert_eq!(3, accounts.ref_count_for_pubkey(&pubkey1));
        accounts.store_for_tests(current_slot, &[(&pubkey1, &zero_lamport_account)]);
        accounts.add_root_and_flush_write_cache(current_slot);
        // had to be a root to flush, but clean won't work as this test expects if it is a root
        // so, remove the root from alive_roots, then restore it after clean
        accounts
            .accounts_index
            .roots_tracker
            .write()
            .unwrap()
            .alive_roots
            .remove(&current_slot);
        accounts.clean_accounts_for_tests();
        accounts
            .accounts_index
            .roots_tracker
            .write()
            .unwrap()
            .alive_roots
            .insert(current_slot);

        assert_eq!(
            // Removed one reference from the dead slot (reference only counted once
            // even though there were two stores to the pubkey in that slot)
            3, /* == 3 - 1 + 1 */
            accounts.ref_count_for_pubkey(&pubkey1)
        );
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // E: Avoid missing bank hash error
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&dummy_pubkey, &dummy_account)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        accounts.assert_load_account(current_slot, pubkey1, zero_lamport);
        accounts.assert_load_account(current_slot, pubkey2, old_lamport);
        accounts.assert_load_account(current_slot, dummy_pubkey, dummy_lamport);

        // At this point, there is no index entries for A and B
        // If step C and step D should be purged, snapshot restore would cause
        // pubkey1 to be revived as the state of step A.
        // So, prevent that from happening by introducing refcount
        ((current_slot - 1)..=current_slot).for_each(|slot| accounts.flush_root_write_cache(slot));
        accounts.clean_accounts_for_tests();
        accounts.update_accounts_hash_for_tests(
            current_slot,
            &linear_ancestors(current_slot),
            false,
            false,
        );
        let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
        accounts.clean_accounts_for_tests();

        info!("pubkey: {}", pubkey1);
        accounts.print_accounts_stats("pre_clean");
        accounts.assert_load_account(current_slot, pubkey1, zero_lamport);
        accounts.assert_load_account(current_slot, pubkey2, old_lamport);
        accounts.assert_load_account(current_slot, dummy_pubkey, dummy_lamport);

        // F: Finally, make Step A cleanable
        current_slot += 1;
        accounts.store_for_tests(current_slot, &[(&pubkey2, &account)]);
        accounts.calculate_accounts_delta_hash(current_slot);
        accounts.add_root(current_slot);

        // Do clean
        accounts.flush_root_write_cache(current_slot);
        accounts.clean_accounts_for_tests();

        // 2nd clean needed to clean-up pubkey1
        accounts.clean_accounts_for_tests();

        // Ensure pubkey2 is cleaned from the index finally
        accounts.assert_not_load_account(current_slot, pubkey1);
        accounts.assert_load_account(current_slot, pubkey2, old_lamport);
        accounts.assert_load_account(current_slot, dummy_pubkey, dummy_lamport);
    }

    #[test]
    fn test_shrink_stale_slots_processed() {
        solana_logger::setup();

        for startup in &[false, true] {
            let accounts = AccountsDb::new_single_for_tests();

            let pubkey_count = 100;
            let pubkeys: Vec<_> = (0..pubkey_count)
                .map(|_| solana_sdk::pubkey::new_rand())
                .collect();

            let some_lamport = 223;
            let no_data = 0;
            let owner = *AccountSharedData::default().owner();

            let account = AccountSharedData::new(some_lamport, no_data, &owner);

            let mut current_slot = 0;

            current_slot += 1;
            for pubkey in &pubkeys {
                accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
            }
            let shrink_slot = current_slot;
            accounts.calculate_accounts_delta_hash(current_slot);
            accounts.add_root_and_flush_write_cache(current_slot);

            current_slot += 1;
            let pubkey_count_after_shrink = 10;
            let updated_pubkeys = &pubkeys[0..pubkey_count - pubkey_count_after_shrink];

            for pubkey in updated_pubkeys {
                accounts.store_for_tests(current_slot, &[(pubkey, &account)]);
            }
            accounts.calculate_accounts_delta_hash(current_slot);
            accounts.add_root_and_flush_write_cache(current_slot);

            accounts.clean_accounts_for_tests();

            assert_eq!(
                pubkey_count,
                accounts.all_account_count_in_append_vec(shrink_slot)
            );
            accounts.shrink_all_slots(*startup, None, &EpochSchedule::default());
            assert_eq!(
                pubkey_count_after_shrink,
                accounts.all_account_count_in_append_vec(shrink_slot)
            );

            let no_ancestors = Ancestors::default();

            let epoch_schedule = EpochSchedule::default();
            let rent_collector = RentCollector::default();
            let config = VerifyAccountsHashAndLamportsConfig::new_for_test(
                &no_ancestors,
                &epoch_schedule,
                &rent_collector,
            );

            accounts.update_accounts_hash_for_tests(current_slot, &no_ancestors, false, false);
            accounts
                .verify_accounts_hash_and_lamports(current_slot, 22300, None, config.clone())
                .unwrap();

            let accounts = reconstruct_accounts_db_via_serialization(&accounts, current_slot);
            accounts
                .verify_accounts_hash_and_lamports(current_slot, 22300, None, config)
                .unwrap();

            // repeating should be no-op
            accounts.shrink_all_slots(*startup, None, &epoch_schedule);
            assert_eq!(
                pubkey_count_after_shrink,
                accounts.all_account_count_in_append_vec(shrink_slot)
            );
        }
    }
}
