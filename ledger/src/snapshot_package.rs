use crate::bank_forks::CompressionType;
use solana_runtime::{accounts_db::SnapshotStorages, bank::BankSlotDelta};
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, SendError, Sender},
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
    pub storages: SnapshotStorages,
    pub tar_output_file: PathBuf,
    pub hash: Hash,
    pub compression: CompressionType,
}

impl SnapshotPackage {
    pub fn new(
        root: Slot,
        slot_deltas: Vec<BankSlotDelta>,
        snapshot_links: TempDir,
        storages: SnapshotStorages,
        tar_output_file: PathBuf,
        hash: Hash,
        compression: CompressionType,
    ) -> Self {
        Self {
            root,
            slot_deltas,
            snapshot_links,
            storages,
            tar_output_file,
            hash,
            compression,
        }
    }
}
