use crate::{
    accounts_db::SnapshotStorages,
    bank::{Bank, BankSlotDelta},
};
use crate::{
    snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter},
    snapshot_utils::{
        ArchiveFormat, BankSnapshotInfo, Result, SnapshotVersion, TMP_FULL_SNAPSHOT_PREFIX,
        TMP_INCREMENTAL_SNAPSHOT_PREFIX,
    },
};
use log::*;
use solana_sdk::clock::Slot;
use solana_sdk::genesis_config::ClusterType;
use solana_sdk::hash::Hash;
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
    pub archive_format: ArchiveFormat,
    pub snapshot_version: SnapshotVersion,
    pub snapshot_output_dir: PathBuf,
    pub expected_capitalization: u64,
    pub hash_for_testing: Option<Hash>,
    pub cluster_type: ClusterType,
}

impl AccountsPackage {
    /// Create an accounts package
    #[allow(clippy::too_many_arguments)]
    fn new(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_package_output_path: impl AsRef<Path>,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
        snapshot_tmpdir: TempDir,
    ) -> Result<Self> {
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
            archive_format,
            snapshot_version,
            snapshot_output_dir: snapshot_package_output_path.as_ref().to_path_buf(),
            expected_capitalization: bank.capitalization(),
            hash_for_testing,
            cluster_type: bank.cluster_type(),
        })
    }

    /// Package up bank snapshot files, snapshot storages, and slot deltas for a full snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_full_snapshot(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        snapshots_dir: impl AsRef<Path>,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_package_output_path: impl AsRef<Path>,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
    ) -> Result<Self> {
        info!(
            "Package full snapshot for bank: {} has {} account storage entries",
            bank.slot(),
            snapshot_storages.len()
        );

        let snapshot_tmpdir = tempfile::Builder::new()
            .prefix(&format!("{}{}-", TMP_FULL_SNAPSHOT_PREFIX, bank.slot()))
            .tempdir_in(snapshots_dir)?;

        Self::new(
            bank,
            bank_snapshot_info,
            status_cache_slot_deltas,
            snapshot_package_output_path,
            snapshot_storages,
            archive_format,
            snapshot_version,
            hash_for_testing,
            snapshot_tmpdir,
        )
    }

    /// Package up bank snapshot files, snapshot storages, and slot deltas for an incremental snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_incremental_snapshot(
        bank: &Bank,
        incremental_snapshot_base_slot: Slot,
        bank_snapshot_info: &BankSnapshotInfo,
        snapshots_dir: impl AsRef<Path>,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_package_output_path: impl AsRef<Path>,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
    ) -> Result<Self> {
        info!(
            "Package incremental snapshot for bank {} (from base slot {}) has {} account storage entries",
            bank.slot(),
            incremental_snapshot_base_slot,
            snapshot_storages.len()
        );

        assert!(
            snapshot_storages.iter().all(|storage| storage
                .iter()
                .all(|entry| entry.slot() > incremental_snapshot_base_slot)),
            "Incremental snapshot package must only contain storage entries where slot > incremental snapshot base slot (i.e. full snapshot slot)!"
        );

        let snapshot_tmpdir = tempfile::Builder::new()
            .prefix(&format!(
                "{}{}-{}-",
                TMP_INCREMENTAL_SNAPSHOT_PREFIX,
                incremental_snapshot_base_slot,
                bank.slot()
            ))
            .tempdir_in(snapshots_dir)?;

        Self::new(
            bank,
            bank_snapshot_info,
            status_cache_slot_deltas,
            snapshot_package_output_path,
            snapshot_storages,
            archive_format,
            snapshot_version,
            hash_for_testing,
            snapshot_tmpdir,
        )
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slot: Slot,
        block_height: u64,
        slot_deltas: Vec<BankSlotDelta>,
        snapshot_links: TempDir,
        storages: SnapshotStorages,
        snapshot_archive_path: PathBuf,
        hash: Hash,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        snapshot_type: SnapshotType,
    ) -> Self {
        Self {
            snapshot_archive_info: SnapshotArchiveInfo {
                path: snapshot_archive_path,
                slot,
                hash,
                archive_format,
            },
            block_height,
            slot_deltas,
            snapshot_links,
            storages,
            snapshot_version,
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
            SnapshotType::FullSnapshot => TMP_FULL_SNAPSHOT_PREFIX,
            SnapshotType::IncrementalSnapshot => TMP_INCREMENTAL_SNAPSHOT_PREFIX,
        }
    }
}
