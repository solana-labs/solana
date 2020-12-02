// Service to clean up dead slots in accounts_db
//
// This can be expensive since we have to walk the append vecs being cleaned up.

use crate::{
    bank::{Bank, BankSlotDelta},
    bank_forks::{BankForks, SnapshotConfig},
    snapshot_package::AccountsPackageSender,
    snapshot_utils,
};
use crossbeam_channel::{Receiver, SendError, Sender};
use log::*;
use rand::{thread_rng, Rng};
use solana_measure::measure::Measure;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    thread::{self, sleep, Builder, JoinHandle},
    time::Duration,
};

const INTERVAL_MS: u64 = 100;
const SHRUNKEN_ACCOUNT_PER_SEC: usize = 250;
const SHRUNKEN_ACCOUNT_PER_INTERVAL: usize =
    SHRUNKEN_ACCOUNT_PER_SEC / (1000 / INTERVAL_MS as usize);
const CLEAN_INTERVAL_BLOCKS: u64 = 100;

pub type SnapshotRequestSender = Sender<SnapshotRequest>;
pub type SnapshotRequestReceiver = Receiver<SnapshotRequest>;
pub type PrunedBanksSender = Sender<Vec<Arc<Bank>>>;
pub type PrunedBanksReceiver = Receiver<Vec<Arc<Bank>>>;

pub struct SnapshotRequest {
    pub snapshot_root_bank: Arc<Bank>,
    pub status_cache_slot_deltas: Vec<BankSlotDelta>,
}

pub struct SnapshotRequestHandler {
    pub snapshot_config: SnapshotConfig,
    pub snapshot_request_receiver: SnapshotRequestReceiver,
    pub accounts_package_sender: AccountsPackageSender,
}

impl SnapshotRequestHandler {
    // Returns the latest requested snapshot slot, if one exists
    pub fn handle_snapshot_requests(&self) -> Option<u64> {
        self.snapshot_request_receiver
            .try_iter()
            .last()
            .map(|snapshot_request| {
                let SnapshotRequest {
                    snapshot_root_bank,
                    status_cache_slot_deltas,
                } = snapshot_request;

                let mut hash_time = Measure::start("hash_time");
                snapshot_root_bank.update_accounts_hash();
                hash_time.stop();

                let mut shrink_time = Measure::start("shrink_time");
                snapshot_root_bank.process_stale_slot_with_budget(0, SHRUNKEN_ACCOUNT_PER_INTERVAL);
                shrink_time.stop();

                let mut clean_time = Measure::start("clean_time");
                // Don't clean the slot we're snapshotting because it may have zero-lamport
                // accounts that were included in the bank delta hash when the bank was frozen,
                // and if we clean them here, the newly created snapshot's hash may not match
                // the frozen hash.
                snapshot_root_bank.clean_accounts(true);
                clean_time.stop();

                // Generate an accounts package
                let mut snapshot_time = Measure::start("snapshot_time");
                let r = snapshot_utils::snapshot_bank(
                    &snapshot_root_bank,
                    status_cache_slot_deltas,
                    &self.accounts_package_sender,
                    &self.snapshot_config.snapshot_path,
                    &self.snapshot_config.snapshot_package_output_path,
                    self.snapshot_config.snapshot_version,
                    &self.snapshot_config.compression,
                );
                if r.is_err() {
                    warn!(
                        "Error generating snapshot for bank: {}, err: {:?}",
                        snapshot_root_bank.slot(),
                        r
                    );
                }
                snapshot_time.stop();

                // Cleanup outdated snapshots
                let mut purge_old_snapshots_time = Measure::start("purge_old_snapshots_time");
                snapshot_utils::purge_old_snapshots(&self.snapshot_config.snapshot_path);
                purge_old_snapshots_time.stop();

                datapoint_info!(
                    "handle_snapshot_requests-timing",
                    ("shrink_time", shrink_time.as_us(), i64),
                    ("clean_time", clean_time.as_us(), i64),
                    ("snapshot_time", snapshot_time.as_us(), i64),
                    (
                        "purge_old_snapshots_time",
                        purge_old_snapshots_time.as_us(),
                        i64
                    ),
                    ("hash_time", hash_time.as_us(), i64),
                );
                snapshot_root_bank.block_height()
            })
    }
}

#[derive(Default)]
pub struct ABSRequestSender {
    snapshot_request_sender: Option<SnapshotRequestSender>,
    pruned_banks_sender: Option<PrunedBanksSender>,
}

impl ABSRequestSender {
    pub fn new(
        snapshot_request_sender: Option<SnapshotRequestSender>,
        pruned_banks_sender: Option<PrunedBanksSender>,
    ) -> Self {
        ABSRequestSender {
            snapshot_request_sender,
            pruned_banks_sender,
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

    pub fn send_pruned_banks(
        &self,
        pruned_banks: Vec<Arc<Bank>>,
    ) -> Result<(), SendError<Vec<Arc<Bank>>>> {
        if let Some(ref pruned_banks_sender) = self.pruned_banks_sender {
            pruned_banks_sender.send(pruned_banks)
        } else {
            // If there is no sender, the banks are dropped immediately, which
            // is not recommended for anything other than tests.
            Ok(())
        }
    }
}

pub struct ABSRequestHandler {
    pub snapshot_request_handler: Option<SnapshotRequestHandler>,
    pub pruned_banks_receiver: PrunedBanksReceiver,
}

impl ABSRequestHandler {
    // Returns the latest requested snapshot block height, if one exists
    pub fn handle_snapshot_requests(&self) -> Option<u64> {
        self.snapshot_request_handler
            .as_ref()
            .and_then(|snapshot_request_handler| {
                snapshot_request_handler.handle_snapshot_requests()
            })
    }

    pub fn handle_pruned_banks<'a>(&'a self) -> impl Iterator<Item = Arc<Bank>> + 'a {
        self.pruned_banks_receiver.try_iter().flatten()
    }
}

pub struct AccountsBackgroundService {
    t_background: JoinHandle<()>,
}

impl AccountsBackgroundService {
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
        request_handler: ABSRequestHandler,
    ) -> Self {
        info!("AccountsBackgroundService active");
        let exit = exit.clone();
        let mut consumed_budget = 0;
        let mut last_cleaned_block_height = 0;
        let mut pending_drop_banks = vec![];
        let mut dropped_count = 0;
        let mut total_scan_pending_total_drop_banks_time = 0;
        let mut total_drop_banks_time = 0;
        let t_background = Builder::new()
            .name("solana-accounts-background".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                // Grab the current root bank
                let bank = bank_forks.read().unwrap().root_bank().clone();

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
                let snapshot_block_height = request_handler.handle_snapshot_requests();

                if let Some(snapshot_block_height) = snapshot_block_height {
                    // Safe, see proof above
                    assert!(last_cleaned_block_height <= snapshot_block_height);
                    last_cleaned_block_height = snapshot_block_height;
                } else {
                    consumed_budget = bank.process_stale_slot_with_budget(
                        consumed_budget,
                        SHRUNKEN_ACCOUNT_PER_INTERVAL,
                    );

                    if bank.block_height() - last_cleaned_block_height
                        > (CLEAN_INTERVAL_BLOCKS + thread_rng().gen_range(0, 10))
                    {
                        bank.clean_accounts(true);
                        last_cleaned_block_height = bank.block_height();
                    }
                }

                pending_drop_banks.extend(request_handler.handle_pruned_banks());
                pending_drop_banks = Self::scan_for_drops(
                    pending_drop_banks,
                    &mut dropped_count,
                    &mut total_scan_pending_total_drop_banks_time,
                    &mut total_drop_banks_time,
                );
                sleep(Duration::from_millis(INTERVAL_MS));
            })
            .unwrap();
        Self { t_background }
    }

    fn scan_for_drops(
        pending_drop_banks: Vec<Arc<Bank>>,
        dropped_count: &mut usize,
        total_scan_pending_total_drop_banks_time: &mut u64,
        total_drop_banks_time: &mut u64,
    ) -> Vec<Arc<Bank>> {
        let mut scan_time = Measure::start("total_scan_pending_total_drop_banks_time");

        // Should use `drain_filter()`, but not available in stable Rust yet.
        let (to_drop, to_keep): (Vec<Arc<Bank>>, Vec<Arc<Bank>>) = pending_drop_banks
            .into_iter()
            .partition(|bank| Arc::strong_count(bank) == 1);
        scan_time.stop();
        *total_scan_pending_total_drop_banks_time += scan_time.as_us();

        *dropped_count += to_drop.len();

        // Drop the banks that are dead
        let mut drop_time = Measure::start("total_drop_banks_time");
        drop(to_drop);
        drop_time.stop();
        *total_drop_banks_time += drop_time.as_us();

        if *dropped_count >= 100 {
            datapoint_info!(
                "drop-banks-timing",
                (
                    "total_scan_pending_total_drop_banks_time",
                    *total_scan_pending_total_drop_banks_time,
                    i64
                ),
                ("total_drop_banks_time", *total_drop_banks_time, i64),
                ("dropped_count", *dropped_count, i64),
            );
            *total_scan_pending_total_drop_banks_time = 0;
            *total_drop_banks_time = 0;
            *dropped_count = 0;
        }

        to_keep
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_background.join()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::genesis_utils::create_genesis_config;
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_scan_for_drops() {
        let genesis = create_genesis_config(10);
        let bank = Arc::new(Bank::new(&genesis.genesis_config));

        // Handle empty case
        assert!(AccountsBackgroundService::scan_for_drops(vec![], &mut 0, &mut 0, &mut 0).is_empty());

        // All banks should be dropped because there's only one reference to the bank
        assert!(
            AccountsBackgroundService::scan_for_drops(vec![bank], &mut 0, &mut 0, &mut 0).is_empty()
        );

        let bank0 = Arc::new(Bank::new(&genesis.genesis_config));
        let bank0_ref = bank0.clone();
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));

        // With an additional reference, only the single referenced bank should be dropped
        let result =
            AccountsBackgroundService::scan_for_drops(vec![bank0, bank1], &mut 0, &mut 0, &mut 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].slot(), 0);

        // Drop the reference, everything should be dropped
        drop(bank0_ref);
        assert!(AccountsBackgroundService::scan_for_drops(result, &mut 0, &mut 0, &mut 0).is_empty());
    }
}
