#[cfg(test)]
mod tests {
    use {
        crate::{
            bank::{
                epoch_accounts_hash_utils, test_utils as bank_test_utils, Bank, BankTestConfig,
                EpochRewardStatus, StartBlockHeightAndRewards,
            },
            genesis_utils::{activate_all_features, activate_feature},
            runtime_config::RuntimeConfig,
            serde_snapshot::{
                reserialize_bank_with_new_accounts_hash, BankIncrementalSnapshotPersistence,
                SerdeAccountsHash, SerdeIncrementalAccountsHash, SerdeStyle, SnapshotStreams,
            },
            snapshot_bank_utils,
            snapshot_utils::{
                self, create_tmp_accounts_dir_for_tests, get_storages_to_serialize, ArchiveFormat,
                StorageAndNextAppendVecId, BANK_SNAPSHOT_PRE_FILENAME_EXTENSION,
            },
            status_cache::StatusCache,
        },
        assert_matches::assert_matches,
        solana_accounts_db::{
            account_storage::{AccountStorageMap, AccountStorageReference},
            accounts_db::{
                get_temp_accounts_paths, AccountShrinkThreshold, AccountStorageEntry, AccountsDb,
                AtomicAppendVecId,
            },
            accounts_file::{AccountsFile, AccountsFileError},
            accounts_hash::{AccountsDeltaHash, AccountsHash},
            accounts_index::AccountSecondaryIndexes,
            epoch_accounts_hash::EpochAccountsHash,
            stake_rewards::StakeReward,
        },
        solana_sdk::{
            epoch_schedule::EpochSchedule,
            feature_set,
            genesis_config::create_genesis_config,
            hash::Hash,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
        },
        std::{
            io::{Cursor, Read, Write},
            num::NonZeroUsize,
            ops::RangeFull,
            path::Path,
            sync::{Arc, RwLock},
        },
        tempfile::TempDir,
    };

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

    fn test_bank_serialize_style(
        serde_style: SerdeStyle,
        reserialize_accounts_hash: bool,
        update_accounts_hash: bool,
        incremental_snapshot_persistence: bool,
        initial_epoch_accounts_hash: bool,
    ) {
        solana_logger::setup();
        let (mut genesis_config, _) = create_genesis_config(500);
        activate_feature(&mut genesis_config, feature_set::epoch_accounts_hash::id());
        genesis_config.epoch_schedule = EpochSchedule::custom(400, 400, false);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        let eah_start_slot = epoch_accounts_hash_utils::calculation_start(&bank0);
        let bank1 = Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 1);
        bank0.squash();

        // Create an account on a non-root fork
        let key1 = Keypair::new();
        bank_test_utils::deposit(&bank1, &key1.pubkey(), 5).unwrap();

        // If setting an initial EAH, then the bank being snapshotted must be in the EAH calculation
        // window.  Otherwise `bank_to_stream()` below will *not* include the EAH in the bank snapshot,
        // and the later-deserialized bank's EAH will not match the expected EAH.
        let bank2_slot = if initial_epoch_accounts_hash {
            eah_start_slot
        } else {
            0
        } + 2;
        let bank2 = Bank::new_from_parent(bank0, &Pubkey::default(), bank2_slot);

        // Test new account
        let key2 = Keypair::new();
        bank_test_utils::deposit(&bank2, &key2.pubkey(), 10).unwrap();
        assert_eq!(bank2.get_balance(&key2.pubkey()), 10);

        let key3 = Keypair::new();
        bank_test_utils::deposit(&bank2, &key3.pubkey(), 0).unwrap();

        bank2.freeze();
        bank2.squash();
        bank2.force_flush_accounts_cache();
        bank2.accounts().accounts_db.set_accounts_hash(
            bank2.slot(),
            (AccountsHash(Hash::new(&[0; 32])), u64::default()),
        );

        let snapshot_storages = bank2.get_snapshot_storages(None);
        let mut buf = vec![];
        let mut writer = Cursor::new(&mut buf);

        let mut expected_epoch_accounts_hash = None;

        if initial_epoch_accounts_hash {
            expected_epoch_accounts_hash = Some(Hash::new(&[7; 32]));
            bank2
                .rc
                .accounts
                .accounts_db
                .epoch_accounts_hash_manager
                .set_valid(
                    EpochAccountsHash::new(expected_epoch_accounts_hash.unwrap()),
                    eah_start_slot,
                );
        }

        crate::serde_snapshot::bank_to_stream(
            serde_style,
            &mut std::io::BufWriter::new(&mut writer),
            &bank2,
            &get_storages_to_serialize(&snapshot_storages),
        )
        .unwrap();

        if update_accounts_hash {
            bank2.accounts().accounts_db.set_accounts_hash(
                bank2.slot(),
                (AccountsHash(Hash::new(&[1; 32])), u64::default()),
            );
        }
        let accounts_hash = bank2.get_accounts_hash().unwrap();

        let slot = bank2.slot();
        let incremental =
            incremental_snapshot_persistence.then(|| BankIncrementalSnapshotPersistence {
                full_slot: slot - 1,
                full_hash: SerdeAccountsHash(Hash::new(&[1; 32])),
                full_capitalization: 31,
                incremental_hash: SerdeIncrementalAccountsHash(Hash::new(&[2; 32])),
                incremental_capitalization: 32,
            });

        if reserialize_accounts_hash || incremental_snapshot_persistence {
            let temp_dir = TempDir::new().unwrap();
            let slot_dir = snapshot_utils::get_bank_snapshot_dir(&temp_dir, slot);
            let post_path = slot_dir.join(slot.to_string());
            let pre_path = post_path.with_extension(BANK_SNAPSHOT_PRE_FILENAME_EXTENSION);
            std::fs::create_dir(&slot_dir).unwrap();
            {
                let mut f = std::fs::File::create(pre_path).unwrap();
                f.write_all(&buf).unwrap();
            }

            assert!(reserialize_bank_with_new_accounts_hash(
                slot_dir,
                slot,
                &accounts_hash,
                incremental.as_ref(),
            ));
            let mut buf_reserialized;
            {
                let previous_len = buf.len();
                let expected = previous_len
                    + if incremental_snapshot_persistence {
                        // previously saved a none (size = sizeof_None), now added a Some
                        let sizeof_none = std::mem::size_of::<u64>();
                        let sizeof_incremental_snapshot_persistence =
                            std::mem::size_of::<Option<BankIncrementalSnapshotPersistence>>();
                        sizeof_incremental_snapshot_persistence - sizeof_none
                    } else {
                        // no change
                        0
                    };

                // +1: larger buffer than expected to make sure the file isn't larger than expected
                buf_reserialized = vec![0; expected + 1];
                let mut f = std::fs::File::open(post_path).unwrap();
                let size = f.read(&mut buf_reserialized).unwrap();

                assert_eq!(
                    size,
                    expected,
                    "(reserialize_accounts_hash, incremental_snapshot_persistence, update_accounts_hash, initial_epoch_accounts_hash): {:?}, previous_len: {previous_len}",
                    (
                        reserialize_accounts_hash,
                        incremental_snapshot_persistence,
                        update_accounts_hash,
                        initial_epoch_accounts_hash,
                    )
                );
                buf_reserialized.truncate(size);
            }
            if update_accounts_hash {
                // We cannot guarantee buffer contents are exactly the same if hash is the same.
                // Things like hashsets/maps have randomness in their in-mem representations.
                // This makes serialized bytes not deterministic.
                // But, we can guarantee that the buffer is different if we change the hash!
                assert_ne!(buf, buf_reserialized);
            }
            if update_accounts_hash || incremental_snapshot_persistence {
                buf = buf_reserialized;
            }
        }

        let rdr = Cursor::new(&buf[..]);
        let mut reader = std::io::BufReader::new(&buf[rdr.position() as usize..]);

        // Create a new set of directories for this bank's accounts
        let (_accounts_dir, dbank_paths) = get_temp_accounts_paths(4).unwrap();
        let mut status_cache = StatusCache::default();
        status_cache.add_root(2);
        // Create a directory to simulate AppendVecs unpackaged from a snapshot tar
        let copied_accounts = TempDir::new().unwrap();
        let storage_and_next_append_vec_id =
            copy_append_vecs(&bank2.rc.accounts.accounts_db, copied_accounts.path()).unwrap();
        let mut snapshot_streams = SnapshotStreams {
            full_snapshot_stream: &mut reader,
            incremental_snapshot_stream: None,
        };
        let mut dbank = crate::serde_snapshot::bank_from_streams(
            serde_style,
            &mut snapshot_streams,
            &dbank_paths,
            storage_and_next_append_vec_id,
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            Some(solana_accounts_db::accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        dbank.status_cache = Arc::new(RwLock::new(status_cache));
        assert_eq!(dbank.get_balance(&key1.pubkey()), 0);
        assert_eq!(dbank.get_balance(&key2.pubkey()), 10);
        assert_eq!(dbank.get_balance(&key3.pubkey()), 0);
        if let Some(incremental_snapshot_persistence) = incremental.clone() {
            assert_eq!(dbank.get_accounts_hash(), None,);
            assert_eq!(
                dbank.get_incremental_accounts_hash(),
                Some(incremental_snapshot_persistence.incremental_hash.into()),
            );
        } else {
            assert_eq!(dbank.get_accounts_hash(), Some(accounts_hash));
            assert_eq!(dbank.get_incremental_accounts_hash(), None);
        }
        assert!(bank2 == dbank);
        assert_eq!(dbank.incremental_snapshot_persistence, incremental);
        assert_eq!(dbank.get_epoch_accounts_hash_to_serialize().map(|epoch_accounts_hash| *epoch_accounts_hash.as_ref()), expected_epoch_accounts_hash,
                   "(reserialize_accounts_hash, incremental_snapshot_persistence, update_accounts_hash, initial_epoch_accounts_hash): {:?}",
                   (
                       reserialize_accounts_hash,
                       incremental_snapshot_persistence,
                       update_accounts_hash,
                       initial_epoch_accounts_hash,
                   )
        );
    }

    #[test]
    fn test_bank_serialize_newer() {
        for (reserialize_accounts_hash, update_accounts_hash) in
            [(false, false), (true, false), (true, true)]
        {
            let parameters = if reserialize_accounts_hash {
                [false, true].to_vec()
            } else {
                [false].to_vec()
            };
            for incremental_snapshot_persistence in parameters.clone() {
                for initial_epoch_accounts_hash in [false, true] {
                    test_bank_serialize_style(
                        SerdeStyle::Newer,
                        reserialize_accounts_hash,
                        update_accounts_hash,
                        incremental_snapshot_persistence,
                        initial_epoch_accounts_hash,
                    )
                }
            }
        }
    }

    fn add_root_and_flush_write_cache(bank: &Bank) {
        bank.rc.accounts.add_root(bank.slot());
        bank.flush_accounts_cache_slot_for_tests()
    }

    #[test]
    fn test_extra_fields_eof() {
        solana_logger::setup();
        let sample_rewards = (0..2)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();
        for epoch_reward_status_active in [None, Some(vec![]), Some(vec![sample_rewards])] {
            let (genesis_config, _) = create_genesis_config(500);

            let bank0 = Arc::new(Bank::new_for_tests_with_config(
                &genesis_config,
                BankTestConfig::default(),
            ));
            bank0.squash();
            let mut bank = Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 1);

            add_root_and_flush_write_cache(&bank0);
            bank.rc
                .accounts
                .accounts_db
                .set_accounts_delta_hash(bank.slot(), AccountsDeltaHash(Hash::new_unique()));
            bank.rc.accounts.accounts_db.set_accounts_hash(
                bank.slot(),
                (AccountsHash(Hash::new_unique()), u64::default()),
            );

            // Set extra fields
            bank.fee_rate_governor.lamports_per_signature = 7000;

            if let Some(rewards) = epoch_reward_status_active.as_ref() {
                assert_eq!(bank.block_height(), 1);
                bank.set_epoch_reward_status_active(rewards.clone());
            }

            // Serialize
            let snapshot_storages = bank.get_snapshot_storages(None);
            let mut buf = vec![];
            let mut writer = Cursor::new(&mut buf);

            crate::serde_snapshot::bank_to_stream(
                SerdeStyle::Newer,
                &mut std::io::BufWriter::new(&mut writer),
                &bank,
                &get_storages_to_serialize(&snapshot_storages),
            )
            .unwrap();

            // Deserialize
            let rdr = Cursor::new(&buf[..]);
            let mut reader = std::io::BufReader::new(&buf[rdr.position() as usize..]);
            let mut snapshot_streams = SnapshotStreams {
                full_snapshot_stream: &mut reader,
                incremental_snapshot_stream: None,
            };
            let (_accounts_dir, dbank_paths) = get_temp_accounts_paths(4).unwrap();
            let copied_accounts = TempDir::new().unwrap();
            let storage_and_next_append_vec_id =
                copy_append_vecs(&bank.rc.accounts.accounts_db, copied_accounts.path()).unwrap();
            let dbank = crate::serde_snapshot::bank_from_streams(
                SerdeStyle::Newer,
                &mut snapshot_streams,
                &dbank_paths,
                storage_and_next_append_vec_id,
                &genesis_config,
                &RuntimeConfig::default(),
                None,
                None,
                AccountSecondaryIndexes::default(),
                None,
                AccountShrinkThreshold::default(),
                false,
                Some(solana_accounts_db::accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING),
                None,
                Arc::default(),
            )
            .unwrap();

            assert_eq!(
                bank.fee_rate_governor.lamports_per_signature,
                dbank.fee_rate_governor.lamports_per_signature
            );

            // assert epoch_reward_status is the same as the set epoch reward status
            let epoch_reward_status = bank
                .get_epoch_reward_status_to_serialize()
                .unwrap_or(&EpochRewardStatus::Inactive);
            if let Some(rewards) = epoch_reward_status_active {
                assert_matches!(epoch_reward_status, EpochRewardStatus::Active(_));
                if let EpochRewardStatus::Active(StartBlockHeightAndRewards {
                    start_block_height,
                    ref stake_rewards_by_partition,
                }) = epoch_reward_status
                {
                    assert_eq!(*start_block_height, 1);
                    assert_eq!(&rewards[..], &stake_rewards_by_partition[..]);
                } else {
                    unreachable!("Epoch reward status should NOT be inactive.");
                }
            } else {
                assert_matches!(epoch_reward_status, EpochRewardStatus::Inactive);
            }
        }
    }

    #[test]
    fn test_extra_fields_full_snapshot_archive() {
        solana_logger::setup();

        let sample_rewards = (0..2)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();
        for epoch_reward_status_active in [None, Some(vec![]), Some(vec![sample_rewards])] {
            let (mut genesis_config, _) = create_genesis_config(500);
            activate_all_features(&mut genesis_config);

            let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
            let mut bank = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
            while !bank.is_complete() {
                bank.fill_bank_with_ticks_for_tests();
            }

            // Set extra field
            bank.fee_rate_governor.lamports_per_signature = 7000;

            if let Some(rewards) = epoch_reward_status_active.as_ref() {
                assert_eq!(bank.block_height(), 1);
                bank.set_epoch_reward_status_active(rewards.clone());
            }

            let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
            let bank_snapshots_dir = TempDir::new().unwrap();
            let full_snapshot_archives_dir = TempDir::new().unwrap();
            let incremental_snapshot_archives_dir = TempDir::new().unwrap();

            // Serialize
            let snapshot_archive_info = snapshot_bank_utils::bank_to_full_snapshot_archive(
                &bank_snapshots_dir,
                &bank,
                None,
                full_snapshot_archives_dir.path(),
                incremental_snapshot_archives_dir.path(),
                ArchiveFormat::TarBzip2,
                NonZeroUsize::new(1).unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();

            // Deserialize
            let (dbank, _) = snapshot_bank_utils::bank_from_snapshot_archives(
                &[accounts_dir],
                bank_snapshots_dir.path(),
                &snapshot_archive_info,
                None,
                &genesis_config,
                &RuntimeConfig::default(),
                None,
                None,
                AccountSecondaryIndexes::default(),
                None,
                AccountShrinkThreshold::default(),
                false,
                false,
                false,
                false,
                Some(solana_accounts_db::accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING),
                None,
                Arc::default(),
            )
            .unwrap();

            assert_eq!(
                bank.fee_rate_governor.lamports_per_signature,
                dbank.fee_rate_governor.lamports_per_signature
            );

            // assert epoch_reward_status is the same as the set epoch reward status
            let epoch_reward_status = bank
                .get_epoch_reward_status_to_serialize()
                .unwrap_or(&EpochRewardStatus::Inactive);
            if let Some(rewards) = epoch_reward_status_active {
                assert_matches!(epoch_reward_status, EpochRewardStatus::Active(_));
                if let EpochRewardStatus::Active(StartBlockHeightAndRewards {
                    start_block_height,
                    ref stake_rewards_by_partition,
                }) = epoch_reward_status
                {
                    assert_eq!(*start_block_height, 1);
                    assert_eq!(&rewards[..], &stake_rewards_by_partition[..]);
                } else {
                    unreachable!("Epoch reward status should NOT be inactive.");
                }
            } else {
                assert_matches!(epoch_reward_status, EpochRewardStatus::Inactive);
            }
        }
    }

    #[test]
    fn test_blank_extra_fields() {
        solana_logger::setup();
        let (genesis_config, _) = create_genesis_config(500);

        let bank0 = Arc::new(Bank::new_for_tests_with_config(
            &genesis_config,
            BankTestConfig::default(),
        ));
        bank0.squash();
        let mut bank = Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 1);
        add_root_and_flush_write_cache(&bank0);
        bank.rc
            .accounts
            .accounts_db
            .set_accounts_delta_hash(bank.slot(), AccountsDeltaHash(Hash::new_unique()));
        bank.rc.accounts.accounts_db.set_accounts_hash(
            bank.slot(),
            (AccountsHash(Hash::new_unique()), u64::default()),
        );

        // Set extra fields
        bank.fee_rate_governor.lamports_per_signature = 7000;

        // Serialize, but don't serialize the extra fields
        let snapshot_storages = bank.get_snapshot_storages(None);
        let mut buf = vec![];
        let mut writer = Cursor::new(&mut buf);

        crate::serde_snapshot::bank_to_stream_no_extra_fields(
            SerdeStyle::Newer,
            &mut std::io::BufWriter::new(&mut writer),
            &bank,
            &get_storages_to_serialize(&snapshot_storages),
        )
        .unwrap();

        // Deserialize
        let rdr = Cursor::new(&buf[..]);
        let mut reader = std::io::BufReader::new(&buf[rdr.position() as usize..]);
        let mut snapshot_streams = SnapshotStreams {
            full_snapshot_stream: &mut reader,
            incremental_snapshot_stream: None,
        };
        let (_accounts_dir, dbank_paths) = get_temp_accounts_paths(4).unwrap();
        let copied_accounts = TempDir::new().unwrap();
        let storage_and_next_append_vec_id =
            copy_append_vecs(&bank.rc.accounts.accounts_db, copied_accounts.path()).unwrap();
        let dbank = crate::serde_snapshot::bank_from_streams(
            SerdeStyle::Newer,
            &mut snapshot_streams,
            &dbank_paths,
            storage_and_next_append_vec_id,
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            Some(solana_accounts_db::accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();

        // Defaults to 0
        assert_eq!(0, dbank.fee_rate_governor.lamports_per_signature);

        // epoch_reward status should default to `Inactive`
        let epoch_reward_status = bank
            .get_epoch_reward_status_to_serialize()
            .unwrap_or(&EpochRewardStatus::Inactive);
        assert_matches!(epoch_reward_status, EpochRewardStatus::Inactive);
    }

    #[cfg(RUSTC_WITH_SPECIALIZATION)]
    mod test_bank_serialize {
        use {super::*, crate::serde_snapshot::serialize_test_bank_and_storage};

        // This some what long test harness is required to freeze the ABI of
        // Bank's serialization due to versioned nature
        #[frozen_abi(digest = "12WNiuA7qeLU8JFweQszX5sCnCj1fYnYV4i9DeACqhQD")]
        #[derive(Serialize, AbiExample)]
        pub struct BankAbiTestWrapperNewer {
            #[serde(serialize_with = "wrapper_newer")]
            bank: Bank,
        }

        pub fn wrapper_newer<S>(bank: &Bank, s: S) -> std::result::Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            bank.rc
                .accounts
                .accounts_db
                .set_accounts_delta_hash(bank.slot(), AccountsDeltaHash(Hash::new_unique()));
            bank.rc.accounts.accounts_db.set_accounts_hash(
                bank.slot(),
                (AccountsHash(Hash::new_unique()), u64::default()),
            );
            let snapshot_storages = bank.rc.accounts.accounts_db.get_snapshot_storages(..=0).0;
            // ensure there is a single snapshot storage example for ABI digesting
            assert_eq!(snapshot_storages.len(), 1);

            serialize_test_bank_and_storage::<S>(
                bank,
                &get_storages_to_serialize(&snapshot_storages),
                s,
            )
        }
    }
}
