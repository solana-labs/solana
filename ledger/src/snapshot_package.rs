use solana_runtime::{accounts_db::AccountStorageEntry, bank::BankSlotDelta};
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::{
    path::PathBuf,
    sync::{
        mpsc::{Receiver, SendError, Sender},
        Arc,
    },
};
use tempfile::TempDir;

pub type SnapshotPackageSender = Sender<SnapshotPackage>;
pub type SnapshotPackageReceiver = Receiver<SnapshotPackage>;
pub type SnapshotPackageSendError = SendError<SnapshotPackage>;

#[derive(Debug)]
pub struct SnapshotPackage {
    pub root: Slot,
    pub slot_deltas: Vec<BankSlotDelta>,
    pub snapshot_links: TempDir,
    pub storage_entries: Vec<Arc<AccountStorageEntry>>,
    pub tar_output_file: PathBuf,
    pub hash: Hash,
}

impl SnapshotPackage {
    pub fn new(
        root: Slot,
        slot_deltas: Vec<BankSlotDelta>,
        snapshot_links: TempDir,
        storage_entries: Vec<Arc<AccountStorageEntry>>,
        tar_output_file: PathBuf,
        hash: Hash,
    ) -> Self {
        Self {
            root,
            slot_deltas,
            snapshot_links,
            storage_entries,
            tar_output_file,
            hash,
        }
    }
}
