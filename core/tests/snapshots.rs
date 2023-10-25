#![allow(clippy::arithmetic_side_effects)]

use {
    crate::snapshot_utils::create_tmp_accounts_dir_for_tests,
    crossbeam_channel::unbounded,
    fs_extra::dir::CopyOptions,
    itertools::Itertools,
    log::{info, trace},
    solana_accounts_db::{
        accounts_db::{self, CalcAccountsHashDataSource, ACCOUNTS_DB_CONFIG_FOR_TESTING},
        accounts_hash::AccountsHash,
        accounts_index::AccountSecondaryIndexes,
        epoch_accounts_hash::EpochAccountsHash,
    },
    solana_core::{
        accounts_hash_verifier::AccountsHashVerifier,
        snapshot_packager_service::SnapshotPackagerService,
    },
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_runtime::{
        accounts_background_service::{
            AbsRequestHandlers, AbsRequestSender, AccountsBackgroundService,
            PrunedBanksRequestHandler, SendDroppedBankCallback, SnapshotRequestHandler,
        },
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
        runtime_config::RuntimeConfig,
        snapshot_archive_info::FullSnapshotArchiveInfo,
        snapshot_bank_utils::{self, DISABLED_SNAPSHOT_ARCHIVE_INTERVAL},
        snapshot_config::SnapshotConfig,
        snapshot_hash::SnapshotHash,
        snapshot_package::{AccountsPackage, AccountsPackageKind, SnapshotKind, SnapshotPackage},
        snapshot_utils::{
            self,
            SnapshotVersion::{self, V1_2_0},
        },
        status_cache::MAX_CACHE_ENTRIES,
    },
    solana_sdk::{
        clock::Slot,
        genesis_config::{
            ClusterType::{self, Development, Devnet, MainnetBeta, Testnet},
            GenesisConfig,
        },
        hash::{hashv, Hash},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_transaction,
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        collections::HashSet,
        fs,
        io::{Error, ErrorKind},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        time::{Duration, Instant},
    },
    tempfile::TempDir,
    test_case::test_case,
};

struct SnapshotTestConfig {
    bank_forks: Arc<RwLock<BankForks>>,
    genesis_config_info: GenesisConfigInfo,
    snapshot_config: SnapshotConfig,
    incremental_snapshot_archives_dir: TempDir,
    full_snapshot_archives_dir: TempDir,
    bank_snapshots_dir: TempDir,
    accounts_dir: PathBuf,
    // as the underscore prefix indicates, this isn't explictly used; but it's needed to keep
    // TempDir::drop from running to retain that dir for the duration of test
    _accounts_tmp_dir: TempDir,
}

impl SnapshotTestConfig {
    fn new(
        snapshot_version: SnapshotVersion,
        cluster_type: ClusterType,
        accounts_hash_interval_slots: Slot,
        full_snapshot_archive_interval_slots: Slot,
        incremental_snapshot_archive_interval_slots: Slot,
    ) -> SnapshotTestConfig {
        let (accounts_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let bank_snapshots_dir = TempDir::new().unwrap();
        let full_snapshot_archives_dir = TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = TempDir::new().unwrap();
        // validator_stake_lamports should be non-zero otherwise stake
        // account will not be stored in accounts-db but still cached in
        // bank stakes which results in mismatch when banks are loaded from
        // snapshots.
        let mut genesis_config_info = create_genesis_config_with_leader(
            10_000,                          // mint_lamports
            &solana_sdk::pubkey::new_rand(), // validator_pubkey
            1,                               // validator_stake_lamports
        );
        genesis_config_info.genesis_config.cluster_type = cluster_type;
        let bank0 = Bank::new_with_paths_for_tests(
            &genesis_config_info.genesis_config,
            Arc::<RuntimeConfig>::default(),
            vec![accounts_dir.clone()],
            AccountSecondaryIndexes::default(),
            accounts_db::AccountShrinkThreshold::default(),
        );
        bank0.freeze();
        bank0.set_startup_verification_complete();
        let bank_forks_arc = BankForks::new_rw_arc(bank0);
        let mut bank_forks = bank_forks_arc.write().unwrap();
        bank_forks.accounts_hash_interval_slots = accounts_hash_interval_slots;

        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots,
            incremental_snapshot_archive_interval_slots,
            full_snapshot_archives_dir: full_snapshot_archives_dir.path().to_path_buf(),
            incremental_snapshot_archives_dir: incremental_snapshot_archives_dir
                .path()
                .to_path_buf(),
            bank_snapshots_dir: bank_snapshots_dir.path().to_path_buf(),
            snapshot_version,
            ..SnapshotConfig::default()
        };
        bank_forks.set_snapshot_config(Some(snapshot_config.clone()));
        SnapshotTestConfig {
            bank_forks: bank_forks_arc.clone(),
            genesis_config_info,
            snapshot_config,
            incremental_snapshot_archives_dir,
            full_snapshot_archives_dir,
            bank_snapshots_dir,
            accounts_dir,
            _accounts_tmp_dir: accounts_tmp_dir,
        }
    }
}

fn restore_from_snapshot(
    old_bank_forks: Arc<RwLock<BankForks>>,
    old_last_slot: Slot,
    old_genesis_config: &GenesisConfig,
    account_paths: &[PathBuf],
) {
    let old_bank_forks = old_bank_forks.read().unwrap();
    let snapshot_config = old_bank_forks.snapshot_config.as_ref().unwrap();
    let old_last_bank = old_bank_forks.get(old_last_slot).unwrap();

    let check_hash_calculation = false;
    let full_snapshot_archive_path = snapshot_utils::build_full_snapshot_archive_path(
        &snapshot_config.full_snapshot_archives_dir,
        old_last_bank.slot(),
        &old_last_bank.get_snapshot_hash(),
        snapshot_config.archive_format,
    );
    let full_snapshot_archive_info =
        FullSnapshotArchiveInfo::new_from_path(full_snapshot_archive_path).unwrap();

    let (deserialized_bank, _timing) = snapshot_bank_utils::bank_from_snapshot_archives(
        account_paths,
        &snapshot_config.bank_snapshots_dir,
        &full_snapshot_archive_info,
        None,
        old_genesis_config,
        &RuntimeConfig::default(),
        None,
        None,
        AccountSecondaryIndexes::default(),
        None,
        accounts_db::AccountShrinkThreshold::default(),
        check_hash_calculation,
        false,
        false,
        false,
        Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        Arc::default(),
    )
    .unwrap();
    deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();

    let bank = old_bank_forks.get(deserialized_bank.slot()).unwrap();
    assert_eq!(bank.as_ref(), &deserialized_bank);
}

// creates banks up to "last_slot" and runs the input function `f` on each bank created
// also marks each bank as root and generates snapshots
// finally tries to restore from the last bank's snapshot and compares the restored bank to the
// `last_slot` bank
fn run_bank_forks_snapshot_n<F>(
    snapshot_version: SnapshotVersion,
    cluster_type: ClusterType,
    last_slot: Slot,
    f: F,
    set_root_interval: u64,
) where
    F: Fn(&mut Bank, &Keypair),
{
    solana_logger::setup();
    // Set up snapshotting config
    let snapshot_test_config = SnapshotTestConfig::new(
        snapshot_version,
        cluster_type,
        set_root_interval,
        set_root_interval,
        DISABLED_SNAPSHOT_ARCHIVE_INTERVAL,
    );

    let bank_forks = snapshot_test_config.bank_forks.clone();
    let mint_keypair = &snapshot_test_config.genesis_config_info.mint_keypair;

    let (accounts_package_sender, _accounts_package_receiver) = crossbeam_channel::unbounded();
    let (snapshot_request_sender, snapshot_request_receiver) = unbounded();
    let request_sender = AbsRequestSender::new(snapshot_request_sender.clone());
    let snapshot_request_handler = SnapshotRequestHandler {
        snapshot_config: snapshot_test_config.snapshot_config.clone(),
        snapshot_request_sender,
        snapshot_request_receiver,
        accounts_package_sender,
    };
    for slot in 1..=last_slot {
        let mut bank = Bank::new_from_parent(
            bank_forks.read().unwrap().get(slot - 1).unwrap().clone(),
            &Pubkey::default(),
            slot,
        );
        f(&mut bank, mint_keypair);
        let bank = bank_forks.write().unwrap().insert(bank);
        // Set root to make sure we don't end up with too many account storage entries
        // and to allow snapshotting of bank and the purging logic on status_cache to
        // kick in
        if slot % set_root_interval == 0 || slot == last_slot {
            // set_root should send a snapshot request
            bank_forks.read().unwrap().prune_program_cache(bank.slot());
            bank_forks
                .write()
                .unwrap()
                .set_root(bank.slot(), &request_sender, None);
            snapshot_request_handler.handle_snapshot_requests(
                false,
                0,
                &mut None,
                &AtomicBool::new(false),
            );
        }
    }

    // Generate a snapshot package for last bank
    let last_bank = bank_forks.read().unwrap().get(last_slot).unwrap();
    let snapshot_config = &snapshot_test_config.snapshot_config;
    let bank_snapshots_dir = &snapshot_config.bank_snapshots_dir;
    let last_bank_snapshot_info = snapshot_utils::get_highest_bank_snapshot_pre(bank_snapshots_dir)
        .expect("no bank snapshots found in path");
    let accounts_package = AccountsPackage::new_for_snapshot(
        AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
        &last_bank,
        &last_bank_snapshot_info,
        &snapshot_config.full_snapshot_archives_dir,
        &snapshot_config.incremental_snapshot_archives_dir,
        last_bank.get_snapshot_storages(None),
        snapshot_config.archive_format,
        snapshot_version,
        None,
    );
    last_bank.force_flush_accounts_cache();
    let accounts_hash =
        last_bank.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);
    solana_runtime::serde_snapshot::reserialize_bank_with_new_accounts_hash(
        accounts_package.bank_snapshot_dir(),
        accounts_package.slot,
        &accounts_hash,
        None,
    );
    let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash.into());
    snapshot_utils::archive_snapshot_package(
        &snapshot_package,
        &snapshot_config.full_snapshot_archives_dir,
        &snapshot_config.incremental_snapshot_archives_dir,
        snapshot_config.maximum_full_snapshot_archives_to_retain,
        snapshot_config.maximum_incremental_snapshot_archives_to_retain,
    )
    .unwrap();

    // Restore bank from snapshot
    let (_tmp_dir, temporary_accounts_dir) = create_tmp_accounts_dir_for_tests();
    let account_paths = &[temporary_accounts_dir];
    let genesis_config = &snapshot_test_config.genesis_config_info.genesis_config;
    restore_from_snapshot(
        snapshot_test_config.bank_forks.clone(),
        last_slot,
        genesis_config,
        account_paths,
    );
}

#[test_case(V1_2_0, Development)]
#[test_case(V1_2_0, Devnet)]
#[test_case(V1_2_0, Testnet)]
#[test_case(V1_2_0, MainnetBeta)]
fn test_bank_forks_snapshot(snapshot_version: SnapshotVersion, cluster_type: ClusterType) {
    // create banks up to slot 4 and create 1 new account in each bank. test that bank 4 snapshots
    // and restores correctly
    run_bank_forks_snapshot_n(
        snapshot_version,
        cluster_type,
        4,
        |bank, mint_keypair| {
            let key1 = Keypair::new().pubkey();
            let tx = system_transaction::transfer(mint_keypair, &key1, 1, bank.last_blockhash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            let key2 = Keypair::new().pubkey();
            let tx = system_transaction::transfer(mint_keypair, &key2, 0, bank.last_blockhash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            bank.freeze();
        },
        1,
    );
}

fn goto_end_of_slot(bank: &Bank) {
    let mut tick_hash = bank.last_blockhash();
    loop {
        tick_hash = hashv(&[tick_hash.as_ref(), &[42]]);
        bank.register_tick(&tick_hash);
        if tick_hash == bank.last_blockhash() {
            bank.freeze();
            return;
        }
    }
}

#[test_case(V1_2_0, Development)]
#[test_case(V1_2_0, Devnet)]
#[test_case(V1_2_0, Testnet)]
#[test_case(V1_2_0, MainnetBeta)]
fn test_concurrent_snapshot_packaging(
    snapshot_version: SnapshotVersion,
    cluster_type: ClusterType,
) {
    solana_logger::setup();
    const MAX_BANK_SNAPSHOTS_TO_RETAIN: usize = 8;

    // Set up snapshotting config
    let snapshot_test_config = SnapshotTestConfig::new(
        snapshot_version,
        cluster_type,
        1,
        1,
        DISABLED_SNAPSHOT_ARCHIVE_INTERVAL,
    );

    let bank_forks = snapshot_test_config.bank_forks.clone();
    let snapshot_config = &snapshot_test_config.snapshot_config;
    let bank_snapshots_dir = &snapshot_config.bank_snapshots_dir;
    let full_snapshot_archives_dir = &snapshot_config.full_snapshot_archives_dir;
    let incremental_snapshot_archives_dir = &snapshot_config.incremental_snapshot_archives_dir;
    let mint_keypair = &snapshot_test_config.genesis_config_info.mint_keypair;
    let genesis_config = &snapshot_test_config.genesis_config_info.genesis_config;

    // Take snapshot of zeroth bank
    let bank0 = bank_forks.read().unwrap().get(0).unwrap();
    let storages = bank0.get_snapshot_storages(None);
    let slot_deltas = bank0.status_cache.read().unwrap().root_slot_deltas();
    snapshot_bank_utils::add_bank_snapshot(
        bank_snapshots_dir,
        &bank0,
        &storages,
        snapshot_version,
        slot_deltas,
    )
    .unwrap();

    // Set up snapshotting channels
    let (real_accounts_package_sender, real_accounts_package_receiver) =
        crossbeam_channel::unbounded();
    let (fake_accounts_package_sender, _fake_accounts_package_receiver) =
        crossbeam_channel::unbounded();

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

    for i in 0..MAX_BANK_SNAPSHOTS_TO_RETAIN + 2 {
        let parent_slot = i as Slot;
        let bank = Bank::new_from_parent(
            bank_forks.read().unwrap().get(parent_slot).unwrap().clone(),
            &Pubkey::default(),
            parent_slot + 1,
        );
        let slot = bank.slot();
        let key1 = Keypair::new().pubkey();
        let tx = system_transaction::transfer(mint_keypair, &key1, 1, genesis_config.hash());
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        bank.squash();

        let accounts_package_sender = {
            if slot == saved_slot {
                // Only send one package on the real accounts package channel so that the
                // packaging service doesn't take forever to run the packaging logic on all
                // MAX_CACHE_ENTRIES later
                &real_accounts_package_sender
            } else {
                &fake_accounts_package_sender
            }
        };

        let snapshot_storages = bank.get_snapshot_storages(None);
        let slot_deltas = bank.status_cache.read().unwrap().root_slot_deltas();
        let bank_snapshot_info = snapshot_bank_utils::add_bank_snapshot(
            bank_snapshots_dir,
            &bank,
            &snapshot_storages,
            snapshot_config.snapshot_version,
            slot_deltas,
        )
        .unwrap();
        let accounts_package = AccountsPackage::new_for_snapshot(
            AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
            &bank,
            &bank_snapshot_info,
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            snapshot_storages,
            snapshot_config.archive_format,
            snapshot_config.snapshot_version,
            None,
        );
        accounts_package_sender.send(accounts_package).unwrap();

        bank_forks.write().unwrap().insert(bank);
        if slot == saved_slot {
            // Find the relevant snapshot storages
            let snapshot_storage_files: HashSet<_> = bank_forks
                .read()
                .unwrap()
                .get(slot)
                .unwrap()
                .get_snapshot_storages(None)
                .into_iter()
                .map(|s| s.get_path())
                .collect();

            // Only save off the files returned by `get_snapshot_storages`. This is because
            // some of the storage entries in the accounts directory may be filtered out by
            // `get_snapshot_storages()` and will not be included in the snapshot. Ultimately,
            // this means copying natively everything in `accounts_dir` to the `saved_accounts_dir`
            // will lead to test failure by mismatch when `saved_accounts_dir` is compared to
            // the unpacked snapshot later in this test's call to `verify_snapshot_archive()`.
            for file in snapshot_storage_files {
                fs::copy(
                    &file,
                    saved_accounts_dir.path().join(file.file_name().unwrap()),
                )
                .unwrap();
            }
            let last_snapshot_path = fs::read_dir(bank_snapshots_dir)
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
            fs_extra::dir::copy(last_snapshot_path, &saved_snapshots_dir, &options).unwrap();

            saved_archive_path = Some(snapshot_utils::build_full_snapshot_archive_path(
                full_snapshot_archives_dir,
                slot,
                // this needs to match the hash value that we reserialize with later. It is complicated, so just use default.
                // This hash value is just used to build the file name. Since this is mocked up test code, it is sufficient to pass default here.
                &SnapshotHash(Hash::default()),
                snapshot_config.archive_format,
            ));
        }
    }

    // Purge all the outdated snapshots, including the ones needed to generate the package
    // currently sitting in the channel
    snapshot_utils::purge_old_bank_snapshots(
        bank_snapshots_dir,
        MAX_BANK_SNAPSHOTS_TO_RETAIN,
        None,
    );

    let mut bank_snapshots = snapshot_utils::get_bank_snapshots_pre(bank_snapshots_dir);
    bank_snapshots.sort_unstable();
    assert!(bank_snapshots
        .into_iter()
        .map(|path| path.slot)
        .eq(3..=MAX_BANK_SNAPSHOTS_TO_RETAIN as u64 + 2));

    // Create a SnapshotPackagerService to create tarballs from all the pending
    // SnapshotPackage's on the channel. By the time this service starts, we have already
    // purged the first two snapshots, which are needed by every snapshot other than
    // the last two snapshots. However, the packaging service should still be able to
    // correctly construct the earlier snapshots because the SnapshotPackage's on the
    // channel hold hard links to these deleted snapshots. We verify this is the case below.
    let exit = Arc::new(AtomicBool::new(false));

    let cluster_info = Arc::new({
        let keypair = Arc::new(Keypair::new());
        let contact_info = ContactInfo::new(
            keypair.pubkey(),
            timestamp(), // wallclock
            0u16,        // shred_version
        );
        ClusterInfo::new(contact_info, keypair, SocketAddrSpace::Unspecified)
    });

    let (snapshot_package_sender, snapshot_package_receiver) = crossbeam_channel::unbounded();
    let snapshot_packager_service = SnapshotPackagerService::new(
        snapshot_package_sender.clone(),
        snapshot_package_receiver,
        None,
        exit.clone(),
        cluster_info,
        snapshot_config.clone(),
        true,
    );

    let _package_receiver = std::thread::Builder::new()
        .name("package-receiver".to_string())
        .spawn({
            let full_snapshot_archives_dir = full_snapshot_archives_dir.clone();
            move || {
                let accounts_package = real_accounts_package_receiver.try_recv().unwrap();
                let accounts_hash = AccountsHash(Hash::default());
                solana_runtime::serde_snapshot::reserialize_bank_with_new_accounts_hash(
                    accounts_package.bank_snapshot_dir(),
                    accounts_package.slot,
                    &accounts_hash,
                    None,
                );
                let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash.into());
                snapshot_package_sender.send(snapshot_package).unwrap();

                // Wait until the package has been archived by SnapshotPackagerService
                while snapshot_utils::get_highest_full_snapshot_archive_slot(
                    &full_snapshot_archives_dir,
                )
                .is_none()
                {
                    std::thread::sleep(Duration::from_millis(100));
                }

                // Shutdown SnapshotPackagerService
                exit.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();

    // Wait for service to finish
    snapshot_packager_service
        .join()
        .expect("SnapshotPackagerService exited with error");

    // Check the archive we cached the state for earlier was generated correctly

    // files were saved off before we reserialized the bank in the hacked up accounts_hash_verifier stand-in.
    solana_runtime::serde_snapshot::reserialize_bank_with_new_accounts_hash(
        snapshot_utils::get_bank_snapshot_dir(&saved_snapshots_dir, saved_slot),
        saved_slot,
        &AccountsHash(Hash::default()),
        None,
    );

    snapshot_utils::verify_snapshot_archive(
        saved_archive_path.unwrap(),
        saved_snapshots_dir.path(),
        snapshot_config.archive_format,
        snapshot_utils::VerifyBank::NonDeterministic,
        saved_slot,
    );
}

#[test_case(V1_2_0, Development)]
#[test_case(V1_2_0, Devnet)]
#[test_case(V1_2_0, Testnet)]
#[test_case(V1_2_0, MainnetBeta)]
fn test_slots_to_snapshot(snapshot_version: SnapshotVersion, cluster_type: ClusterType) {
    solana_logger::setup();
    let num_set_roots = MAX_CACHE_ENTRIES * 2;

    for add_root_interval in &[1, 3, 9] {
        let (snapshot_sender, _snapshot_receiver) = unbounded();
        // Make sure this test never clears bank.slots_since_snapshot
        let snapshot_test_config = SnapshotTestConfig::new(
            snapshot_version,
            cluster_type,
            (*add_root_interval * num_set_roots * 2) as Slot,
            (*add_root_interval * num_set_roots * 2) as Slot,
            DISABLED_SNAPSHOT_ARCHIVE_INTERVAL,
        );
        let bank_forks = snapshot_test_config.bank_forks.clone();
        let bank_forks_r = bank_forks.read().unwrap();
        let mut current_bank = bank_forks_r[0].clone();
        drop(bank_forks_r);
        let request_sender = AbsRequestSender::new(snapshot_sender);
        for _ in 0..num_set_roots {
            for _ in 0..*add_root_interval {
                let new_slot = current_bank.slot() + 1;
                let new_bank = Bank::new_from_parent(current_bank, &Pubkey::default(), new_slot);
                current_bank = bank_forks.write().unwrap().insert(new_bank).clone();
            }
            bank_forks
                .read()
                .unwrap()
                .prune_program_cache(current_bank.slot());
            bank_forks
                .write()
                .unwrap()
                .set_root(current_bank.slot(), &request_sender, None);

            // Since the accounts background services are not runnning, EpochAccountsHash
            // calculation requests will not be handled. To prevent banks from hanging during
            // Bank::freeze() due to waiting for EAH to complete, just set the EAH to Valid.
            let epoch_accounts_hash_manager = &current_bank
                .rc
                .accounts
                .accounts_db
                .epoch_accounts_hash_manager;
            if epoch_accounts_hash_manager
                .try_get_epoch_accounts_hash()
                .is_none()
            {
                epoch_accounts_hash_manager.set_valid(
                    EpochAccountsHash::new(Hash::new_unique()),
                    current_bank.slot(),
                )
            }
        }

        let num_old_slots = num_set_roots * *add_root_interval - MAX_CACHE_ENTRIES + 1;
        let expected_slots_to_snapshot =
            num_old_slots as u64..=num_set_roots as u64 * *add_root_interval as u64;

        let slots_to_snapshot = bank_forks
            .read()
            .unwrap()
            .root_bank()
            .status_cache
            .read()
            .unwrap()
            .roots()
            .iter()
            .cloned()
            .sorted();
        assert!(slots_to_snapshot.into_iter().eq(expected_slots_to_snapshot));
    }
}

#[test_case(V1_2_0, Development)]
#[test_case(V1_2_0, Devnet)]
#[test_case(V1_2_0, Testnet)]
#[test_case(V1_2_0, MainnetBeta)]
fn test_bank_forks_status_cache_snapshot(
    snapshot_version: SnapshotVersion,
    cluster_type: ClusterType,
) {
    // create banks up to slot (MAX_CACHE_ENTRIES * 2) + 1 while transferring 1 lamport into 2 different accounts each time
    // this is done to ensure the AccountStorageEntries keep getting cleaned up as the root moves
    // ahead. Also tests the status_cache purge and status cache snapshotting.
    // Makes sure that the last bank is restored correctly
    let key1 = Keypair::new().pubkey();
    let key2 = Keypair::new().pubkey();
    for set_root_interval in &[1, 4] {
        run_bank_forks_snapshot_n(
            snapshot_version,
            cluster_type,
            (MAX_CACHE_ENTRIES * 2) as u64,
            |bank, mint_keypair| {
                let tx = system_transaction::transfer(
                    mint_keypair,
                    &key1,
                    1,
                    bank.parent().unwrap().last_blockhash(),
                );
                assert_eq!(bank.process_transaction(&tx), Ok(()));
                let tx = system_transaction::transfer(
                    mint_keypair,
                    &key2,
                    1,
                    bank.parent().unwrap().last_blockhash(),
                );
                assert_eq!(bank.process_transaction(&tx), Ok(()));
                goto_end_of_slot(bank);
            },
            *set_root_interval,
        );
    }
}

#[test_case(V1_2_0, Development)]
#[test_case(V1_2_0, Devnet)]
#[test_case(V1_2_0, Testnet)]
#[test_case(V1_2_0, MainnetBeta)]
fn test_bank_forks_incremental_snapshot(
    snapshot_version: SnapshotVersion,
    cluster_type: ClusterType,
) {
    solana_logger::setup();

    const SET_ROOT_INTERVAL: Slot = 2;
    const INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = SET_ROOT_INTERVAL * 2;
    const FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot =
        INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS * 5;
    const LAST_SLOT: Slot = FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS * 2 - 1;

    info!("Running bank forks incremental snapshot test, full snapshot interval: {} slots, incremental snapshot interval: {} slots, last slot: {}, set root interval: {} slots",
              FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS, INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS, LAST_SLOT, SET_ROOT_INTERVAL);

    let snapshot_test_config = SnapshotTestConfig::new(
        snapshot_version,
        cluster_type,
        SET_ROOT_INTERVAL,
        FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
        INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
    );
    trace!("SnapshotTestConfig:\naccounts_dir: {}\nbank_snapshots_dir: {}\nfull_snapshot_archives_dir: {}\nincremental_snapshot_archives_dir: {}",
            snapshot_test_config.accounts_dir.display(), snapshot_test_config.bank_snapshots_dir.path().display(), snapshot_test_config.full_snapshot_archives_dir.path().display(), snapshot_test_config.incremental_snapshot_archives_dir.path().display());

    let bank_forks = snapshot_test_config.bank_forks.clone();
    let mint_keypair = &snapshot_test_config.genesis_config_info.mint_keypair;

    let (accounts_package_sender, _accounts_package_receiver) = crossbeam_channel::unbounded();
    let (snapshot_request_sender, snapshot_request_receiver) = unbounded();
    let request_sender = AbsRequestSender::new(snapshot_request_sender.clone());
    let snapshot_request_handler = SnapshotRequestHandler {
        snapshot_config: snapshot_test_config.snapshot_config.clone(),
        snapshot_request_sender,
        snapshot_request_receiver,
        accounts_package_sender,
    };

    let mut last_full_snapshot_slot = None;
    for slot in 1..=LAST_SLOT {
        // Make a new bank and perform some transactions
        let bank = {
            let parent = bank_forks.read().unwrap().get(slot - 1).unwrap();
            let bank = Bank::new_from_parent(parent, &Pubkey::default(), slot);

            let key = solana_sdk::pubkey::new_rand();
            let tx = system_transaction::transfer(mint_keypair, &key, 1, bank.last_blockhash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            let key = solana_sdk::pubkey::new_rand();
            let tx = system_transaction::transfer(mint_keypair, &key, 0, bank.last_blockhash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            while !bank.is_complete() {
                bank.register_tick(&Hash::new_unique());
            }

            bank_forks.write().unwrap().insert(bank)
        };

        // Set root to make sure we don't end up with too many account storage entries
        // and to allow snapshotting of bank and the purging logic on status_cache to
        // kick in
        if slot % SET_ROOT_INTERVAL == 0 {
            // set_root sends a snapshot request
            bank_forks.read().unwrap().prune_program_cache(bank.slot());
            bank_forks
                .write()
                .unwrap()
                .set_root(bank.slot(), &request_sender, None);
            snapshot_request_handler.handle_snapshot_requests(
                false,
                0,
                &mut last_full_snapshot_slot,
                &AtomicBool::new(false),
            );
        }

        // Since AccountsBackgroundService isn't running, manually make a full snapshot archive
        // at the right interval
        if snapshot_utils::should_take_full_snapshot(slot, FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS) {
            make_full_snapshot_archive(&bank, &snapshot_test_config.snapshot_config).unwrap();
        }
        // Similarly, make an incremental snapshot archive at the right interval, but only if
        // there's been at least one full snapshot first, and a full snapshot wasn't already
        // taken at this slot.
        //
        // Then, after making an incremental snapshot, restore the bank and verify it is correct
        else if snapshot_utils::should_take_incremental_snapshot(
            slot,
            INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
            last_full_snapshot_slot,
        ) && slot != last_full_snapshot_slot.unwrap()
        {
            make_incremental_snapshot_archive(
                &bank,
                last_full_snapshot_slot.unwrap(),
                &snapshot_test_config.snapshot_config,
            )
            .unwrap();

            // Accounts directory needs to be separate from the active accounts directory
            // so that dropping append vecs in the active accounts directory doesn't
            // delete the unpacked appendvecs in the snapshot
            let (_tmp_dir, temporary_accounts_dir) = create_tmp_accounts_dir_for_tests();
            restore_from_snapshots_and_check_banks_are_equal(
                &bank,
                &snapshot_test_config.snapshot_config,
                temporary_accounts_dir,
                &snapshot_test_config.genesis_config_info.genesis_config,
            )
            .unwrap();
        }
    }
}

fn make_full_snapshot_archive(
    bank: &Bank,
    snapshot_config: &SnapshotConfig,
) -> snapshot_utils::Result<()> {
    let slot = bank.slot();
    info!("Making full snapshot archive from bank at slot: {}", slot);
    bank.force_flush_accounts_cache();
    bank.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);
    let bank_snapshot_info =
        snapshot_utils::get_bank_snapshots_pre(&snapshot_config.bank_snapshots_dir)
            .into_iter()
            .find(|elem| elem.slot == slot)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Other,
                    "did not find bank snapshot with this path",
                )
            })?;
    snapshot_bank_utils::package_and_archive_full_snapshot(
        bank,
        &bank_snapshot_info,
        &snapshot_config.full_snapshot_archives_dir,
        &snapshot_config.incremental_snapshot_archives_dir,
        bank.get_snapshot_storages(None),
        snapshot_config.archive_format,
        snapshot_config.snapshot_version,
        snapshot_config.maximum_full_snapshot_archives_to_retain,
        snapshot_config.maximum_incremental_snapshot_archives_to_retain,
    )?;

    Ok(())
}

fn make_incremental_snapshot_archive(
    bank: &Bank,
    incremental_snapshot_base_slot: Slot,
    snapshot_config: &SnapshotConfig,
) -> snapshot_utils::Result<()> {
    let slot = bank.slot();
    info!(
        "Making incremental snapshot archive from bank at slot: {}, and base slot: {}",
        slot, incremental_snapshot_base_slot,
    );
    bank.force_flush_accounts_cache();
    bank.update_incremental_accounts_hash(incremental_snapshot_base_slot);
    let bank_snapshot_info =
        snapshot_utils::get_bank_snapshots_pre(&snapshot_config.bank_snapshots_dir)
            .into_iter()
            .find(|elem| elem.slot == slot)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Other,
                    "did not find bank snapshot with this path",
                )
            })?;
    let storages = bank.get_snapshot_storages(Some(incremental_snapshot_base_slot));
    snapshot_bank_utils::package_and_archive_incremental_snapshot(
        bank,
        incremental_snapshot_base_slot,
        &bank_snapshot_info,
        &snapshot_config.full_snapshot_archives_dir,
        &snapshot_config.incremental_snapshot_archives_dir,
        storages,
        snapshot_config.archive_format,
        snapshot_config.snapshot_version,
        snapshot_config.maximum_full_snapshot_archives_to_retain,
        snapshot_config.maximum_incremental_snapshot_archives_to_retain,
    )?;

    Ok(())
}

fn restore_from_snapshots_and_check_banks_are_equal(
    bank: &Bank,
    snapshot_config: &SnapshotConfig,
    accounts_dir: PathBuf,
    genesis_config: &GenesisConfig,
) -> snapshot_utils::Result<()> {
    let (deserialized_bank, ..) = snapshot_bank_utils::bank_from_latest_snapshot_archives(
        &snapshot_config.bank_snapshots_dir,
        &snapshot_config.full_snapshot_archives_dir,
        &snapshot_config.incremental_snapshot_archives_dir,
        &[accounts_dir],
        genesis_config,
        &RuntimeConfig::default(),
        None,
        None,
        AccountSecondaryIndexes::default(),
        None,
        accounts_db::AccountShrinkThreshold::default(),
        false,
        false,
        false,
        false,
        Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        Arc::default(),
    )?;
    deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();

    assert_eq!(bank, &deserialized_bank);

    Ok(())
}

/// Spin up the background services fully and test taking snapshots
#[test_case(V1_2_0, Development)]
#[test_case(V1_2_0, Devnet)]
#[test_case(V1_2_0, Testnet)]
#[test_case(V1_2_0, MainnetBeta)]
fn test_snapshots_with_background_services(
    snapshot_version: SnapshotVersion,
    cluster_type: ClusterType,
) {
    solana_logger::setup();

    const SET_ROOT_INTERVAL_SLOTS: Slot = 2;
    const BANK_SNAPSHOT_INTERVAL_SLOTS: Slot = SET_ROOT_INTERVAL_SLOTS * 2;
    const INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = BANK_SNAPSHOT_INTERVAL_SLOTS * 3;
    const FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot =
        INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS * 5;
    const LAST_SLOT: Slot =
        FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS * 3 + INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS * 2;

    // Maximum amount of time to wait for each snapshot archive to be created.
    // This should be enough time, but if it times-out in CI, try increasing it.
    const MAX_WAIT_DURATION: Duration = Duration::from_secs(10);

    info!("Running snapshots with background services test...");
    trace!(
        "Test configuration parameters:\
        \n\tfull snapshot archive interval: {} slots\
        \n\tincremental snapshot archive interval: {} slots\
        \n\tbank snapshot interval: {} slots\
        \n\tset root interval: {} slots\
        \n\tlast slot: {}",
        FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
        INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
        BANK_SNAPSHOT_INTERVAL_SLOTS,
        SET_ROOT_INTERVAL_SLOTS,
        LAST_SLOT
    );

    let snapshot_test_config = SnapshotTestConfig::new(
        snapshot_version,
        cluster_type,
        BANK_SNAPSHOT_INTERVAL_SLOTS,
        FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
        INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS,
    );

    let node_keypair = Arc::new(Keypair::new());
    let cluster_info = Arc::new(ClusterInfo::new(
        ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
        node_keypair,
        SocketAddrSpace::Unspecified,
    ));

    let (pruned_banks_sender, pruned_banks_receiver) = unbounded();
    let (snapshot_request_sender, snapshot_request_receiver) = unbounded();
    let (accounts_package_sender, accounts_package_receiver) = unbounded();
    let (snapshot_package_sender, snapshot_package_receiver) = unbounded();

    let bank_forks = snapshot_test_config.bank_forks.clone();
    bank_forks
        .read()
        .unwrap()
        .root_bank()
        .rc
        .accounts
        .accounts_db
        .enable_bank_drop_callback();
    let callback = SendDroppedBankCallback::new(pruned_banks_sender);
    for bank in bank_forks.read().unwrap().banks().values() {
        bank.set_callback(Some(Box::new(callback.clone())));
    }

    let abs_request_sender = AbsRequestSender::new(snapshot_request_sender.clone());
    let snapshot_request_handler = SnapshotRequestHandler {
        snapshot_config: snapshot_test_config.snapshot_config.clone(),
        snapshot_request_sender,
        snapshot_request_receiver,
        accounts_package_sender: accounts_package_sender.clone(),
    };
    let pruned_banks_request_handler = PrunedBanksRequestHandler {
        pruned_banks_receiver,
    };
    let abs_request_handler = AbsRequestHandlers {
        snapshot_request_handler,
        pruned_banks_request_handler,
    };

    let exit = Arc::new(AtomicBool::new(false));
    let snapshot_packager_service = SnapshotPackagerService::new(
        snapshot_package_sender.clone(),
        snapshot_package_receiver,
        None,
        exit.clone(),
        cluster_info.clone(),
        snapshot_test_config.snapshot_config.clone(),
        false,
    );

    let accounts_hash_verifier = AccountsHashVerifier::new(
        accounts_package_sender,
        accounts_package_receiver,
        Some(snapshot_package_sender),
        exit.clone(),
        cluster_info,
        None,
        snapshot_test_config.snapshot_config.clone(),
    );

    let accounts_background_service = AccountsBackgroundService::new(
        bank_forks.clone(),
        exit.clone(),
        abs_request_handler,
        false,
        None,
    );

    let mut last_full_snapshot_slot = None;
    let mut last_incremental_snapshot_slot = None;
    let mint_keypair = &snapshot_test_config.genesis_config_info.mint_keypair;
    for slot in 1..=LAST_SLOT {
        // Make a new bank and process some transactions
        {
            let bank = Bank::new_from_parent(
                bank_forks.read().unwrap().get(slot - 1).unwrap(),
                &Pubkey::default(),
                slot,
            );

            let key = solana_sdk::pubkey::new_rand();
            let tx = system_transaction::transfer(mint_keypair, &key, 1, bank.last_blockhash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            let key = solana_sdk::pubkey::new_rand();
            let tx = system_transaction::transfer(mint_keypair, &key, 0, bank.last_blockhash());
            assert_eq!(bank.process_transaction(&tx), Ok(()));

            while !bank.is_complete() {
                bank.register_tick(&Hash::new_unique());
            }

            bank_forks.write().unwrap().insert(bank);
        }

        // Call `BankForks::set_root()` to cause snapshots to be taken
        if slot % SET_ROOT_INTERVAL_SLOTS == 0 {
            bank_forks
                .write()
                .unwrap()
                .set_root(slot, &abs_request_sender, None);
        }

        // If a snapshot should be taken this slot, wait for it to complete
        if slot % FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS == 0 {
            let timer = Instant::now();
            while snapshot_utils::get_highest_full_snapshot_archive_slot(
                &snapshot_test_config
                    .snapshot_config
                    .full_snapshot_archives_dir,
            ) != Some(slot)
            {
                assert!(
                    timer.elapsed() < MAX_WAIT_DURATION,
                    "Waiting for full snapshot {slot} exceeded the {MAX_WAIT_DURATION:?} maximum wait duration!",
                );
                std::thread::sleep(Duration::from_secs(1));
            }
            last_full_snapshot_slot = Some(slot);
        } else if slot % INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS == 0
            && last_full_snapshot_slot.is_some()
        {
            let timer = Instant::now();
            while snapshot_utils::get_highest_incremental_snapshot_archive_slot(
                &snapshot_test_config
                    .snapshot_config
                    .incremental_snapshot_archives_dir,
                last_full_snapshot_slot.unwrap(),
            ) != Some(slot)
            {
                assert!(
                    timer.elapsed() < MAX_WAIT_DURATION,
                    "Waiting for incremental snapshot {slot} exceeded the {MAX_WAIT_DURATION:?} maximum wait duration!",
                );
                std::thread::sleep(Duration::from_secs(1));
            }
            last_incremental_snapshot_slot = Some(slot);
        }
    }

    // Load the snapshot and ensure it matches what's in BankForks
    let (_tmp_dir, temporary_accounts_dir) = create_tmp_accounts_dir_for_tests();
    let (deserialized_bank, ..) = snapshot_bank_utils::bank_from_latest_snapshot_archives(
        &snapshot_test_config.snapshot_config.bank_snapshots_dir,
        &snapshot_test_config
            .snapshot_config
            .full_snapshot_archives_dir,
        &snapshot_test_config
            .snapshot_config
            .incremental_snapshot_archives_dir,
        &[temporary_accounts_dir],
        &snapshot_test_config.genesis_config_info.genesis_config,
        &RuntimeConfig::default(),
        None,
        None,
        AccountSecondaryIndexes::default(),
        None,
        accounts_db::AccountShrinkThreshold::default(),
        false,
        false,
        false,
        false,
        Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        exit.clone(),
    )
    .unwrap();
    deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();

    assert_eq!(
        deserialized_bank.slot(),
        last_incremental_snapshot_slot.unwrap()
    );
    assert_eq!(
        &deserialized_bank,
        bank_forks
            .read()
            .unwrap()
            .get(deserialized_bank.slot())
            .unwrap()
            .as_ref()
    );

    // Stop the background services, ignore any errors
    info!("Shutting down background services...");
    exit.store(true, Ordering::Relaxed);
    _ = accounts_background_service.join();
    _ = accounts_hash_verifier.join();
    _ = snapshot_packager_service.join();
}
