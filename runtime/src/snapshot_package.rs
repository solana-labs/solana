use {
    crate::{
        accounts_db::SnapshotStorages,
        bank::{Bank, BankSlotDelta},
        snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::{
            self, ArchiveFormat, BankSnapshotInfo, Result, SnapshotVersion,
            TMP_BANK_SNAPSHOT_PREFIX,
        },
    },
    log::*,
    solana_sdk::{clock::Slot, genesis_config::ClusterType, hash::Hash},
    std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            mpsc::{Receiver, SendError, Sender},
            Arc, Mutex,
        },
    },
    tempfile::TempDir,
};

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
    pub snapshot_storages: SnapshotStorages,
    pub hash: Hash, // temporarily here while we still have to calculate hash before serializing bank
    pub archive_format: ArchiveFormat,
    pub snapshot_version: SnapshotVersion,
    pub snapshot_archives_dir: PathBuf,
    pub expected_capitalization: u64,
    pub hash_for_testing: Option<Hash>,
    pub cluster_type: ClusterType,
    pub snapshot_type: Option<SnapshotType>,
}

impl AccountsPackage {
    /// Package up bank files, storages, and slot deltas for a snapshot
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        bank_snapshots_dir: impl AsRef<Path>,
        slot_deltas: Vec<BankSlotDelta>,
        snapshot_archives_dir: impl AsRef<Path>,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        hash_for_testing: Option<Hash>,
        snapshot_type: Option<SnapshotType>,
    ) -> Result<Self> {
        info!(
            "Package snapshot for bank {} has {} account storage entries (snapshot type: {:?})",
            bank.slot(),
            snapshot_storages.len(),
            snapshot_type,
        );

        if let Some(SnapshotType::IncrementalSnapshot(incremental_snapshot_base_slot)) =
            snapshot_type
        {
            assert!(
                bank.slot() > incremental_snapshot_base_slot,
                "Incremental snapshot base slot must be less than the bank being snapshotted!"
            );
            assert!(
            snapshot_storages.iter().all(|storage| storage
                .iter()
                .all(|entry| entry.slot() > incremental_snapshot_base_slot)),
            "Incremental snapshot package must only contain storage entries where slot > incremental snapshot base slot (i.e. full snapshot slot)!"
            );
        }

        // Hard link the snapshot into a tmpdir, to ensure its not removed prior to packaging.
        let snapshot_links = tempfile::Builder::new()
            .prefix(&format!("{}{}-", TMP_BANK_SNAPSHOT_PREFIX, bank.slot()))
            .tempdir_in(bank_snapshots_dir)?;
        {
            let snapshot_hardlink_dir = snapshot_links
                .path()
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
            slot_deltas,
            snapshot_links,
            snapshot_storages,
            hash: bank.get_accounts_hash(),
            archive_format,
            snapshot_version,
            snapshot_archives_dir: snapshot_archives_dir.as_ref().to_path_buf(),
            expected_capitalization: bank.capitalization(),
            hash_for_testing,
            cluster_type: bank.cluster_type(),
            snapshot_type,
        })
    }
}

pub struct SnapshotPackage {
    pub snapshot_archive_info: SnapshotArchiveInfo,
    pub block_height: Slot,
    pub slot_deltas: Vec<BankSlotDelta>,
    pub snapshot_links: TempDir,
    pub snapshot_storages: SnapshotStorages,
    pub snapshot_version: SnapshotVersion,
    pub snapshot_type: SnapshotType,
}

impl From<AccountsPackage> for SnapshotPackage {
    fn from(accounts_package: AccountsPackage) -> Self {
        assert!(
            accounts_package.snapshot_type.is_some(),
            "Cannot make a SnapshotPackage from an AccountsPackage when SnapshotType is None!"
        );

        let snapshot_archive_path = match accounts_package.snapshot_type.unwrap() {
            SnapshotType::FullSnapshot => snapshot_utils::build_full_snapshot_archive_path(
                accounts_package.snapshot_archives_dir,
                accounts_package.slot,
                &accounts_package.hash,
                accounts_package.archive_format,
            ),
            SnapshotType::IncrementalSnapshot(incremental_snapshot_base_slot) => {
                snapshot_utils::build_incremental_snapshot_archive_path(
                    accounts_package.snapshot_archives_dir,
                    incremental_snapshot_base_slot,
                    accounts_package.slot,
                    &accounts_package.hash,
                    accounts_package.archive_format,
                )
            }
        };

        Self {
            snapshot_archive_info: SnapshotArchiveInfo {
                path: snapshot_archive_path,
                slot: accounts_package.slot,
                hash: accounts_package.hash,
                archive_format: accounts_package.archive_format,
            },
            block_height: accounts_package.block_height,
            slot_deltas: accounts_package.slot_deltas,
            snapshot_links: accounts_package.snapshot_links,
            snapshot_storages: accounts_package.snapshot_storages,
            snapshot_version: accounts_package.snapshot_version,
            snapshot_type: accounts_package.snapshot_type.unwrap(),
        }
    }
}

impl SnapshotArchiveInfoGetter for SnapshotPackage {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo {
        &self.snapshot_archive_info
    }
}

/// Snapshots come in two flavors, Full and Incremental.  The IncrementalSnapshot has a Slot field,
/// which is the incremental snapshot base slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotType {
    FullSnapshot,
    IncrementalSnapshot(Slot),
}

impl SnapshotType {
    pub fn is_full_snapshot(&self) -> bool {
        matches!(self, SnapshotType::FullSnapshot)
    }
    pub fn is_incremental_snapshot(&self) -> bool {
        matches!(self, SnapshotType::IncrementalSnapshot(_))
    }
}
