// Service to clean up dead slots in accounts_db
//
// This can be expensive since we have to walk the append vecs being cleaned up.

use crate::{
    bank::{Bank, BankSlotDelta},
    bank_forks::{BankForks, SnapshotConfig},
    snapshot_package::AccountsPackageSender,
    snapshot_utils,
};
use crossbeam_channel::{Receiver, Sender};
use log::*;
use solana_measure::measure::Measure;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

const INTERVAL_MS: u64 = 100;
const SHRUNKEN_ACCOUNT_PER_SEC: usize = 250;
const SHRUNKEN_ACCOUNT_PER_INTERVAL: usize =
    SHRUNKEN_ACCOUNT_PER_SEC / (1000 / INTERVAL_MS as usize);

pub type SnapshotRequestSender = Sender<SnapshotRequest>;
pub type SnapshotRequestReceiver = Receiver<SnapshotRequest>;

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
    pub fn handle_snapshot_requests(&self) -> Option<Arc<Bank>> {
        self.snapshot_request_receiver
            .try_iter()
            .last()
            .map(|snapshot_request| {
                let SnapshotRequest {
                    snapshot_root_bank,
                    status_cache_slot_deltas,
                } = snapshot_request;

                snapshot_root_bank.process_stale_slot_with_budget(0, SHRUNKEN_ACCOUNT_PER_INTERVAL);
                // Don't clean the slot we're snapshotting because it may have zero-lamport
                // accounts that were included in the bank delta hash when the bank was frozen,
                // and if we clean them here, the newly created snapshot's hash may not match
                // the frozen hash.
                snapshot_root_bank.clean_accounts(Some(snapshot_root_bank.slot() - 1));

                // Generate an accounts package
                let mut snapshot_time = Measure::start("total-snapshot-ms");
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

                // Cleanup outdated snapshots
                snapshot_utils::purge_old_snapshots(&self.snapshot_config.snapshot_path);
                snapshot_time.stop();
                inc_new_counter_info!("total-snapshot-ms", snapshot_time.as_ms() as usize);
                snapshot_root_bank
            })
    }
}

pub struct AccountsBackgroundService {
    t_background: JoinHandle<()>,
}

impl AccountsBackgroundService {
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
        snapshot_request_handler: Option<SnapshotRequestHandler>,
    ) -> Self {
        info!("AccountsBackgroundService active");
        let exit = exit.clone();
        let mut consumed_budget = 0;
        let mut last_cleaned_block_height = 0;
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

                // Claim: Any snapshot request for slot `N` found here means that cleanup has
                // been called at most on slot `N`
                //
                // Proof: Assume for contradiction that we find a snapshot request for slot `N` here,
                // but cleanup has already happened on some slot `M > N`.
                //
                // Then that means on some previous iteration of this loop, we found `bank_forks.root_bank() == M`,
                // but did not see the snapshot for `N`. However, this is impossible because BankForks.set_root()
                // will always flush the snapshot request for `N` to the snapshot request channel
                // before setting a root `M > N`.
                let snapshot_root_bank =
                    snapshot_request_handler
                        .as_ref()
                        .and_then(|snapshot_request_handler| {
                            snapshot_request_handler.handle_snapshot_requests()
                        });

                if let Some(snapshot_root_bank) = snapshot_root_bank {
                    let snapshot_block_height = snapshot_root_bank.block_height();
                    // Safe, see proof above
                    assert!(last_cleaned_block_height <= snapshot_block_height);
                    last_cleaned_block_height = snapshot_block_height;
                } else {
                    consumed_budget = bank.process_stale_slot_with_budget(
                        consumed_budget,
                        SHRUNKEN_ACCOUNT_PER_INTERVAL,
                    );
                }

                sleep(Duration::from_millis(INTERVAL_MS));
            })
            .unwrap();
        Self { t_background }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_background.join()
    }
}
