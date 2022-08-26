// Service to clean up dead slots in accounts_db
//
// This can be expensive since we have to walk the append vecs being cleaned up.

mod stats;
use {
    crate::{
        accounts_hash::CalcAccountsHashConfig,
        bank::{Bank, BankSlotDelta, DropCallback},
        bank_forks::BankForks,
        snapshot_config::SnapshotConfig,
        snapshot_package::{PendingAccountsPackage, SnapshotType},
        snapshot_utils::{self, SnapshotError},
    },
    crossbeam_channel::{Receiver, SendError, Sender},
    log::*,
    rand::{thread_rng, Rng},
    solana_measure::measure::Measure,
    solana_sdk::{
        clock::{BankId, Slot},
        hash::Hash,
    },
    stats::StatsManager,
    std::{
        boxed::Box,
        fmt::{Debug, Formatter},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const INTERVAL_MS: u64 = 100;
const SHRUNKEN_ACCOUNT_PER_SEC: usize = 250;
const SHRUNKEN_ACCOUNT_PER_INTERVAL: usize =
    SHRUNKEN_ACCOUNT_PER_SEC / (1000 / INTERVAL_MS as usize);
const CLEAN_INTERVAL_BLOCKS: u64 = 100;

// This value is chosen to spread the dropping cost over 3 expiration checks
// RecycleStores are fully populated almost all of its lifetime. So, otherwise
// this would drop MAX_RECYCLE_STORES mmaps at once in the worst case...
// (Anyway, the dropping part is outside the AccountsDb::recycle_stores lock
// and dropped in this AccountsBackgroundServe, so this shouldn't matter much)
const RECYCLE_STORE_EXPIRATION_INTERVAL_SECS: u64 = crate::accounts_db::EXPIRATION_TTL_SECONDS / 3;

pub type SnapshotRequestSender = Sender<SnapshotRequest>;
pub type SnapshotRequestReceiver = Receiver<SnapshotRequest>;
pub type DroppedSlotsSender = Sender<(Slot, BankId)>;
pub type DroppedSlotsReceiver = Receiver<(Slot, BankId)>;

#[derive(Clone)]
pub struct SendDroppedBankCallback {
    sender: DroppedSlotsSender,
}

impl DropCallback for SendDroppedBankCallback {
    fn callback(&self, bank: &Bank) {
        let l = self.sender.len();
        if l > 10_000 {
            warn!("Excessive pruned_bank_channel_len: {}", l);
        }

        match self.sender.send((bank.slot(), bank.bank_id())) {
            Err(SendError(_)) => {
                info!("bank DropCallback signal queue disconnected.");
            }
            // success
            Ok(_) => {}
        }
    }

    fn clone_box(&self) -> Box<dyn DropCallback + Send + Sync> {
        Box::new(self.clone())
    }
}

impl Debug for SendDroppedBankCallback {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "SendDroppedBankCallback({:p})", self)
    }
}

impl SendDroppedBankCallback {
    pub fn new(sender: DroppedSlotsSender) -> Self {
        Self { sender }
    }
}

pub struct SnapshotRequest {
    pub snapshot_root_bank: Arc<Bank>,
    pub status_cache_slot_deltas: Vec<BankSlotDelta>,
}

pub struct SnapshotRequestHandler {
    pub snapshot_config: SnapshotConfig,
    pub snapshot_request_receiver: SnapshotRequestReceiver,
    pub pending_accounts_package: PendingAccountsPackage,
}

impl SnapshotRequestHandler {
    // Returns the latest requested snapshot slot, if one exists
    pub fn handle_snapshot_requests(
        &self,
        accounts_db_caching_enabled: bool,
        test_hash_calculation: bool,
        non_snapshot_time_us: u128,
        last_full_snapshot_slot: &mut Option<Slot>,
    ) -> Option<Result<u64, SnapshotError>> {
        self.snapshot_request_receiver
            .try_iter()
            .last()
            .map(|snapshot_request| {
                let mut total_time = Measure::start("snapshot_request_receiver_total_time");
                let SnapshotRequest {
                    snapshot_root_bank,
                    status_cache_slot_deltas,
                } = snapshot_request;

                // we should not rely on the state of this validator until startup verification is complete
                assert!(snapshot_root_bank.is_startup_verification_complete());

                let previous_hash = if test_hash_calculation {
                    // We have to use the index version here.
                    // We cannot calculate the non-index way because cache has not been flushed and stores don't match reality. This comment is out of date and can be re-evaluated.
                    snapshot_root_bank.update_accounts_hash_with_index_option(true, false, false)
                } else {
                    Hash::default()
                };

                let mut shrink_time = Measure::start("shrink_time");
                if !accounts_db_caching_enabled {
                    snapshot_root_bank
                        .process_stale_slot_with_budget(0, SHRUNKEN_ACCOUNT_PER_INTERVAL);
                }
                shrink_time.stop();

                let mut flush_accounts_cache_time = Measure::start("flush_accounts_cache_time");
                if accounts_db_caching_enabled {
                    // Forced cache flushing MUST flush all roots <= snapshot_root_bank.slot().
                    // That's because `snapshot_root_bank.slot()` must be root at this point,
                    // and contains relevant updates because each bank has at least 1 account update due
                    // to sysvar maintenance. Otherwise, this would cause missing storages in the snapshot
                    snapshot_root_bank.force_flush_accounts_cache();
                    // Ensure all roots <= `self.slot()` have been flushed.
                    // Note `max_flush_root` could be larger than self.slot() if there are
                    // `> MAX_CACHE_SLOT` cached and rooted slots which triggered earlier flushes.
                    assert!(
                        snapshot_root_bank.slot()
                            <= snapshot_root_bank
                                .rc
                                .accounts
                                .accounts_db
                                .accounts_cache
                                .fetch_max_flush_root()
                    );
                }
                flush_accounts_cache_time.stop();

                let hash_for_testing = if test_hash_calculation {
                    let use_index_hash_calculation = false;
                    let check_hash = false;

                    let (this_hash, capitalization) = snapshot_root_bank.accounts().accounts_db.calculate_accounts_hash_helper(
                        use_index_hash_calculation,
                        snapshot_root_bank.slot(),
                        &CalcAccountsHashConfig {
                            use_bg_thread_pool: true,
                            check_hash,
                            ancestors: None,
                            use_write_cache: false,
                            epoch_schedule: snapshot_root_bank.epoch_schedule(),
                            rent_collector: snapshot_root_bank.rent_collector(),
                            store_detailed_debug_info_on_failure: false,
                            full_snapshot: None,
                        },
                    ).unwrap();
                    assert_eq!(previous_hash, this_hash);
                    assert_eq!(capitalization, snapshot_root_bank.capitalization());
                    Some(this_hash)
                } else {
                    None
                };

                let mut clean_time = Measure::start("clean_time");
                snapshot_root_bank.clean_accounts(*last_full_snapshot_slot);
                clean_time.stop();

                if accounts_db_caching_enabled {
                    shrink_time = Measure::start("shrink_time");
                    snapshot_root_bank.shrink_candidate_slots();
                    shrink_time.stop();
                }

                let block_height = snapshot_root_bank.block_height();
                let snapshot_type = if snapshot_utils::should_take_full_snapshot(
                    block_height,
                    self.snapshot_config.full_snapshot_archive_interval_slots,
                ) {
                    *last_full_snapshot_slot = Some(snapshot_root_bank.slot());
                    Some(SnapshotType::FullSnapshot)
                } else if snapshot_utils::should_take_incremental_snapshot(
                    block_height,
                    self.snapshot_config
                        .incremental_snapshot_archive_interval_slots,
                    *last_full_snapshot_slot,
                ) {
                    Some(SnapshotType::IncrementalSnapshot(
                        last_full_snapshot_slot.unwrap(),
                    ))
                } else {
                    None
                };

                // Snapshot the bank and send over an accounts package
                let mut snapshot_time = Measure::start("snapshot_time");
                let result = snapshot_utils::snapshot_bank(
                    &snapshot_root_bank,
                    status_cache_slot_deltas,
                    &self.pending_accounts_package,
                    &self.snapshot_config.bank_snapshots_dir,
                    &self.snapshot_config.full_snapshot_archives_dir,
                    &self.snapshot_config.incremental_snapshot_archives_dir,
                    self.snapshot_config.snapshot_version,
                    self.snapshot_config.archive_format,
                    hash_for_testing,
                    snapshot_type,
                );
                if let Err(e) = result {
                    warn!(
                        "Error taking bank snapshot. slot: {}, snapshot type: {:?}, err: {:?}",
                        snapshot_root_bank.slot(),
                        snapshot_type,
                        e,
                    );

                    if Self::is_snapshot_error_fatal(&e) {
                        return Err(e);
                    }
                }
                snapshot_time.stop();
                info!("Took bank snapshot. snapshot type: {:?}, slot: {}, accounts hash: {}, bank hash: {}",
                      snapshot_type,
                      snapshot_root_bank.slot(),
                      snapshot_root_bank.get_accounts_hash(),
                      snapshot_root_bank.hash(),
                  );

                // Cleanup outdated snapshots
                let mut purge_old_snapshots_time = Measure::start("purge_old_snapshots_time");
                snapshot_utils::purge_old_bank_snapshots(&self.snapshot_config.bank_snapshots_dir);
                purge_old_snapshots_time.stop();
                total_time.stop();

                datapoint_info!(
                    "handle_snapshot_requests-timing",
                    (
                        "flush_accounts_cache_time",
                        flush_accounts_cache_time.as_us(),
                        i64
                    ),
                    ("shrink_time", shrink_time.as_us(), i64),
                    ("clean_time", clean_time.as_us(), i64),
                    ("snapshot_time", snapshot_time.as_us(), i64),
                    (
                        "purge_old_snapshots_time",
                        purge_old_snapshots_time.as_us(),
                        i64
                    ),
                    ("total_us", total_time.as_us(), i64),
                    ("non_snapshot_time_us", non_snapshot_time_us, i64),
                );
                Ok(snapshot_root_bank.block_height())
            })
    }

    /// Check if a SnapshotError should be treated as 'fatal' by SnapshotRequestHandler, and
    /// `handle_snapshot_requests()` in particular.  Fatal errors will cause the node to shutdown.
    /// Non-fatal errors are logged and then swallowed.
    ///
    /// All `SnapshotError`s are enumerated, and there is **NO** default case.  This way, if
    /// a new error is added to SnapshotError, a conscious decision must be made on how it should
    /// be handled.
    fn is_snapshot_error_fatal(err: &SnapshotError) -> bool {
        match err {
            SnapshotError::Io(..) => true,
            SnapshotError::Serialize(..) => true,
            SnapshotError::ArchiveGenerationFailure(..) => true,
            SnapshotError::StoragePathSymlinkInvalid => true,
            SnapshotError::UnpackError(..) => true,
            SnapshotError::IoWithSource(..) => true,
            SnapshotError::PathToFileNameError(..) => true,
            SnapshotError::FileNameToStrError(..) => true,
            SnapshotError::ParseSnapshotArchiveFileNameError(..) => true,
            SnapshotError::MismatchedBaseSlot(..) => true,
            SnapshotError::NoSnapshotArchives => true,
            SnapshotError::MismatchedSlotHash(..) => true,
            SnapshotError::VerifySlotDeltas(..) => true,
        }
    }
}

#[derive(Default, Clone)]
pub struct AbsRequestSender {
    snapshot_request_sender: Option<SnapshotRequestSender>,
}

impl AbsRequestSender {
    pub fn new(snapshot_request_sender: SnapshotRequestSender) -> Self {
        Self {
            snapshot_request_sender: Some(snapshot_request_sender),
        }
    }

    pub fn is_snapshot_creation_enabled(&self) -> bool {
        self.snapshot_request_sender.is_some()
    }

    pub fn send_snapshot_request(
        &self,
        snapshot_request: SnapshotRequest,
    ) -> Result<(), SendError<SnapshotRequest>> {
        if let Some(ref snapshot_request_sender) = self.snapshot_request_sender {
            snapshot_request_sender.send(snapshot_request)
        } else {
            Ok(())
        }
    }
}

pub struct AbsRequestHandler {
    pub snapshot_request_handler: Option<SnapshotRequestHandler>,
    pub pruned_banks_receiver: DroppedSlotsReceiver,
}

impl AbsRequestHandler {
    // Returns the latest requested snapshot block height, if one exists
    pub fn handle_snapshot_requests(
        &self,
        accounts_db_caching_enabled: bool,
        test_hash_calculation: bool,
        non_snapshot_time_us: u128,
        last_full_snapshot_slot: &mut Option<Slot>,
    ) -> Option<Result<u64, SnapshotError>> {
        self.snapshot_request_handler
            .as_ref()
            .and_then(|snapshot_request_handler| {
                snapshot_request_handler.handle_snapshot_requests(
                    accounts_db_caching_enabled,
                    test_hash_calculation,
                    non_snapshot_time_us,
                    last_full_snapshot_slot,
                )
            })
    }

    pub fn handle_pruned_banks(&self, bank: &Bank, is_serialized_with_abs: bool) -> usize {
        let mut count = 0;
        for (pruned_slot, pruned_bank_id) in self.pruned_banks_receiver.try_iter() {
            count += 1;
            bank.rc.accounts.accounts_db.purge_slot(
                pruned_slot,
                pruned_bank_id,
                is_serialized_with_abs,
            );
        }

        count
    }
}

pub struct AccountsBackgroundService {
    t_background: JoinHandle<()>,
}

impl AccountsBackgroundService {
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
        request_handler: AbsRequestHandler,
        accounts_db_caching_enabled: bool,
        test_hash_calculation: bool,
        mut last_full_snapshot_slot: Option<Slot>,
    ) -> Self {
        info!("AccountsBackgroundService active");
        let exit = exit.clone();
        let mut consumed_budget = 0;
        let mut last_cleaned_block_height = 0;
        let mut removed_slots_count = 0;
        let mut total_remove_slots_time = 0;
        let mut last_expiration_check_time = Instant::now();
        let t_background = Builder::new()
            .name("solBgAccounts".to_string())
            .spawn(move || {
                let mut stats = StatsManager::new();
                let mut last_snapshot_end_time = None;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    let start_time = Instant::now();

                    // Grab the current root bank
                    let bank = bank_forks.read().unwrap().root_bank().clone();

                    // Purge accounts of any dead slots
                    Self::remove_dead_slots(
                        &bank,
                        &request_handler,
                        &mut removed_slots_count,
                        &mut total_remove_slots_time,
                    );

                    Self::expire_old_recycle_stores(&bank, &mut last_expiration_check_time);

                    let non_snapshot_time = last_snapshot_end_time
                        .map(|last_snapshot_end_time: Instant| {
                            last_snapshot_end_time.elapsed().as_micros()
                        })
                        .unwrap_or_default();

                    // Check to see if there were any requests for snapshotting banks
                    // < the current root bank `bank` above.

                    // Claim: Any snapshot request for slot `N` found here implies that the last cleanup
                    // slot `M` satisfies `M < N`
                    //
                    // Proof: Assume for contradiction that we find a snapshot request for slot `N` here,
                    // but cleanup has already happened on some slot `M >= N`. Because the call to
                    // `bank.clean_accounts(true)` (in the code below) implies we only clean slots `<= bank - 1`,
                    // then that means in some *previous* iteration of this loop, we must have gotten a root
                    // bank for slot some slot `R` where `R > N`, but did not see the snapshot for `N` in the
                    // snapshot request channel.
                    //
                    // However, this is impossible because BankForks.set_root() will always flush the snapshot
                    // request for `N` to the snapshot request channel before setting a root `R > N`, and
                    // snapshot_request_handler.handle_requests() will always look for the latest
                    // available snapshot in the channel.
                    let snapshot_block_height_option_result = request_handler
                        .handle_snapshot_requests(
                            accounts_db_caching_enabled,
                            test_hash_calculation,
                            non_snapshot_time,
                            &mut last_full_snapshot_slot,
                        );
                    if snapshot_block_height_option_result.is_some() {
                        last_snapshot_end_time = Some(Instant::now());
                    }

                    if accounts_db_caching_enabled {
                        // Note that the flush will do an internal clean of the
                        // cache up to bank.slot(), so should be safe as long
                        // as any later snapshots that are taken are of
                        // slots >= bank.slot()
                        bank.flush_accounts_cache_if_needed();
                    }

                    if let Some(snapshot_block_height_result) = snapshot_block_height_option_result
                    {
                        // Safe, see proof above
                        if let Ok(snapshot_block_height) = snapshot_block_height_result {
                            assert!(last_cleaned_block_height <= snapshot_block_height);
                            last_cleaned_block_height = snapshot_block_height;
                        } else {
                            exit.store(true, Ordering::Relaxed);
                            return;
                        }
                    } else {
                        if accounts_db_caching_enabled {
                            bank.shrink_candidate_slots();
                        } else {
                            // under sustained writes, shrink can lag behind so cap to
                            // SHRUNKEN_ACCOUNT_PER_INTERVAL (which is based on INTERVAL_MS,
                            // which in turn roughly associated block time)
                            consumed_budget = bank
                                .process_stale_slot_with_budget(
                                    consumed_budget,
                                    SHRUNKEN_ACCOUNT_PER_INTERVAL,
                                )
                                .min(SHRUNKEN_ACCOUNT_PER_INTERVAL);
                        }
                        if bank.block_height() - last_cleaned_block_height
                            > (CLEAN_INTERVAL_BLOCKS + thread_rng().gen_range(0, 10))
                        {
                            if accounts_db_caching_enabled {
                                // Note that the flush will do an internal clean of the
                                // cache up to bank.slot(), so should be safe as long
                                // as any later snapshots that are taken are of
                                // slots >= bank.slot()
                                bank.force_flush_accounts_cache();
                            }
                            bank.clean_accounts(last_full_snapshot_slot);
                            last_cleaned_block_height = bank.block_height();
                        }
                    }
                    stats.record_and_maybe_submit(start_time.elapsed());
                    sleep(Duration::from_millis(INTERVAL_MS));
                }
            })
            .unwrap();
        Self { t_background }
    }

    /// Should be called immediately after bank_fork_utils::load_bank_forks(), and as such, there
    /// should only be one bank, the root bank, in `bank_forks`
    /// All banks added to `bank_forks` will be descended from the root bank, and thus will inherit
    /// the bank drop callback.
    pub fn setup_bank_drop_callback(bank_forks: Arc<RwLock<BankForks>>) -> DroppedSlotsReceiver {
        assert_eq!(bank_forks.read().unwrap().banks().len(), 1);

        let (pruned_banks_sender, pruned_banks_receiver) = crossbeam_channel::unbounded();
        {
            let root_bank = bank_forks.read().unwrap().root_bank();
            root_bank.set_callback(Some(Box::new(
                root_bank
                    .rc
                    .accounts
                    .accounts_db
                    .create_drop_bank_callback(pruned_banks_sender),
            )));
        }
        pruned_banks_receiver
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_background.join()
    }

    fn remove_dead_slots(
        bank: &Bank,
        request_handler: &AbsRequestHandler,
        removed_slots_count: &mut usize,
        total_remove_slots_time: &mut u64,
    ) {
        let mut remove_slots_time = Measure::start("remove_slots_time");
        *removed_slots_count += request_handler.handle_pruned_banks(bank, true);
        remove_slots_time.stop();
        *total_remove_slots_time += remove_slots_time.as_us();

        if *removed_slots_count >= 100 {
            datapoint_info!(
                "remove_slots_timing",
                ("remove_slots_time", *total_remove_slots_time, i64),
                ("removed_slots_count", *removed_slots_count, i64),
            );
            *total_remove_slots_time = 0;
            *removed_slots_count = 0;
        }
    }

    fn expire_old_recycle_stores(bank: &Bank, last_expiration_check_time: &mut Instant) {
        let now = Instant::now();
        if now.duration_since(*last_expiration_check_time).as_secs()
            > RECYCLE_STORE_EXPIRATION_INTERVAL_SECS
        {
            bank.expire_old_recycle_stores();
            *last_expiration_check_time = now;
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::genesis_utils::create_genesis_config,
        crossbeam_channel::unbounded,
        solana_sdk::{account::AccountSharedData, pubkey::Pubkey},
    };

    #[test]
    fn test_accounts_background_service_remove_dead_slots() {
        let genesis = create_genesis_config(10);
        let bank0 = Arc::new(Bank::new_for_tests(&genesis.genesis_config));
        let (pruned_banks_sender, pruned_banks_receiver) = unbounded();
        let request_handler = AbsRequestHandler {
            snapshot_request_handler: None,
            pruned_banks_receiver,
        };

        // Store an account in slot 0
        let account_key = Pubkey::new_unique();
        bank0.store_account(
            &account_key,
            &AccountSharedData::new(264, 0, &Pubkey::default()),
        );
        assert!(bank0.get_account(&account_key).is_some());
        pruned_banks_sender.send((0, 0)).unwrap();

        assert!(!bank0.rc.accounts.scan_slot(0, |_| Some(())).is_empty());

        AccountsBackgroundService::remove_dead_slots(&bank0, &request_handler, &mut 0, &mut 0);

        assert!(bank0.rc.accounts.scan_slot(0, |_| Some(())).is_empty());
    }
}
