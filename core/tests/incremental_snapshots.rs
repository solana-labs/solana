// Long-running bank_forks tests
#![allow(clippy::integer_arithmetic)]

macro_rules! DEFINE_SNAPSHOT_VERSION_PARAMETERIZED_TEST_FUNCTIONS {
    ($x:ident, $y:ident, $z:ident) => {
        #[allow(non_snake_case)]
        mod $z {
            use super::*;

            const SNAPSHOT_VERSION: SnapshotVersion = SnapshotVersion::$x;
            const CLUSTER_TYPE: ClusterType = ClusterType::$y;

            #[test]
            fn test_bank_forks_incremental_snapshot_n() {
                run_test_bank_forks_incremental_snapshot_n(SNAPSHOT_VERSION, CLUSTER_TYPE)
            }

            #[test]
            fn test_concurrent_snapshot_packaging() {
                run_test_concurrent_snapshot_packaging(SNAPSHOT_VERSION, CLUSTER_TYPE)
            }

            #[test]
            fn test_slots_to_snapshot() {
                run_test_slots_to_snapshot(SNAPSHOT_VERSION, CLUSTER_TYPE)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use bincode::serialize_into;
    use crossbeam_channel::unbounded;
    use fs_extra::dir::CopyOptions;
    use itertools::Itertools;
    use log::info;
    use solana_core::snapshot_packager_service::{PendingSnapshotPackage, SnapshotPackagerService};
    use solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo};
    use solana_runtime::{
        accounts_background_service::{AbsRequestSender, SnapshotRequestHandler},
        accounts_db,
        accounts_index::AccountSecondaryIndexes,
        bank::{Bank, BankSlotDelta},
        bank_forks::{ArchiveFormat, BankForks, SnapshotConfig},
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        snapshot_utils,
        snapshot_utils::{SnapshotVersion, DEFAULT_MAX_SNAPSHOTS_TO_RETAIN},
        status_cache::MAX_CACHE_ENTRIES,
    };
    use solana_sdk::{
        clock::Slot,
        genesis_config::{ClusterType, GenesisConfig},
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_transaction,
    };
    use std::{
        collections::HashSet,
        fmt::Debug,
        fs,
        io::{Error, ErrorKind},
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::channel,
            Arc,
        },
        time::Duration,
    };
    use tempfile::TempDir;

    DEFINE_SNAPSHOT_VERSION_PARAMETERIZED_TEST_FUNCTIONS!(V1_2_0, Development, V1_2_0_Development);
    DEFINE_SNAPSHOT_VERSION_PARAMETERIZED_TEST_FUNCTIONS!(V1_2_0, Devnet, V1_2_0_Devnet);
    DEFINE_SNAPSHOT_VERSION_PARAMETERIZED_TEST_FUNCTIONS!(V1_2_0, Testnet, V1_2_0_Testnet);
    DEFINE_SNAPSHOT_VERSION_PARAMETERIZED_TEST_FUNCTIONS!(V1_2_0, MainnetBeta, V1_2_0_MainnetBeta);

    struct IncrementalSnapshotTestConfig {
        accounts_dir: TempDir,
        snapshot_dir: TempDir,
        snapshot_config: SnapshotConfig,
        bank_forks: BankForks,
        genesis_config_info: GenesisConfigInfo,
    }

    impl IncrementalSnapshotTestConfig {
        fn new(
            snapshot_version: SnapshotVersion,
            cluster_type: ClusterType,
            snapshot_interval_slots: Slot,
            incremental_snapshot_interval_slots: Slot,
        ) -> IncrementalSnapshotTestConfig {
            let accounts_dir = TempDir::new().unwrap();
            let snapshot_dir = TempDir::new().unwrap();
            let snapshot_output_path = TempDir::new().unwrap();
            let mut genesis_config_info = create_genesis_config(10_000);
            genesis_config_info.genesis_config.cluster_type = cluster_type;
            let bank0 = Bank::new_with_paths(
                &genesis_config_info.genesis_config,
                vec![accounts_dir.path().to_path_buf()],
                &[],
                None,
                None,
                AccountSecondaryIndexes::default(),
                false,
                accounts_db::AccountShrinkThreshold::default(),
            );
            bank0.freeze();
            let mut bank_forks = BankForks::new(bank0);
            bank_forks.accounts_hash_interval_slots = incremental_snapshot_interval_slots;

            let snapshot_config = SnapshotConfig {
                snapshot_interval_slots,
                snapshot_package_output_path: PathBuf::from(snapshot_output_path.path()),
                snapshot_path: PathBuf::from(snapshot_dir.path()),
                archive_format: ArchiveFormat::TarBzip2,
                snapshot_version,
                maximum_snapshots_to_retain: std::usize::MAX, // bprumo TODO: DEFAULT_MAX_SNAPSHOTS_TO_RETAIN,
                incremental_snapshot_interval_slots,
            };
            bank_forks.set_snapshot_config(Some(snapshot_config.clone()));
            IncrementalSnapshotTestConfig {
                accounts_dir,
                snapshot_dir,
                snapshot_config,
                bank_forks,
                genesis_config_info,
            }
        }
    }

    // creates banks up to "last_slot" and runs the input function `f` on each bank created
    // also marks each bank as root and generates snapshots
    // finally tries to restore from the last bank's snapshot and compares the restored bank to the
    // `last_slot` bank
    fn run_bank_forks_incremental_snapshot_n<F>(
        snapshot_version: SnapshotVersion,
        cluster_type: ClusterType,
        full_snapshot_interval_slots: Slot,
        incremental_snapshot_interval_slots: Slot,
        last_slot: Slot,
        set_root_interval: Slot,
        f: F,
    ) where
        F: Fn(&mut Bank, &Keypair),
    {
        solana_logger::setup();
        assert!(last_slot > 0);
        assert!(last_slot > full_snapshot_interval_slots);
        assert!(last_slot > incremental_snapshot_interval_slots);
        assert!(last_slot >= full_snapshot_interval_slots + incremental_snapshot_interval_slots);
        assert!(full_snapshot_interval_slots % set_root_interval == 0);
        assert!(incremental_snapshot_interval_slots % set_root_interval == 0);
        let archive_format = ArchiveFormat::TarBzip2;

        info!("Running bank forks incremental snapshot test, full snapshot interval: {} slots, incremental snapshot interval: {} slots, last slot: {}, set root interval: {} slots",
              full_snapshot_interval_slots, incremental_snapshot_interval_slots, last_slot, set_root_interval);

        // Set up snapshotting config
        let mut snapshot_test_config = IncrementalSnapshotTestConfig::new(
            snapshot_version,
            cluster_type,
            full_snapshot_interval_slots,
            incremental_snapshot_interval_slots,
        );

        let bank_forks = &mut snapshot_test_config.bank_forks;
        let mint_keypair = &snapshot_test_config.genesis_config_info.mint_keypair;

        let (snapshot_request_sender, snapshot_request_receiver) = unbounded();
        let (accounts_package_sender, _) = channel();
        let request_sender = AbsRequestSender::new(Some(snapshot_request_sender));
        let snapshot_request_handler = SnapshotRequestHandler {
            snapshot_config: snapshot_test_config.snapshot_config.clone(),
            snapshot_request_receiver,
            accounts_package_sender,
        };

        /*
         * let mut full_snapshot_archive_path = PathBuf::default();
         */
        let mut last_full_snapshot_slot = None;
        for slot in 1..=last_slot {
            let mut bank = Bank::new_from_parent(&bank_forks[slot - 1], &Pubkey::default(), slot);
            f(&mut bank, &mint_keypair);
            let bank = bank_forks.insert(bank);

            // Set root to make sure we don't end up with too many account storage entries
            // and to allow snapshotting of bank and the purging logic on status_cache to
            // kick in
            if slot % set_root_interval == 0 {
                // set_root should send a snapshot request
                bank_forks.set_root(bank.slot(), &request_sender, None);
                bank.update_accounts_hash();
                snapshot_request_handler.handle_snapshot_requests(false, false, false);
            }

            // Since AccountsBackgroundService isn't running, manually make a full snapshot archive
            // at the right interval
            if slot % full_snapshot_interval_slots == 0 {
                make_full_snapshot_archive(
                    &bank,
                    &snapshot_test_config.snapshot_config.snapshot_path,
                    &snapshot_test_config
                        .snapshot_config
                        .snapshot_package_output_path,
                    archive_format,
                    snapshot_version,
                )
                .unwrap();
                last_full_snapshot_slot = Some(slot);
            }
            // Similarly, make an incremental snapshot archive at the right interval, but only if
            // there's been at least one full snapshot first
            else if slot % incremental_snapshot_interval_slots == 0
                && last_full_snapshot_slot.is_some()
            {
                make_incremental_snapshot_archive(
                    &bank,
                    &snapshot_test_config.snapshot_config.snapshot_path,
                    &snapshot_test_config
                        .snapshot_config
                        .snapshot_package_output_path,
                    archive_format,
                    snapshot_version,
                )
                .unwrap();

                let account_paths = &[snapshot_test_config.accounts_dir.path().to_path_buf()];
                let genesis_config = &snapshot_test_config.genesis_config_info.genesis_config;
                restore_from_incremental_snapshot_and_check_banks_are_equal(
                    &bank,
                    account_paths,
                    genesis_config,
                    &snapshot_test_config.snapshot_config.snapshot_path,
                    &snapshot_test_config
                        .snapshot_config
                        .snapshot_package_output_path,
                    archive_format,
                    last_full_snapshot_slot.unwrap(),
                    slot,
                );
            }
        }
    }

    fn run_test_bank_forks_incremental_snapshot_n(
        snapshot_version: SnapshotVersion,
        cluster_type: ClusterType,
    ) {
        let set_root_interval = 2;
        let incremental_snapshot_interval_slots = set_root_interval * 2;
        let full_snapshot_interval_slots = incremental_snapshot_interval_slots * 5;
        let last_slot = full_snapshot_interval_slots * 3 - 1;

        run_bank_forks_incremental_snapshot_n(
            snapshot_version,
            cluster_type,
            full_snapshot_interval_slots,
            incremental_snapshot_interval_slots,
            last_slot,
            set_root_interval,
            |bank, mint_keypair| {
                let key1 = Keypair::new().pubkey();
                let tx =
                    system_transaction::transfer(&mint_keypair, &key1, 1, bank.last_blockhash());
                assert_eq!(bank.process_transaction(&tx), Ok(()));

                let key2 = Keypair::new().pubkey();
                let tx =
                    system_transaction::transfer(&mint_keypair, &key2, 0, bank.last_blockhash());
                assert_eq!(bank.process_transaction(&tx), Ok(()));

                while !bank.is_complete() {
                    bank.register_tick(&Hash::new_unique());
                }

                bank.freeze();
            },
        );
    }

    fn make_full_snapshot_archive<P, Q>(
        bank: &Bank,
        snapshot_path: P,
        snapshot_package_output_path: Q,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
    ) -> snapshot_utils::Result<()>
    where
        P: AsRef<Path> + Debug,
        Q: AsRef<Path> + Debug,
    {
        let slot = bank.slot();
        info!("Making full snapshot archive from bank at slot: {}", slot);
        let slot_snapshot_path = snapshot_utils::get_snapshot_paths(snapshot_path.as_ref())
            .into_iter()
            .find(|elem| elem.slot == slot)
            .ok_or_else(|| Error::new(ErrorKind::Other, "did not find snapshot with this path"))?;
        let snapshot_package = snapshot_utils::package_snapshot(
            bank,
            &slot_snapshot_path,
            snapshot_path.as_ref(),
            bank.src.slot_deltas(&bank.src.roots()),
            snapshot_package_output_path.as_ref(),
            bank.get_snapshot_storages(),
            archive_format,
            snapshot_version,
            None,
        )?;
        let snapshot_package = snapshot_utils::process_accounts_package_pre(
            snapshot_package,
            Some(&bank.get_thread_pool()),
        );
        snapshot_utils::archive_snapshot_package(
            &snapshot_package,
            std::usize::MAX, // bprumo TODO: DEFAULT_MAX_SNAPSHOTS_TO_RETAIN,
        )
    }

    fn make_incremental_snapshot_archive<P, Q>(
        bank: &Bank,
        snapshot_path: P,
        snapshot_package_output_path: Q,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
    ) -> snapshot_utils::Result<()>
    where
        P: AsRef<Path> + Debug,
        Q: AsRef<Path> + Debug,
    {
        let full_snapshot_archive_slot = snapshot_utils::get_highest_snapshot_archive_slot(
            snapshot_package_output_path.as_ref(),
        )
        .ok_or_else(|| Error::new(ErrorKind::Other, "no full snapshot archive"))?;

        let slot = bank.slot();
        info!(
            "Making incremental snapshot archive from bank at slot: {}, and full snapshot slot: {}",
            slot, full_snapshot_archive_slot
        );
        let slot_snapshot_path = snapshot_utils::get_snapshot_paths(snapshot_path.as_ref())
            .into_iter()
            .find(|elem| elem.slot == slot)
            .ok_or_else(|| Error::new(ErrorKind::Other, "did not find snapshot with this path"))?;
        let incremental_snapshot_package = snapshot_utils::package_incremental_snapshot(
            bank,
            full_snapshot_archive_slot,
            &slot_snapshot_path,
            snapshot_path.as_ref(),
            bank.src.slot_deltas(&bank.src.roots()),
            snapshot_package_output_path.as_ref(),
            bank.get_incremental_snapshot_storages(full_snapshot_archive_slot),
            archive_format,
            snapshot_version,
            None,
        )?;
        let incremental_snapshot_package = snapshot_utils::process_accounts_package_pre_incremental(
            incremental_snapshot_package,
            Some(&bank.get_thread_pool()),
            full_snapshot_archive_slot,
        );
        snapshot_utils::archive_incremental_snapshot_package(
            &incremental_snapshot_package,
            std::usize::MAX, // bprumo TODO: DEFAULT_MAX_SNAPSHOTS_TO_RETAIN,
            full_snapshot_archive_slot,
        )
    }

    fn restore_from_incremental_snapshot_and_check_banks_are_equal<P, Q>(
        bank: &Bank,
        account_paths: &[PathBuf],
        genesis_config: &GenesisConfig,
        snapshot_path: P,
        snapshot_package_output_path: Q,
        archive_format: ArchiveFormat,
        last_full_snapshot_slot: Slot,
        last_incremental_snapshot_slot: Slot,
    ) where
        P: AsRef<Path> + Debug,
        Q: AsRef<Path> + Debug,
    {
        let (
            full_snapshot_archive_slot,
            (base_incremental_snapshot_archive_slot, incremental_snapshot_archive_slot),
            deserialized_bank,
        ) = restore_from_incremental_snapshot(
            &account_paths,
            &genesis_config,
            snapshot_path,
            snapshot_package_output_path,
            archive_format,
        )
        .unwrap();

        assert_eq!(
            full_snapshot_archive_slot,
            base_incremental_snapshot_archive_slot
        );
        assert_eq!(full_snapshot_archive_slot, last_full_snapshot_slot);
        assert_eq!(
            incremental_snapshot_archive_slot,
            last_incremental_snapshot_slot
        );
        assert_eq!(*bank, deserialized_bank);
    }

    fn restore_from_incremental_snapshot<P, Q>(
        account_paths: &[PathBuf],
        genesis_config: &GenesisConfig,
        snapshot_path: P,
        snapshot_package_output_path: Q,
        archive_format: ArchiveFormat,
    ) -> snapshot_utils::Result<(Slot, (Slot, Slot), Bank)>
    where
        P: AsRef<Path> + Debug,
        Q: AsRef<Path> + Debug,
    {
        let full_snapshot_archive_info = snapshot_utils::get_highest_snapshot_archive_info(
            snapshot_package_output_path.as_ref(),
        )
        .ok_or_else(|| Error::new(ErrorKind::Other, "no full snapshot"))?;

        let incremental_snapshot_archive_info =
            snapshot_utils::get_highest_incremental_snapshot_archive_info(
                snapshot_package_output_path.as_ref(),
                full_snapshot_archive_info.slot,
            )
            .ok_or_else(|| Error::new(ErrorKind::Other, "no incremental snapshot"))?;

        info!("Restoring bank from full snapshot slot: {}, and incremental snapshot slot: {} (with base slot: {})",
        full_snapshot_archive_info.slot, incremental_snapshot_archive_info.slot, incremental_snapshot_archive_info.base_slot);
        let deserialized_bank = snapshot_utils::bank_from_incremental_snapshot_archive(
            &account_paths,
            &[],
            snapshot_path.as_ref(),
            &full_snapshot_archive_info.path,
            &incremental_snapshot_archive_info.path,
            archive_format,
            genesis_config,
            None,
            None,
            AccountSecondaryIndexes::default(),
            false,
            None,
            accounts_db::AccountShrinkThreshold::default(),
        )?;

        Ok((
            full_snapshot_archive_info.slot,
            (
                incremental_snapshot_archive_info.base_slot,
                incremental_snapshot_archive_info.slot,
            ),
            deserialized_bank,
        ))
    }

    fn run_test_concurrent_snapshot_packaging(
        snapshot_version: SnapshotVersion,
        cluster_type: ClusterType,
    ) {
        solana_logger::setup();

        // Set up snapshotting config
        let mut snapshot_test_config =
            IncrementalSnapshotTestConfig::new(snapshot_version, cluster_type, 100, 1);

        let bank_forks = &mut snapshot_test_config.bank_forks;
        let snapshots_dir = &snapshot_test_config.snapshot_dir;
        let snapshot_config = &snapshot_test_config.snapshot_config;
        let snapshot_path = &snapshot_config.snapshot_path;
        let snapshot_package_output_path = &snapshot_config.snapshot_package_output_path;
        let mint_keypair = &snapshot_test_config.genesis_config_info.mint_keypair;
        let genesis_config = &snapshot_test_config.genesis_config_info.genesis_config;

        // Take snapshot of zeroth bank
        let bank0 = bank_forks.get(0).unwrap();
        let storages = bank0.get_snapshot_storages();
        snapshot_utils::add_snapshot(snapshot_path, bank0, &storages, snapshot_version).unwrap();

        // Set up snapshotting channels
        let (sender, receiver) = channel();
        let (fake_sender, _fake_receiver) = channel();

        // Create next MAX_CACHE_ENTRIES + 2 banks and snapshots. Every bank will get snapshotted
        // and the snapshot purging logic will run on every snapshot taken. This means the three
        // (including snapshot for bank0 created above) earliest snapshots will get purged by the
        // time this loop is done.

        // Also, make a saved copy of the state of the snapshot for a bank with
        // bank.slot == saved_slot, so we can use it for a correctness check later.
        let saved_snapshots_dir = TempDir::new().unwrap();
        let saved_accounts_dir = TempDir::new().unwrap();
        let saved_slot = 4;
        let mut saved_archive_path = None;

        for forks in 0..snapshot_utils::MAX_SNAPSHOTS + 2 {
            let bank = Bank::new_from_parent(
                &bank_forks[forks as u64],
                &Pubkey::default(),
                (forks + 1) as u64,
            );
            let slot = bank.slot();
            let key1 = Keypair::new().pubkey();
            let tx = system_transaction::transfer(&mint_keypair, &key1, 1, genesis_config.hash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));
            bank.squash();
            let accounts_hash = bank.update_accounts_hash();

            let package_sender = {
                if slot == saved_slot as u64 {
                    // Only send one package on the real sender so that the packaging service
                    // doesn't take forever to run the packaging logic on all MAX_CACHE_ENTRIES
                    // later
                    &sender
                } else {
                    &fake_sender
                }
            };

            snapshot_utils::snapshot_bank(
                &bank,
                vec![],
                &package_sender,
                &snapshot_path,
                &snapshot_package_output_path,
                snapshot_config.snapshot_version,
                &snapshot_config.archive_format,
                None,
            )
            .unwrap();

            bank_forks.insert(bank);
            if slot == saved_slot as u64 {
                // Find the relevant snapshot storages
                let snapshot_storage_files: HashSet<_> = bank_forks[slot]
                    .get_snapshot_storages()
                    .into_iter()
                    .flatten()
                    .map(|s| s.get_path())
                    .collect();

                // Only save off the files returned by `get_snapshot_storages`. This is because
                // some of the storage entries in the accounts directory may be filtered out by
                // `get_snapshot_storages()` and will not be included in the snapshot. Ultimately,
                // this means copying naitvely everything in `accounts_dir` to the `saved_accounts_dir`
                // will lead to test failure by mismatch when `saved_accounts_dir` is compared to
                // the unpacked snapshot later in this test's call to `verify_snapshot_archive()`.
                for file in snapshot_storage_files {
                    fs::copy(
                        &file,
                        &saved_accounts_dir.path().join(file.file_name().unwrap()),
                    )
                    .unwrap();
                }
                let last_snapshot_path = fs::read_dir(snapshot_path)
                    .unwrap()
                    .filter_map(|entry| {
                        let e = entry.unwrap();
                        let file_path = e.path();
                        let file_name = file_path.file_name().unwrap();
                        file_name
                            .to_str()
                            .map(|s| s.parse::<u64>().ok().map(|_| file_path.clone()))
                            .unwrap_or(None)
                    })
                    .sorted()
                    .last()
                    .unwrap();
                // only save off the snapshot of this slot, we don't need the others.
                let options = CopyOptions::new();
                fs_extra::dir::copy(&last_snapshot_path, &saved_snapshots_dir, &options).unwrap();

                saved_archive_path = Some(snapshot_utils::build_snapshot_archive_path(
                    snapshot_package_output_path.to_path_buf(),
                    slot,
                    &accounts_hash,
                    ArchiveFormat::TarBzip2,
                ));
            }
        }

        // Purge all the outdated snapshots, including the ones needed to generate the package
        // currently sitting in the channel
        snapshot_utils::purge_old_snapshots(&snapshot_path);
        assert!(snapshot_utils::get_snapshot_paths(&snapshots_dir)
            .into_iter()
            .map(|path| path.slot)
            .eq(3..=snapshot_utils::MAX_SNAPSHOTS as u64 + 2));

        // Create a SnapshotPackagerService to create tarballs from all the pending
        // SnapshotPackage's on the channel. By the time this service starts, we have already
        // purged the first two snapshots, which are needed by every snapshot other than
        // the last two snapshots. However, the packaging service should still be able to
        // correctly construct the earlier snapshots because the SnapshotPackage's on the
        // channel hold hard links to these deleted snapshots. We verify this is the case below.
        let exit = Arc::new(AtomicBool::new(false));

        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default()));

        let pending_snapshot_package = PendingSnapshotPackage::default();
        let snapshot_packager_service = SnapshotPackagerService::new(
            pending_snapshot_package.clone(),
            None,
            &exit,
            &cluster_info,
            DEFAULT_MAX_SNAPSHOTS_TO_RETAIN,
        );

        let thread_pool = accounts_db::make_min_priority_thread_pool();

        let _package_receiver = std::thread::Builder::new()
            .name("package-receiver".to_string())
            .spawn(move || {
                while let Ok(mut snapshot_package) = receiver.recv() {
                    // Only package the latest
                    while let Ok(new_snapshot_package) = receiver.try_recv() {
                        snapshot_package = new_snapshot_package;
                    }

                    let snapshot_package =
                        solana_runtime::snapshot_utils::process_accounts_package_pre(
                            snapshot_package,
                            Some(&thread_pool),
                        );
                    *pending_snapshot_package.lock().unwrap() = Some(snapshot_package);
                }

                // Wait until the package is consumed by SnapshotPackagerService
                while pending_snapshot_package.lock().unwrap().is_some() {
                    std::thread::sleep(Duration::from_millis(100));
                }

                // Shutdown SnapshotPackagerService
                exit.store(true, Ordering::Relaxed);
            })
            .unwrap();

        // Close the channel so that the package receiver will exit after reading all the
        // packages off the channel
        drop(sender);

        // Wait for service to finish
        snapshot_packager_service
            .join()
            .expect("SnapshotPackagerService exited with error");

        // Check the archive we cached the state for earlier was generated correctly

        // before we compare, stick an empty status_cache in this dir so that the package comparison works
        // This is needed since the status_cache is added by the packager and is not collected from
        // the source dir for snapshots
        snapshot_utils::serialize_snapshot_data_file(
            &saved_snapshots_dir
                .path()
                .join(snapshot_utils::SNAPSHOT_STATUS_CACHE_FILE_NAME),
            |stream| {
                serialize_into(stream, &[] as &[BankSlotDelta])?;
                Ok(())
            },
        )
        .unwrap();

        snapshot_utils::verify_snapshot_archive(
            saved_archive_path.unwrap(),
            saved_snapshots_dir.path(),
            saved_accounts_dir.path(),
            ArchiveFormat::TarBzip2,
        );
    }

    fn run_test_slots_to_snapshot(snapshot_version: SnapshotVersion, cluster_type: ClusterType) {
        solana_logger::setup();
        let num_set_roots = MAX_CACHE_ENTRIES * 2;

        for add_root_interval in &[1, 3, 9] {
            let (snapshot_sender, _snapshot_receiver) = unbounded();
            // Make sure this test never clears bank.slots_since_snapshot
            let mut snapshot_test_config = IncrementalSnapshotTestConfig::new(
                snapshot_version,
                cluster_type,
                (*add_root_interval * num_set_roots * 2) as u64,
                std::u64::MAX,
            );
            let mut current_bank = snapshot_test_config.bank_forks[0].clone();
            let request_sender = AbsRequestSender::new(Some(snapshot_sender));
            for _ in 0..num_set_roots {
                for _ in 0..*add_root_interval {
                    let new_slot = current_bank.slot() + 1;
                    let new_bank =
                        Bank::new_from_parent(&current_bank, &Pubkey::default(), new_slot);
                    snapshot_test_config.bank_forks.insert(new_bank);
                    current_bank = snapshot_test_config.bank_forks[new_slot].clone();
                }
                snapshot_test_config.bank_forks.set_root(
                    current_bank.slot(),
                    &request_sender,
                    None,
                );
            }

            let num_old_slots = num_set_roots * *add_root_interval - MAX_CACHE_ENTRIES + 1;
            let expected_slots_to_snapshot =
                num_old_slots as u64..=num_set_roots as u64 * *add_root_interval as u64;

            let slots_to_snapshot = snapshot_test_config
                .bank_forks
                .get(snapshot_test_config.bank_forks.root())
                .unwrap()
                .src
                .roots();
            assert!(slots_to_snapshot.into_iter().eq(expected_slots_to_snapshot));
        }
    }
}
