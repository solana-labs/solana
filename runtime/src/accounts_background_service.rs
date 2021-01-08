// Service to clean up dead slots in accounts_db
//
// This can be expensive since we have to walk the append vecs being cleaned up.

use crate::{
    bank::{Bank, BankSlotDelta, DropCallback},
    bank_forks::{BankForks, SnapshotConfig},
    snapshot_package::AccountsPackageSender,
    snapshot_utils,
};
use crossbeam_channel::{Receiver, SendError, Sender};
use log::*;
use rand::{thread_rng, Rng};
use solana_measure::measure::Measure;
use solana_sdk::clock::Slot;
use std::{
    boxed::Box,
    fmt::{Debug, Formatter},
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
pub type DroppedSlotsSender = Sender<Slot>;
pub type DroppedSlotsReceiver = Receiver<Slot>;

#[derive(Clone)]
pub struct SendDroppedBankCallback {
    sender: DroppedSlotsSender,
}

impl DropCallback for SendDroppedBankCallback {
    fn callback(&self, bank: &Bank) {
        if let Err(e) = self.sender.send(bank.slot()) {
            warn!("Error sending dropped banks: {:?}", e);
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
                    &self.snapshot_config.archive_format,
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
}

impl ABSRequestSender {
    pub fn new(snapshot_request_sender: Option<SnapshotRequestSender>) -> Self {
        ABSRequestSender {
            snapshot_request_sender,
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

pub struct ABSRequestHandler {
    pub snapshot_request_handler: Option<SnapshotRequestHandler>,
    pub pruned_banks_receiver: DroppedSlotsReceiver,
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

    pub fn handle_pruned_banks(&self, bank: &Bank) -> usize {
        let mut count = 0;
        for pruned_slot in self.pruned_banks_receiver.try_iter() {
            count += 1;
            bank.rc.accounts.purge_slot(pruned_slot);
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
        request_handler: ABSRequestHandler,
    ) -> Self {
        info!("AccountsBackgroundService active");
        let exit = exit.clone();
        let mut consumed_budget = 0;
        let mut last_cleaned_block_height = 0;
        let mut removed_slots_count = 0;
        let mut total_remove_slots_time = 0;
        let t_background = Builder::new()
            .name("solana-accounts-background".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                // Grab the current root bank
                let bank = bank_forks.read().unwrap().root_bank().clone();

                // Purge accounts of any dead slots
                Self::remove_dead_slots(
                    &bank,
                    &request_handler,
                    &mut removed_slots_count,
                    &mut total_remove_slots_time,
                );

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
                    // under sustained writes, shrink can lag behind so cap to
                    // SHRUNKEN_ACCOUNT_PER_INTERVAL (which is based on INTERVAL_MS,
                    // which in turn roughly asscociated block time)
                    consumed_budget = bank
                        .process_stale_slot_with_budget(
                            consumed_budget,
                            SHRUNKEN_ACCOUNT_PER_INTERVAL,
                        )
                        .min(SHRUNKEN_ACCOUNT_PER_INTERVAL);

                    if bank.block_height() - last_cleaned_block_height
                        > (CLEAN_INTERVAL_BLOCKS + thread_rng().gen_range(0, 10))
                    {
                        bank.clean_accounts(true);
                        last_cleaned_block_height = bank.block_height();
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

    fn remove_dead_slots(
        bank: &Bank,
        request_handler: &ABSRequestHandler,
        removed_slots_count: &mut usize,
        total_remove_slots_time: &mut u64,
    ) {
        let mut remove_slots_time = Measure::start("remove_slots_time");
        *removed_slots_count += request_handler.handle_pruned_banks(&bank);
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
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::genesis_utils::create_genesis_config;
    use crossbeam_channel::unbounded;
    use solana_sdk::{account::Account, pubkey::Pubkey};

    #[test]
    fn test_accounts_background_service_remove_dead_slots() {
        let genesis = create_genesis_config(10);
        let bank0 = Arc::new(Bank::new(&genesis.genesis_config));
        let (pruned_banks_sender, pruned_banks_receiver) = unbounded();
        let request_handler = ABSRequestHandler {
            snapshot_request_handler: None,
            pruned_banks_receiver,
        };

        // Store an account in slot 0
        let account_key = Pubkey::new_unique();
        bank0.store_account(&account_key, &Account::new(264, 0, &Pubkey::default()));
        assert!(bank0.get_account(&account_key).is_some());
        pruned_banks_sender.send(0).unwrap();
        AccountsBackgroundService::remove_dead_slots(&bank0, &request_handler, &mut 0, &mut 0);

        // Slot should be removed
        assert!(bank0.get_account(&account_key).is_none());
    }
}
