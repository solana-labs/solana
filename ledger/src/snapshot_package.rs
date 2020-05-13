use crate::bank_forks::CompressionType;
use solana_runtime::{accounts_db::SnapshotStorages, bank::BankSlotDelta};
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, SendError, Sender},
};
use tempfile::TempDir;

pub type AccountsPackageSender = Sender<AccountsPackage>;
pub type AccountsPackageReceiver = Receiver<AccountsPackage>;
pub type AccountsPackageSendError = SendError<AccountsPackage>;

#[derive(Debug)]
pub struct AccountsPackage {
    pub root: Slot,
    pub block_height: Slot,
    pub slot_deltas: Vec<BankSlotDelta>,
    pub snapshot_links: TempDir,
    pub storages: SnapshotStorages,
    pub tar_output_file: PathBuf,
    pub hash: Hash,
    pub compression: CompressionType,
}

impl AccountsPackage {
    pub fn new(
        root: Slot,
        block_height: u64,
        slot_deltas: Vec<BankSlotDelta>,
        snapshot_links: TempDir,
        storages: SnapshotStorages,
        tar_output_file: PathBuf,
        hash: Hash,
        compression: CompressionType,
    ) -> Self {
        Self {
            root,
            block_height,
            slot_deltas,
            snapshot_links,
            storages,
            tar_output_file,
            hash,
            compression,
        }
    }
}
