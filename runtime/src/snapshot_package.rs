use crate::snapshot_utils::{
    ArchiveFormat, BankSnapshotInfo, Result, SnapshotArchiveInfo, SnapshotArchiveInfoGetter,
    SnapshotVersion, TMP_FULL_SNAPSHOT_PREFIX, TMP_INCREMENTAL_SNAPSHOT_PREFIX,
};
use crate::{
    accounts_db::SnapshotStorages,
    bank::{Bank, BankSlotDelta},
};
use log::*;
use solana_sdk::clock::Slot;
use solana_sdk::genesis_config::ClusterType;
use solana_sdk::hash::Hash;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, SendError, Sender},
};
use tempfile::TempDir;

pub type AccountsPackageSender = Sender<AccountsPackagePre>;
pub type AccountsPackageReceiver = Receiver<AccountsPackagePre>;
pub type AccountsPackageSendError = SendError<AccountsPackagePre>;

#[derive(Debug)]
pub struct AccountsPackagePre {
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

impl AccountsPackagePre {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        slot: Slot,
        block_height: u64,
        slot_deltas: Vec<BankSlotDelta>,
        snapshot_links: TempDir,
        storages: SnapshotStorages,
        hash: Hash,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        snapshot_output_dir: PathBuf,
        expected_capitalization: u64,
        hash_for_testing: Option<Hash>,
        cluster_type: ClusterType,
    ) -> Self {
        Self {
            slot,
            block_height,
            slot_deltas,
            snapshot_links,
            storages,
            hash,
            archive_format,
            snapshot_version,
            snapshot_output_dir,
            expected_capitalization,
            hash_for_testing,
            cluster_type,
        }
    }

    /// Create a snapshot package
    #[allow(clippy::too_many_arguments)]
    fn new_snapshot_package<P>(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_package_output_path: P,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
        snapshot_tmpdir: TempDir,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
    {
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

        Ok(Self::new(
            bank.slot(),
            bank.block_height(),
            status_cache_slot_deltas,
            snapshot_tmpdir,
            snapshot_storages,
            bank.get_accounts_hash(),
            archive_format,
            snapshot_version,
            snapshot_package_output_path.as_ref().to_path_buf(),
            bank.capitalization(),
            hash_for_testing,
            bank.cluster_type(),
        ))
    }

    /// Package up bank snapshot files, snapshot storages, and slot deltas for a full snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn new_full_snapshot_package<P, Q>(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        snapshots_dir: P,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_package_output_path: Q,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        info!(
            "Package full snapshot for bank: {} has {} account storage entries",
            bank.slot(),
            snapshot_storages.len()
        );

        let snapshot_tmpdir = tempfile::Builder::new()
            .prefix(&format!("{}{}-", TMP_FULL_SNAPSHOT_PREFIX, bank.slot()))
            .tempdir_in(snapshots_dir)?;

        Self::new_snapshot_package(
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
    pub fn new_incremental_snapshot_package<P, Q>(
        bank: &Bank,
        incremental_snapshot_base_slot: Slot,
        bank_snapshot_info: &BankSnapshotInfo,
        snapshots_dir: P,
        status_cache_slot_deltas: Vec<BankSlotDelta>,
        snapshot_package_output_path: Q,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
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

        Self::new_snapshot_package(
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

pub struct AccountsPackage {
    pub snapshot_archive_info: SnapshotArchiveInfo,
    pub block_height: Slot,
    pub slot_deltas: Vec<BankSlotDelta>,
    pub snapshot_links: TempDir,
    pub storages: SnapshotStorages,
    pub snapshot_version: SnapshotVersion,
}

impl AccountsPackage {
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
        }
    }
}

impl SnapshotArchiveInfoGetter for AccountsPackage {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo {
        &self.snapshot_archive_info
    }
}
