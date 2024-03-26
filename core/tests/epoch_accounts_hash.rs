// REMOVE once https://github.com/rust-lang/rust-clippy/issues/11153 is fixed
#![allow(clippy::items_after_test_module)]

use {
    crate::snapshot_utils::create_tmp_accounts_dir_for_tests,
    log::*,
    solana_accounts_db::{
        accounts_db::{AccountShrinkThreshold, CalcAccountsHashDataSource},
        accounts_hash::CalcAccountsHashConfig,
        accounts_index::AccountSecondaryIndexes,
        epoch_accounts_hash::EpochAccountsHash,
    },
    solana_core::{
        accounts_hash_verifier::AccountsHashVerifier,
        snapshot_packager_service::SnapshotPackagerService,
    },
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_program_runtime::runtime_config::RuntimeConfig,
    solana_runtime::{
        accounts_background_service::{
            AbsRequestHandlers, AbsRequestSender, AccountsBackgroundService, DroppedSlotsReceiver,
            PrunedBanksRequestHandler, SnapshotRequestHandler,
        },
        bank::{epoch_accounts_hash_utils, Bank},
        bank_forks::BankForks,
        genesis_utils::{self, GenesisConfigInfo},
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_bank_utils,
        snapshot_config::SnapshotConfig,
        snapshot_utils,
    },
    solana_sdk::{
        clock::Slot,
        epoch_schedule::EpochSchedule,
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_transaction,
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        mem::ManuallyDrop,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        time::Duration,
    },
    tempfile::TempDir,
    test_case::test_case,
};

struct TestEnvironment {
    /// NOTE: The fields are arranged to ensure they are dropped in the correct order.
    /// - BankForks must be dropped before BackgroundServices
    /// - BackgroundServices must be dropped before the TempDirs
    /// - SnapshotConfig should be dropped before the TempDirs
    bank_forks: Arc<RwLock<BankForks>>,
    background_services: BackgroundServices,
    genesis_config_info: GenesisConfigInfo,
    snapshot_config: SnapshotConfig,
    _bank_snapshots_dir: TempDir,
    _full_snapshot_archives_dir: TempDir,
    _incremental_snapshot_archives_dir: TempDir,
}

impl TestEnvironment {
    /// A small, round number to make the tests run quickly, and easy to debug
    const SLOTS_PER_EPOCH: u64 = 400;

    /// A small, round number to ensure accounts packages are sent to the background services
    const ACCOUNTS_HASH_INTERVAL: u64 = 40;

    #[must_use]
    fn new() -> TestEnvironment {
        Self::_new(SnapshotConfig::new_load_only())
    }

    #[must_use]
    fn new_with_snapshots(
        full_snapshot_archive_interval_slots: Slot,
        incremental_snapshot_archive_interval_slots: Slot,
    ) -> TestEnvironment {
        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots,
            incremental_snapshot_archive_interval_slots,
            ..SnapshotConfig::default()
        };
        Self::_new(snapshot_config)
    }

    #[must_use]
    fn _new(snapshot_config: SnapshotConfig) -> TestEnvironment {
        const MINT_LAMPORTS: u64 = 100_000 * LAMPORTS_PER_SOL;
        const STAKE_LAMPORTS: u64 = 100 * LAMPORTS_PER_SOL;
        let bank_snapshots_dir = TempDir::new().unwrap();
        let full_snapshot_archives_dir = TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = TempDir::new().unwrap();
        let mut genesis_config_info = genesis_utils::create_genesis_config_with_leader(
            MINT_LAMPORTS,
            &Pubkey::new_unique(),
            STAKE_LAMPORTS,
        );
        genesis_config_info.genesis_config.epoch_schedule =
            EpochSchedule::custom(Self::SLOTS_PER_EPOCH, Self::SLOTS_PER_EPOCH, false);
        let snapshot_config = SnapshotConfig {
            full_snapshot_archives_dir: full_snapshot_archives_dir.path().to_path_buf(),
            incremental_snapshot_archives_dir: incremental_snapshot_archives_dir
                .path()
                .to_path_buf(),
            bank_snapshots_dir: bank_snapshots_dir.path().to_path_buf(),
            ..snapshot_config
        };

        let bank_forks =
            BankForks::new_rw_arc(Bank::new_for_tests(&genesis_config_info.genesis_config));
        bank_forks
            .write()
            .unwrap()
            .set_snapshot_config(Some(snapshot_config.clone()));
        bank_forks
            .write()
            .unwrap()
            .set_accounts_hash_interval_slots(Self::ACCOUNTS_HASH_INTERVAL);

        let exit = Arc::new(AtomicBool::new(false));
        let node_id = Arc::new(Keypair::new());
        let cluster_info = Arc::new(ClusterInfo::new(
            ContactInfo::new_localhost(&node_id.pubkey(), timestamp()),
            Arc::clone(&node_id),
            SocketAddrSpace::Unspecified,
        ));

        let pruned_banks_receiver =
            AccountsBackgroundService::setup_bank_drop_callback(Arc::clone(&bank_forks));
        let background_services = BackgroundServices::new(
            Arc::clone(&exit),
            Arc::clone(&cluster_info),
            &snapshot_config,
            pruned_banks_receiver,
            Arc::clone(&bank_forks),
        );
        let bank = bank_forks.read().unwrap().working_bank();
        assert!(epoch_accounts_hash_utils::is_enabled_this_epoch(&bank));

        bank.set_startup_verification_complete();

        TestEnvironment {
            bank_forks,
            genesis_config_info,
            _bank_snapshots_dir: bank_snapshots_dir,
            _full_snapshot_archives_dir: full_snapshot_archives_dir,
            _incremental_snapshot_archives_dir: incremental_snapshot_archives_dir,
            snapshot_config,
            background_services,
        }
    }
}

/// In order to shut down the background services correctly, each service's thread must be joined.
/// However, since `.join()` takes a `self` and `drop()` takes a `&mut self`, it means a "normal"
/// implementation of drop will not work.  Instead, we must handle drop ourselves.
struct BackgroundServices {
    exit: Arc<AtomicBool>,
    accounts_background_service: ManuallyDrop<AccountsBackgroundService>,
    accounts_background_request_sender: AbsRequestSender,
    accounts_hash_verifier: ManuallyDrop<AccountsHashVerifier>,
    snapshot_packager_service: ManuallyDrop<SnapshotPackagerService>,
}

impl BackgroundServices {
    #[must_use]
    fn new(
        exit: Arc<AtomicBool>,
        cluster_info: Arc<ClusterInfo>,
        snapshot_config: &SnapshotConfig,
        pruned_banks_receiver: DroppedSlotsReceiver,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> Self {
        info!("Starting background services...");

        let (snapshot_package_sender, snapshot_package_receiver) = crossbeam_channel::unbounded();
        let snapshot_packager_service = SnapshotPackagerService::new(
            snapshot_package_sender.clone(),
            snapshot_package_receiver,
            None,
            exit.clone(),
            cluster_info.clone(),
            snapshot_config.clone(),
            false,
        );

        let (accounts_package_sender, accounts_package_receiver) = crossbeam_channel::unbounded();
        let accounts_hash_verifier = AccountsHashVerifier::new(
            accounts_package_sender.clone(),
            accounts_package_receiver,
            Some(snapshot_package_sender),
            exit.clone(),
            snapshot_config.clone(),
        );

        let (snapshot_request_sender, snapshot_request_receiver) = crossbeam_channel::unbounded();
        let accounts_background_request_sender =
            AbsRequestSender::new(snapshot_request_sender.clone());
        let snapshot_request_handler = SnapshotRequestHandler {
            snapshot_config: snapshot_config.clone(),
            snapshot_request_sender,
            snapshot_request_receiver,
            accounts_package_sender,
        };
        let pruned_banks_request_handler = PrunedBanksRequestHandler {
            pruned_banks_receiver,
        };
        let accounts_background_service = AccountsBackgroundService::new(
            bank_forks,
            exit.clone(),
            AbsRequestHandlers {
                snapshot_request_handler,
                pruned_banks_request_handler,
            },
            false,
            None,
        );

        info!("Starting background services... DONE");
        Self {
            exit,
            accounts_background_service: ManuallyDrop::new(accounts_background_service),
            accounts_background_request_sender,
            accounts_hash_verifier: ManuallyDrop::new(accounts_hash_verifier),
            snapshot_packager_service: ManuallyDrop::new(snapshot_packager_service),
        }
    }
}

impl Drop for BackgroundServices {
    fn drop(&mut self) {
        info!("Stopping background services...");
        self.exit.store(true, Ordering::Relaxed);

        // Join the background threads, and ignore any errors.
        // SAFETY: We do not use any of the `ManuallyDrop` fields again, so `.take()` is OK here.
        _ = unsafe { ManuallyDrop::take(&mut self.accounts_background_service) }.join();
        _ = unsafe { ManuallyDrop::take(&mut self.accounts_hash_verifier) }.join();
        _ = unsafe { ManuallyDrop::take(&mut self.snapshot_packager_service) }.join();

        info!("Stopping background services... DONE");
    }
}

/// Ensure that EAHs are requested, calculated, and awaited correctly.
/// Test both with and without snapshots to make sure they don't interfere with EAH.
#[test_case(TestEnvironment::new()                      ; "without snapshots")]
#[test_case(TestEnvironment::new_with_snapshots(80, 40) ; "with snapshots")]
fn test_epoch_accounts_hash_basic(test_environment: TestEnvironment) {
    solana_logger::setup();

    const NUM_EPOCHS_TO_TEST: u64 = 2;
    const SET_ROOT_INTERVAL: Slot = 3;

    let bank_forks = test_environment.bank_forks.clone();

    let mut expected_epoch_accounts_hash = None;

    let slots_per_epoch = test_environment
        .genesis_config_info
        .genesis_config
        .epoch_schedule
        .slots_per_epoch;
    for _ in 0..slots_per_epoch.checked_mul(NUM_EPOCHS_TO_TEST).unwrap() {
        let bank = {
            let parent = bank_forks.read().unwrap().working_bank();
            let slot = parent.slot().checked_add(1).unwrap();
            let bank = bank_forks.write().unwrap().insert(Bank::new_from_parent(
                parent,
                &Pubkey::default(),
                slot,
            ));

            let transaction = system_transaction::transfer(
                &test_environment.genesis_config_info.mint_keypair,
                &Pubkey::new_unique(),
                1,
                bank.last_blockhash(),
            );
            bank.process_transaction(&transaction).unwrap();
            bank.fill_bank_with_ticks_for_tests();

            bank
        };
        trace!("new bank {}", bank.slot());

        // Set roots so that ABS requests are sent (this is what requests EAH calculations)
        if bank.slot().checked_rem(SET_ROOT_INTERVAL).unwrap() == 0 {
            trace!("rooting bank {}", bank.slot());
            bank_forks.read().unwrap().prune_program_cache(bank.slot());
            bank_forks.write().unwrap().set_root(
                bank.slot(),
                &test_environment
                    .background_services
                    .accounts_background_request_sender,
                None,
            );
        }

        // To ensure EAH calculations are correct, calculate the accounts hash here, in-band.
        // This will be the expected EAH that gets saved into the "stop" bank.
        if bank.slot() == epoch_accounts_hash_utils::calculation_start(&bank) {
            bank.freeze();
            let (accounts_hash, _) = bank
                .rc
                .accounts
                .accounts_db
                .calculate_accounts_hash_from_index(
                    bank.slot(),
                    &CalcAccountsHashConfig {
                        use_bg_thread_pool: false,
                        check_hash: false,
                        ancestors: Some(&bank.ancestors),
                        epoch_schedule: bank.epoch_schedule(),
                        rent_collector: bank.rent_collector(),
                        store_detailed_debug_info_on_failure: false,
                    },
                )
                .unwrap();
            expected_epoch_accounts_hash = Some(EpochAccountsHash::from(accounts_hash));
            debug!(
                "slot {}, expected epoch accounts hash: {:?}",
                bank.slot(),
                expected_epoch_accounts_hash
            );
        }

        // Test: Ensure that the "stop" bank has the correct EAH
        if bank.slot() == epoch_accounts_hash_utils::calculation_stop(&bank) {
            // Sometimes AHV does not get scheduled to run, which causes the test to fail
            // spuriously.  Sleep a bit here to ensure AHV gets a chance to run.
            std::thread::sleep(Duration::from_secs(1));
            let actual_epoch_accounts_hash = bank.epoch_accounts_hash();
            debug!(
                "slot {},   actual epoch accounts hash: {:?}",
                bank.slot(),
                actual_epoch_accounts_hash,
            );
            assert_eq!(expected_epoch_accounts_hash, actual_epoch_accounts_hash);
        }

        // Give the background services a chance to run
        std::thread::yield_now();
    }
}

/// Ensure that snapshots always have the expected EAH
///
/// Generate snapshots:
/// - Before EAH start
/// - After EAH start but before EAH stop
/// - After EAH stop
///
/// In Epoch 0, this will correspond to all three EAH states (invalid, in-flight, and valid). In
/// Epoch 1, this will correspond to a normal running cluster, where EAH will only be either
/// in-flight or valid.
#[test]
fn test_snapshots_have_expected_epoch_accounts_hash() {
    solana_logger::setup();

    const NUM_EPOCHS_TO_TEST: u64 = 2;

    // Since slots-per-epoch is 400, EAH start will be slots 100 and 500, and EAH stop will be slots
    // 300 and 700.  Pick a full snapshot interval that triggers in the three scenarios outlined in
    // the test's description.
    const FULL_SNAPSHOT_INTERVAL: Slot = 80;

    let test_environment =
        TestEnvironment::new_with_snapshots(FULL_SNAPSHOT_INTERVAL, FULL_SNAPSHOT_INTERVAL);
    let bank_forks = test_environment.bank_forks.clone();

    let slots_per_epoch = test_environment
        .genesis_config_info
        .genesis_config
        .epoch_schedule
        .slots_per_epoch;
    for _ in 0..slots_per_epoch.checked_mul(NUM_EPOCHS_TO_TEST).unwrap() {
        let bank = {
            let parent = bank_forks.read().unwrap().working_bank();
            let slot = parent.slot().checked_add(1).unwrap();
            let bank = bank_forks.write().unwrap().insert(Bank::new_from_parent(
                parent,
                &Pubkey::default(),
                slot,
            ));

            let transaction = system_transaction::transfer(
                &test_environment.genesis_config_info.mint_keypair,
                &Pubkey::new_unique(),
                1,
                bank.last_blockhash(),
            );
            bank.process_transaction(&transaction).unwrap();
            bank.fill_bank_with_ticks_for_tests();

            bank
        };
        trace!("new bank {}", bank.slot());

        // Root every bank.  This is what a normal validator does as well.
        // `set_root()` is also what requests snapshots and EAH calculations.
        bank_forks.read().unwrap().prune_program_cache(bank.slot());
        bank_forks.write().unwrap().set_root(
            bank.slot(),
            &test_environment
                .background_services
                .accounts_background_request_sender,
            None,
        );

        // After submitting an EAH calculation request, wait until it gets handled by ABS so that
        // subsequent snapshot requests are not swallowed.
        if bank.slot() == epoch_accounts_hash_utils::calculation_start(&bank) {
            while bank.epoch_accounts_hash().is_none() {
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        // After submitting a snapshot request...
        // - Wait until the snapshot archive has been generated
        // - Deserialize the bank from the snapshot archive
        // - Ensure the EAHs match
        if bank.slot() % FULL_SNAPSHOT_INTERVAL == 0 {
            let snapshot_config = &test_environment.snapshot_config;
            let full_snapshot_archive_info = loop {
                if let Some(full_snapshot_archive_info) =
                    snapshot_utils::get_highest_full_snapshot_archive_info(
                        &snapshot_config.full_snapshot_archives_dir,
                    )
                {
                    if full_snapshot_archive_info.slot() == bank.slot() {
                        break full_snapshot_archive_info;
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            };

            let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
            let deserialized_bank = snapshot_bank_utils::bank_from_snapshot_archives(
                &[accounts_dir],
                &snapshot_config.bank_snapshots_dir,
                &full_snapshot_archive_info,
                None,
                &test_environment.genesis_config_info.genesis_config,
                &RuntimeConfig::default(),
                None,
                None,
                AccountSecondaryIndexes::default(),
                None,
                AccountShrinkThreshold::default(),
                true,
                true,
                false,
                true,
                None,
                None,
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
            .0;
            deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();

            assert_eq!(&deserialized_bank, bank.as_ref());
            assert_eq!(
                deserialized_bank.epoch_accounts_hash(),
                bank.get_epoch_accounts_hash_to_serialize(),
            );
        }

        // Give the background services a chance to run
        std::thread::yield_now();
    }
}

/// Ensure that EAH works well with ABS's snapshot request handling
///
/// Given the scenario where two banks are rooted back-to-back, where the first bank sends an
/// EAH request and the second bank sends a snapshot request, both requests should be handled.
#[test]
fn test_background_services_request_handling_for_epoch_accounts_hash() {
    solana_logger::setup();

    const NUM_EPOCHS_TO_TEST: u64 = 2;
    const FULL_SNAPSHOT_INTERVAL: Slot = 80;

    let test_environment =
        TestEnvironment::new_with_snapshots(FULL_SNAPSHOT_INTERVAL, FULL_SNAPSHOT_INTERVAL);
    let bank_forks = test_environment.bank_forks.clone();
    let snapshot_config = &test_environment.snapshot_config;

    let slots_per_epoch = test_environment
        .genesis_config_info
        .genesis_config
        .epoch_schedule
        .slots_per_epoch;
    for _ in 0..slots_per_epoch.checked_mul(NUM_EPOCHS_TO_TEST).unwrap() {
        let bank = {
            let parent = bank_forks.read().unwrap().working_bank();
            let slot = parent.slot().checked_add(1).unwrap();
            let bank = bank_forks.write().unwrap().insert(Bank::new_from_parent(
                parent,
                &Pubkey::default(),
                slot,
            ));

            let transaction = system_transaction::transfer(
                &test_environment.genesis_config_info.mint_keypair,
                &Pubkey::new_unique(),
                1,
                bank.last_blockhash(),
            );
            bank.process_transaction(&transaction).unwrap();
            bank.fill_bank_with_ticks_for_tests();

            bank
        };
        debug!("new bank {}", bank.slot());

        // Based on the EAH start and snapshot interval, pick a slot to mass-root all the banks in
        // this range such that an EAH request will be sent and also a snapshot request.
        let eah_start_slot = epoch_accounts_hash_utils::calculation_start(&bank);
        let set_root_slot = eah_start_slot.next_multiple_of(FULL_SNAPSHOT_INTERVAL);

        if bank.block_height() == set_root_slot {
            info!("Calling set_root() on bank {}...", bank.slot());
            bank_forks.read().unwrap().prune_program_cache(bank.slot());
            bank_forks.write().unwrap().set_root(
                bank.slot(),
                &test_environment
                    .background_services
                    .accounts_background_request_sender,
                None,
            );
            info!("Calling set_root() on bank {}... DONE", bank.slot());

            // wait until eah is valid
            info!("Calculating epoch accounts hash...");
            while bank.epoch_accounts_hash().is_none() {
                trace!("waiting for epoch accounts hash...");
                std::thread::sleep(Duration::from_secs(1));
            }
            info!("Calculating epoch accounts hash... DONE");

            // wait until FSS is made
            info!("Taking full snapshot...");
            while snapshot_utils::get_highest_full_snapshot_archive_slot(
                &snapshot_config.full_snapshot_archives_dir,
            ) != Some(bank.slot())
            {
                trace!("waiting for full snapshot...");
                std::thread::sleep(Duration::from_secs(1));
            }
            info!("Taking full snapshot... DONE");
        }

        // Give the background services a chance to run
        std::thread::yield_now();
    }
}

/// Ensure that warping and EAH play nicely together
///
/// Ledger-tool allows warping when creating a snapshot, so it is important that EAH does not break
/// that use-case.
#[test]
fn test_epoch_accounts_hash_and_warping() {
    solana_logger::setup();

    let test_environment = TestEnvironment::new();
    let bank_forks = test_environment.bank_forks.clone();
    let bank = bank_forks.read().unwrap().working_bank();
    let epoch_schedule = test_environment
        .genesis_config_info
        .genesis_config
        .epoch_schedule;

    // Ensure warping past the EAH stop slot is OK
    info!("Warping past EAH stop slot...");
    let eah_stop_offset = epoch_accounts_hash_utils::calculation_offset_stop(&bank);
    let eah_stop_slot_in_next_epoch =
        epoch_schedule.get_first_slot_in_epoch(bank.epoch() + 1) + eah_stop_offset;
    // have to set root here so that we can flush the write cache
    bank_forks.read().unwrap().prune_program_cache(bank.slot());
    bank_forks.write().unwrap().set_root(
        bank.slot(),
        &test_environment
            .background_services
            .accounts_background_request_sender,
        None,
    );
    // flush the write cache so warping can calculate the accounts hash from storages
    bank.force_flush_accounts_cache();
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(Bank::warp_from_parent(
            bank,
            &Pubkey::default(),
            eah_stop_slot_in_next_epoch,
            CalcAccountsHashDataSource::Storages,
        ))
        .clone_without_scheduler();
    let slot = bank.slot().checked_add(1).unwrap();
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(Bank::new_from_parent(bank, &Pubkey::default(), slot))
        .clone_without_scheduler();
    bank_forks.read().unwrap().prune_program_cache(bank.slot());
    bank_forks.write().unwrap().set_root(
        bank.slot(),
        &test_environment
            .background_services
            .accounts_background_request_sender,
        None,
    );
    info!("Waiting for epoch accounts hash...");
    _ = bank
        .rc
        .accounts
        .accounts_db
        .epoch_accounts_hash_manager
        .wait_get_epoch_accounts_hash();
    info!("Waiting for epoch accounts hash... DONE");

    // Ensure warping past the EAH start slot is OK
    info!("Warping past EAH start slot...");
    let eah_start_offset = epoch_accounts_hash_utils::calculation_offset_start(&bank);
    let eah_start_slot_in_next_epoch =
        epoch_schedule.get_first_slot_in_epoch(bank.epoch() + 1) + eah_start_offset;
    // flush the write cache so warping can calculate the accounts hash from storages
    bank.force_flush_accounts_cache();
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(Bank::warp_from_parent(
            bank,
            &Pubkey::default(),
            eah_start_slot_in_next_epoch,
            CalcAccountsHashDataSource::Storages,
        ))
        .clone_without_scheduler();
    let slot = bank.slot().checked_add(1).unwrap();
    let bank = bank_forks
        .write()
        .unwrap()
        .insert(Bank::new_from_parent(bank, &Pubkey::default(), slot))
        .clone_without_scheduler();
    bank_forks.read().unwrap().prune_program_cache(bank.slot());
    bank_forks.write().unwrap().set_root(
        bank.slot(),
        &test_environment
            .background_services
            .accounts_background_request_sender,
        None,
    );
    info!("Waiting for epoch accounts hash...");
    _ = bank
        .rc
        .accounts
        .accounts_db
        .epoch_accounts_hash_manager
        .wait_get_epoch_accounts_hash();
    info!("Waiting for epoch accounts hash... DONE");
}
