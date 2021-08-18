use crate::{
    accounts_db::SnapshotStorages,
    bank::{Bank, BankSlotDelta},
    snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter},
    snapshot_utils::{self, ArchiveFormat, BankSnapshotInfo, Result, SnapshotVersion},
};
use solana_sdk::{clock::Slot, genesis_config::ClusterType, hash::Hash};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        mpsc::{Receiver, SendError, Sender},
        Arc, Mutex,
    },
};
use tempfile::TempDir;

/// The sender side of the AccountsPackage channel, used by AccountsBackgroundService
pub type AccountsPackageSender = Sender<AccountsPackage>;

/// The receiver side of the AccountsPackage channel, used by AccountsHashVerifier
pub type AccountsPackageReceiver = Receiver<AccountsPackage>;

/// The error type when sending an AccountsPackage over the channel fails
pub type AccountsPackageSendError = SendError<AccountsPackage>;

/// The PendingSnapshotPackage passes a SnapshotPackage from AccountsHashVerifier to
/// SnapshotPackagerService for archiving
pub type PendingSnapshotPackage = Arc<Mutex<Option<SnapshotPackage>>>;

#[derive(Debug)]
pub struct AccountsPackage {
    pub slot: Slot,
    pub block_height: Slot,
    pub slot_deltas: Vec<BankSlotDelta>,
    pub snapshot_links: TempDir,
    pub storages: SnapshotStorages,
    pub hash: Hash, // temporarily here while we still have to calculate hash before serializing bank
    pub snapshot_version: SnapshotVersion,
    pub expected_capitalization: u64,
    pub hash_for_testing: Option<Hash>,
    pub cluster_type: ClusterType,
}

impl AccountsPackage {
    /// Create an accounts package
    pub fn new(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        snapshots_dir: impl AsRef<Path>,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_storages: SnapshotStorages,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
    ) -> Result<Self> {
        let snapshot_tmpdir = tempfile::Builder::new()
            .prefix(&format!(
                "{}{}-",
                snapshot_utils::TMP_BANK_SNAPSHOT_PREFIX,
                bank.slot()
            ))
            .tempdir_in(snapshots_dir)?;

        // Hard link the snapshot into a tmpdir, to ensure its not removed prior to packaging.
        {
            let snapshot_hardlink_dir = snapshot_tmpdir
                .as_ref()
                .join(bank_snapshot_info.slot.to_string());
            fs::create_dir_all(&snapshot_hardlink_dir)?;
            fs::hard_link(
                &bank_snapshot_info.snapshot_path,
                &snapshot_hardlink_dir.join(bank_snapshot_info.slot.to_string()),
            )?;
        }

        Ok(Self {
            slot: bank.slot(),
            block_height: bank.block_height(),
            slot_deltas: status_cache_slot_deltas,
            snapshot_links: snapshot_tmpdir,
            storages: snapshot_storages,
            hash: bank.get_accounts_hash(),
            snapshot_version,
            expected_capitalization: bank.capitalization(),
            hash_for_testing,
            cluster_type: bank.cluster_type(),
        })
    }
}

pub struct SnapshotPackage {
    pub snapshot_archive_info: SnapshotArchiveInfo,
    pub block_height: Slot,
    pub slot_deltas: Vec<BankSlotDelta>,
    pub snapshot_links: TempDir,
    pub storages: SnapshotStorages,
    pub snapshot_version: SnapshotVersion,
    pub snapshot_type: SnapshotType,
}

impl SnapshotPackage {
    pub fn new(
        mut accounts_package: AccountsPackage,
        snapshot_archives_dir: PathBuf,
        archive_format: ArchiveFormat,
        incremental_snapshot_base_slot: Option<Slot>,
    ) -> Self {
        let (snapshot_type, snapshot_archive_path) = match incremental_snapshot_base_slot {
            None => {
                let snapshot_archive_path = snapshot_utils::build_full_snapshot_archive_path(
                    snapshot_archives_dir,
                    accounts_package.slot,
                    &accounts_package.hash,
                    archive_format,
                );
                (SnapshotType::FullSnapshot, snapshot_archive_path)
            }
            Some(incremental_snapshot_base_slot) => {
                let snapshot_archive_path = snapshot_utils::build_incremental_snapshot_archive_path(
                    snapshot_archives_dir,
                    incremental_snapshot_base_slot,
                    accounts_package.slot,
                    &accounts_package.hash,
                    archive_format,
                );
                snapshot_utils::filter_snapshot_storages_for_incremental_snapshot(
                    &mut accounts_package.storages,
                    incremental_snapshot_base_slot,
                );
                (SnapshotType::IncrementalSnapshot, snapshot_archive_path)
            }
        };

        Self {
            snapshot_archive_info: SnapshotArchiveInfo {
                path: snapshot_archive_path,
                slot: accounts_package.slot,
                hash: accounts_package.hash,
                archive_format,
            },
            block_height: accounts_package.block_height,
            slot_deltas: accounts_package.slot_deltas,
            snapshot_links: accounts_package.snapshot_links,
            storages: accounts_package.storages,
            snapshot_version: accounts_package.snapshot_version,
            snapshot_type,
        }
    }
}

impl SnapshotArchiveInfoGetter for SnapshotPackage {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo {
        &self.snapshot_archive_info
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotType {
    FullSnapshot,
    IncrementalSnapshot,
}

impl SnapshotType {
    /// Get the string prefix of the snapshot type
    pub fn to_prefix(&self) -> &'static str {
        match self {
            SnapshotType::FullSnapshot => snapshot_utils::TMP_FULL_SNAPSHOT_ARCHIVE_PREFIX,
            SnapshotType::IncrementalSnapshot => {
                snapshot_utils::TMP_INCREMENTAL_SNAPSHOT_ARCHIVE_PREFIX
            }
        }
    }
}
