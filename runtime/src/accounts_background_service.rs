// Service to clean up dead slots in accounts_db
//
// This can be expensive since we have to walk the append vecs being cleaned up.

use crate::{
    bank::{Bank, BankSlotDelta},
    bank_forks::{BankForks, SnapshotConfig},
    snapshot_package::{AccountsPackageSendError, AccountsPackageSender},
    snapshot_utils::{self, SnapshotError},
    status_cache::MAX_CACHE_ENTRIES,
};
use crossbeam_channel::{Receiver, Sender};
use log::*;
use rand::{thread_rng, Rng};
use solana_measure::measure::Measure;
use solana_sdk::clock::Slot;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;
use thiserror::Error;

const INTERVAL_MS: u64 = 100;
const SHRUNKEN_ACCOUNT_PER_SEC: usize = 250;
const SHRUNKEN_ACCOUNT_PER_INTERVAL: usize =
    SHRUNKEN_ACCOUNT_PER_SEC / (1000 / INTERVAL_MS as usize);
const CLEAN_INTERVAL_SLOTS: u64 = 100;

pub type SnapshotRequestSender = Sender<SnapshotRequest>;
pub type SnapshotRequestReceiver = Receiver<SnapshotRequest>;
type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("snapshot error")]
    SnapshotError(#[from] SnapshotError),

    #[error("accounts package send error")]
    AccountsPackageSendError(#[from] AccountsPackageSendError),
}

pub struct SnapshotRequest {
    pub snapshot_root_bank: Arc<Bank>,
    pub status_cache_slot_deltas: Vec<BankSlotDelta>,
}

pub struct SnapshotItems {
    pub snapshot_config: SnapshotConfig,
    pub snapshot_request_receiver: SnapshotRequestReceiver,
    pub accounts_package_sender: AccountsPackageSender,
}

pub struct AccountsBackgroundService {
    t_background: JoinHandle<()>,
}

impl AccountsBackgroundService {
    pub fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
        snapshot_items: Option<SnapshotItems>,
    ) -> Self {
        info!("AccountsBackgroundService active");
        let exit = exit.clone();
        let mut consumed_budget = 0;
        let mut last_cleaned_slot = 0;
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
                let snapshotted_slot = snapshot_items.as_ref().and_then(|snapshot_items| {
                    let SnapshotItems {
                        snapshot_config,
                        snapshot_request_receiver,
                        accounts_package_sender,
                    } = snapshot_items;
                    Self::process_snapshot_requests(
                        snapshot_config,
                        snapshot_request_receiver,
                        accounts_package_sender,
                    )
                });

                if let Some(snapshotted_slot) = snapshotted_slot {
                    // Safe, see proof above
                    assert!(last_cleaned_slot <= snapshotted_slot);
                    last_cleaned_slot = snapshotted_slot;
                } else {
                    consumed_budget = bank.process_stale_slot_with_budget(
                        consumed_budget,
                        SHRUNKEN_ACCOUNT_PER_INTERVAL,
                    );

                    if bank.block_height() - last_cleaned_slot
                        > (CLEAN_INTERVAL_SLOTS + thread_rng().gen_range(0, 10))
                    {
                        bank.clean_accounts(None);
                        last_cleaned_slot = bank.block_height();
                    }
                }

                sleep(Duration::from_millis(INTERVAL_MS));
            })
            .unwrap();
        Self { t_background }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_background.join()
    }

    // Returns the latest rquested snapshot slot, if one exists
    pub fn process_snapshot_requests(
        snapshot_config: &SnapshotConfig,
        snapshot_request_receiver: &SnapshotRequestReceiver,
        accounts_package_sender: &AccountsPackageSender,
    ) -> Option<Slot> {
        snapshot_request_receiver
            .try_iter()
            .last()
            .map(|snapshot_request| {
                let SnapshotRequest {
                    snapshot_root_bank,
                    status_cache_slot_deltas,
                } = snapshot_request;

                snapshot_root_bank.clean_accounts(Some(snapshot_root_bank.slot()));
                snapshot_root_bank.process_stale_slot_with_budget(0, SHRUNKEN_ACCOUNT_PER_INTERVAL);

                // Generate an accounts package
                let mut snapshot_time = Measure::start("total-snapshot-ms");
                let r = Self::generate_accounts_package(
                    &snapshot_root_bank,
                    status_cache_slot_deltas,
                    accounts_package_sender,
                    snapshot_config,
                );
                if r.is_err() {
                    warn!(
                        "Error generating snapshot for bank: {}, err: {:?}",
                        snapshot_root_bank.slot(),
                        r
                    );
                }

                // Cleanup outdated snapshots
                Self::purge_old_snapshots(snapshot_config);
                snapshot_time.stop();
                inc_new_counter_info!("total-snapshot-ms", snapshot_time.as_ms() as usize);
                snapshot_root_bank.slot()
            })
    }

    // Gather the necessary elements for a snapshot of the given `root_bank`
    pub fn generate_accounts_package(
        root_bank: &Bank,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        accounts_package_sender: &AccountsPackageSender,
        snapshot_config: &SnapshotConfig,
    ) -> Result<()> {
        let storages: Vec<_> = root_bank.get_snapshot_storages();
        let mut add_snapshot_time = Measure::start("add-snapshot-ms");
        snapshot_utils::add_snapshot(
            &snapshot_config.snapshot_path,
            &root_bank,
            &storages,
            snapshot_config.snapshot_version,
        )?;
        add_snapshot_time.stop();
        inc_new_counter_info!("add-snapshot-ms", add_snapshot_time.as_ms() as usize);

        // Package the relevant snapshots
        let slot_snapshot_paths =
            snapshot_utils::get_snapshot_paths(&snapshot_config.snapshot_path);
        let latest_slot_snapshot_paths = slot_snapshot_paths
            .last()
            .expect("no snapshots found in config snapshot_path");
        // We only care about the last bank's snapshot.
        // We'll ask the bank for MAX_CACHE_ENTRIES (on the rooted path) worth of statuses
        let package = snapshot_utils::package_snapshot(
            &root_bank,
            latest_slot_snapshot_paths,
            &snapshot_config.snapshot_path,
            status_cache_slot_deltas,
            &snapshot_config.snapshot_package_output_path,
            storages,
            snapshot_config.compression.clone(),
            snapshot_config.snapshot_version,
        )?;

        accounts_package_sender.send(package)?;

        Ok(())
    }

    pub fn purge_old_snapshots(snapshot_config: &SnapshotConfig) {
        // Remove outdated snapshots
        let slot_snapshot_paths =
            snapshot_utils::get_snapshot_paths(&snapshot_config.snapshot_path);
        let num_to_remove = slot_snapshot_paths.len().saturating_sub(MAX_CACHE_ENTRIES);
        for slot_files in &slot_snapshot_paths[..num_to_remove] {
            let r =
                snapshot_utils::remove_snapshot(slot_files.slot, &snapshot_config.snapshot_path);
            if r.is_err() {
                warn!(
                    "Couldn't remove snapshot at: {:?}",
                    snapshot_config.snapshot_path
                );
            }
        }
    }
}
