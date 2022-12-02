use {
    crate::{
        accounts::Accounts,
        accounts_db::SnapshotStorages,
        bank::{Bank, BankSlotDelta},
        rent_collector::RentCollector,
        snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_utils::{
            self, ArchiveFormat, BankSnapshotInfo, Result, SnapshotVersion,
            TMP_BANK_SNAPSHOT_PREFIX,
        },
    },
    log::*,
    solana_sdk::{
        clock::Slot, genesis_config::ClusterType, hash::Hash, sysvar::epoch_schedule::EpochSchedule,
    },
    std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    },
    tempfile::TempDir,
};

/// The PendingAccountsPackage passes an AccountsPackage from AccountsBackgroundService to
/// AccountsHashVerifier for hashing
pub type PendingAccountsPackage = Arc<Mutex<Option<AccountsPackage>>>;

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
    pub archive_format: ArchiveFormat,
    pub snapshot_version: SnapshotVersion,
    pub full_snapshot_archives_dir: PathBuf,
    pub incremental_snapshot_archives_dir: PathBuf,
    pub expected_capitalization: u64,
    pub accounts_hash_for_testing: Option<Hash>,
    pub cluster_type: ClusterType,
    pub snapshot_type: Option<SnapshotType>,
    pub accounts: Arc<Accounts>,
    pub epoch_schedule: EpochSchedule,
    pub rent_collector: RentCollector,
}

impl AccountsPackage {
    /// Package up bank files, storages, and slot deltas for a snapshot
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        bank_snapshots_dir: impl AsRef<Path>,
        slot_deltas: Vec<BankSlotDelta>,
        full_snapshot_archives_dir: impl AsRef<Path>,
        incremental_snapshot_archives_dir: impl AsRef<Path>,
        snapshot_storages: SnapshotStorages,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        accounts_hash_for_testing: Option<Hash>,
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
            let file_name =
                snapshot_utils::path_to_file_name_str(&bank_snapshot_info.snapshot_path)?;
            fs::hard_link(
                &bank_snapshot_info.snapshot_path,
                &snapshot_hardlink_dir.join(file_name),
            )?;
        }

        Ok(Self {
            slot: bank.slot(),
            block_height: bank.block_height(),
            slot_deltas,
            snapshot_links,
            snapshot_storages,
            archive_format,
            snapshot_version,
            full_snapshot_archives_dir: full_snapshot_archives_dir.as_ref().to_path_buf(),
            incremental_snapshot_archives_dir: incremental_snapshot_archives_dir
                .as_ref()
                .to_path_buf(),
            expected_capitalization: bank.capitalization(),
            accounts_hash_for_testing,
            cluster_type: bank.cluster_type(),
            snapshot_type,
            accounts: bank.accounts(),
            epoch_schedule: *bank.epoch_schedule(),
            rent_collector: bank.rent_collector().clone(),
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

impl SnapshotPackage {
    pub fn new(accounts_package: AccountsPackage, accounts_hash: Hash) -> Self {
        assert!(
            accounts_package.snapshot_type.is_some(),
            "Cannot make a SnapshotPackage from an AccountsPackage when SnapshotType is None!"
        );

        let mut snapshot_storages = accounts_package.snapshot_storages;
        let snapshot_archive_path = match accounts_package.snapshot_type.unwrap() {
            SnapshotType::FullSnapshot => snapshot_utils::build_full_snapshot_archive_path(
                accounts_package.full_snapshot_archives_dir,
                accounts_package.slot,
                &accounts_hash,
                accounts_package.archive_format,
            ),
            SnapshotType::IncrementalSnapshot(incremental_snapshot_base_slot) => {
                snapshot_storages.retain(|storages| {
                    storages
                        .first() // storages are grouped by slot in the outer Vec, so all storages will have the same slot as the first
                        .map(|storage| storage.slot() > incremental_snapshot_base_slot)
                        .unwrap_or_default()
                });
                assert!(
                    snapshot_storages.iter().all(|storage| storage
                        .iter()
                        .all(|entry| entry.slot() > incremental_snapshot_base_slot)),
                    "Incremental snapshot package must only contain storage entries where slot > incremental snapshot base slot (i.e. full snapshot slot)!"
                    );
                snapshot_utils::build_incremental_snapshot_archive_path(
                    accounts_package.incremental_snapshot_archives_dir,
                    incremental_snapshot_base_slot,
                    accounts_package.slot,
                    &accounts_hash,
                    accounts_package.archive_format,
                )
            }
        };

        Self {
            snapshot_archive_info: SnapshotArchiveInfo {
                path: snapshot_archive_path,
                slot: accounts_package.slot,
                hash: accounts_hash,
                archive_format: accounts_package.archive_format,
            },
            block_height: accounts_package.block_height,
            slot_deltas: accounts_package.slot_deltas,
            snapshot_links: accounts_package.snapshot_links,
            snapshot_storages,
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

/// Helper function to retain only max n of elements to the right of a vector,
/// viz. remove v.len() - n elements from the left of the vector.
#[inline(always)]
pub fn retain_max_n_elements<T>(v: &mut Vec<T>, n: usize) {
    if v.len() > n {
        let to_truncate = v.len() - n;
        v.rotate_left(to_truncate);
        v.truncate(n);
    }
}
