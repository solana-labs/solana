use {
    crate::{
        bank::Bank,
        snapshot_archive_info::{SnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_hash::SnapshotHash,
        snapshot_utils::{self, ArchiveFormat, BankSnapshotInfo, SnapshotVersion},
    },
    log::*,
    solana_accounts_db::{
        accounts::Accounts,
        accounts_db::{AccountStorageEntry, IncludeSlotInHash, INCLUDE_SLOT_IN_HASH_TESTS},
        accounts_hash::{AccountsHash, AccountsHashKind},
        epoch_accounts_hash::EpochAccountsHash,
        rent_collector::RentCollector,
    },
    solana_sdk::{clock::Slot, feature_set, sysvar::epoch_schedule::EpochSchedule},
    std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Instant,
    },
};

mod compare;
pub use compare::*;

/// This struct packages up fields to send from AccountsBackgroundService to AccountsHashVerifier
pub struct AccountsPackage {
    pub package_kind: AccountsPackageKind,
    pub slot: Slot,
    pub block_height: Slot,
    pub snapshot_storages: Vec<Arc<AccountStorageEntry>>,
    pub expected_capitalization: u64,
    pub accounts_hash_for_testing: Option<AccountsHash>,
    pub accounts: Arc<Accounts>,
    pub epoch_schedule: EpochSchedule,
    pub rent_collector: RentCollector,
    pub is_incremental_accounts_hash_feature_enabled: bool,
    pub include_slot_in_hash: IncludeSlotInHash,

    /// Supplemental information needed for snapshots
    pub snapshot_info: Option<SupplementalSnapshotInfo>,

    /// The instant this accounts package was send to the queue.
    /// Used to track how long accounts packages wait before processing.
    pub enqueued: Instant,
}

impl AccountsPackage {
    /// Package up bank files, storages, and slot deltas for a snapshot
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_snapshot(
        package_kind: AccountsPackageKind,
        bank: &Bank,
        bank_snapshot_info: &BankSnapshotInfo,
        full_snapshot_archives_dir: impl AsRef<Path>,
        incremental_snapshot_archives_dir: impl AsRef<Path>,
        snapshot_storages: Vec<Arc<AccountStorageEntry>>,
        archive_format: ArchiveFormat,
        snapshot_version: SnapshotVersion,
        accounts_hash_for_testing: Option<AccountsHash>,
    ) -> Self {
        if let AccountsPackageKind::Snapshot(snapshot_kind) = package_kind {
            info!(
                "Package snapshot for bank {} has {} account storage entries (snapshot kind: {:?})",
                bank.slot(),
                snapshot_storages.len(),
                snapshot_kind,
            );
            if let SnapshotKind::IncrementalSnapshot(incremental_snapshot_base_slot) = snapshot_kind
            {
                assert!(
                    bank.slot() > incremental_snapshot_base_slot,
                    "Incremental snapshot base slot must be less than the bank being snapshotted!"
                );
            }
        }

        let snapshot_info = SupplementalSnapshotInfo {
            bank_snapshot_dir: bank_snapshot_info.snapshot_dir.clone(),
            archive_format,
            snapshot_version,
            full_snapshot_archives_dir: full_snapshot_archives_dir.as_ref().to_path_buf(),
            incremental_snapshot_archives_dir: incremental_snapshot_archives_dir
                .as_ref()
                .to_path_buf(),
            epoch_accounts_hash: bank.get_epoch_accounts_hash_to_serialize(),
        };
        Self::_new(
            package_kind,
            bank,
            snapshot_storages,
            accounts_hash_for_testing,
            Some(snapshot_info),
        )
    }

    /// Package up fields needed to verify an accounts hash
    #[must_use]
    pub fn new_for_accounts_hash_verifier(
        package_kind: AccountsPackageKind,
        bank: &Bank,
        snapshot_storages: Vec<Arc<AccountStorageEntry>>,
        accounts_hash_for_testing: Option<AccountsHash>,
    ) -> Self {
        assert_eq!(package_kind, AccountsPackageKind::AccountsHashVerifier);
        Self::_new(
            package_kind,
            bank,
            snapshot_storages,
            accounts_hash_for_testing,
            None,
        )
    }

    /// Package up fields needed to compute an EpochAccountsHash
    #[must_use]
    pub fn new_for_epoch_accounts_hash(
        package_kind: AccountsPackageKind,
        bank: &Bank,
        snapshot_storages: Vec<Arc<AccountStorageEntry>>,
        accounts_hash_for_testing: Option<AccountsHash>,
    ) -> Self {
        assert_eq!(package_kind, AccountsPackageKind::EpochAccountsHash);
        Self::_new(
            package_kind,
            bank,
            snapshot_storages,
            accounts_hash_for_testing,
            None,
        )
    }

    fn _new(
        package_kind: AccountsPackageKind,
        bank: &Bank,
        snapshot_storages: Vec<Arc<AccountStorageEntry>>,
        accounts_hash_for_testing: Option<AccountsHash>,
        snapshot_info: Option<SupplementalSnapshotInfo>,
    ) -> Self {
        let is_incremental_accounts_hash_feature_enabled = bank
            .feature_set
            .is_active(&feature_set::incremental_snapshot_only_incremental_hash_calculation::id());
        Self {
            package_kind,
            slot: bank.slot(),
            block_height: bank.block_height(),
            snapshot_storages,
            expected_capitalization: bank.capitalization(),
            accounts_hash_for_testing,
            accounts: bank.accounts(),
            epoch_schedule: *bank.epoch_schedule(),
            rent_collector: bank.rent_collector().clone(),
            is_incremental_accounts_hash_feature_enabled,
            include_slot_in_hash: bank.include_slot_in_hash(),
            snapshot_info,
            enqueued: Instant::now(),
        }
    }

    /// Create a new Accounts Package where basically every field is defaulted.
    /// Only use for tests; many of the fields are invalid!
    pub fn default_for_tests() -> Self {
        Self {
            package_kind: AccountsPackageKind::AccountsHashVerifier,
            slot: Slot::default(),
            block_height: Slot::default(),
            snapshot_storages: Vec::default(),
            expected_capitalization: u64::default(),
            accounts_hash_for_testing: Option::default(),
            accounts: Arc::new(Accounts::default_for_tests()),
            epoch_schedule: EpochSchedule::default(),
            rent_collector: RentCollector::default(),
            is_incremental_accounts_hash_feature_enabled: bool::default(),
            include_slot_in_hash: INCLUDE_SLOT_IN_HASH_TESTS,
            snapshot_info: Some(SupplementalSnapshotInfo {
                bank_snapshot_dir: PathBuf::default(),
                archive_format: ArchiveFormat::Tar,
                snapshot_version: SnapshotVersion::default(),
                full_snapshot_archives_dir: PathBuf::default(),
                incremental_snapshot_archives_dir: PathBuf::default(),
                epoch_accounts_hash: Option::default(),
            }),
            enqueued: Instant::now(),
        }
    }

    /// Returns the path to the snapshot dir
    ///
    /// NOTE: This fn will panic if the AccountsPackage is of type EpochAccountsHash.
    pub fn bank_snapshot_dir(&self) -> &Path {
        match self.package_kind {
            AccountsPackageKind::AccountsHashVerifier | AccountsPackageKind::Snapshot(..) => self
                .snapshot_info
                .as_ref()
                .unwrap()
                .bank_snapshot_dir
                .as_path(),
            AccountsPackageKind::EpochAccountsHash => {
                panic!("EAH accounts packages do not contain snapshot information")
            }
        }
    }
}

impl std::fmt::Debug for AccountsPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountsPackage")
            .field("kind", &self.package_kind)
            .field("slot", &self.slot)
            .field("block_height", &self.block_height)
            .finish_non_exhaustive()
    }
}

/// Supplemental information needed for snapshots
pub struct SupplementalSnapshotInfo {
    pub bank_snapshot_dir: PathBuf,
    pub archive_format: ArchiveFormat,
    pub snapshot_version: SnapshotVersion,
    pub full_snapshot_archives_dir: PathBuf,
    pub incremental_snapshot_archives_dir: PathBuf,
    pub epoch_accounts_hash: Option<EpochAccountsHash>,
}

/// Accounts packages are sent to the Accounts Hash Verifier for processing.  There are multiple
/// types of accounts packages, which are specified as variants in this enum.  All accounts
/// packages do share some processing: such as calculating the accounts hash.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AccountsPackageKind {
    AccountsHashVerifier,
    Snapshot(SnapshotKind),
    EpochAccountsHash,
}

/// This struct packages up fields to send from AccountsHashVerifier to SnapshotPackagerService
pub struct SnapshotPackage {
    pub snapshot_archive_info: SnapshotArchiveInfo,
    pub block_height: Slot,
    pub bank_snapshot_dir: PathBuf,
    pub snapshot_storages: Vec<Arc<AccountStorageEntry>>,
    pub snapshot_version: SnapshotVersion,
    pub snapshot_kind: SnapshotKind,

    /// The instant this snapshot package was sent to the queue.
    /// Used to track how long snapshot packages wait before handling.
    pub enqueued: Instant,
}

impl SnapshotPackage {
    pub fn new(accounts_package: AccountsPackage, accounts_hash: AccountsHashKind) -> Self {
        let AccountsPackageKind::Snapshot(snapshot_kind) = accounts_package.package_kind else {
            panic!(
                "The AccountsPackage must be of kind Snapshot in order to make a SnapshotPackage!"
            );
        };
        let Some(snapshot_info) = accounts_package.snapshot_info else {
            panic!(
                "The AccountsPackage must have snapshot info in order to make a SnapshotPackage!"
            );
        };
        let snapshot_hash =
            SnapshotHash::new(&accounts_hash, snapshot_info.epoch_accounts_hash.as_ref());
        let mut snapshot_storages = accounts_package.snapshot_storages;
        let snapshot_archive_path = match snapshot_kind {
            SnapshotKind::FullSnapshot => snapshot_utils::build_full_snapshot_archive_path(
                snapshot_info.full_snapshot_archives_dir,
                accounts_package.slot,
                &snapshot_hash,
                snapshot_info.archive_format,
            ),
            SnapshotKind::IncrementalSnapshot(incremental_snapshot_base_slot) => {
                snapshot_storages.retain(|storage| storage.slot() > incremental_snapshot_base_slot);
                assert!(
                    snapshot_storages.iter().all(|storage| storage.slot() > incremental_snapshot_base_slot),
                    "Incremental snapshot package must only contain storage entries where slot > incremental snapshot base slot (i.e. full snapshot slot)!"
                );
                snapshot_utils::build_incremental_snapshot_archive_path(
                    snapshot_info.incremental_snapshot_archives_dir,
                    incremental_snapshot_base_slot,
                    accounts_package.slot,
                    &snapshot_hash,
                    snapshot_info.archive_format,
                )
            }
        };

        Self {
            snapshot_archive_info: SnapshotArchiveInfo {
                path: snapshot_archive_path,
                slot: accounts_package.slot,
                hash: snapshot_hash,
                archive_format: snapshot_info.archive_format,
            },
            block_height: accounts_package.block_height,
            bank_snapshot_dir: snapshot_info.bank_snapshot_dir,
            snapshot_storages,
            snapshot_version: snapshot_info.snapshot_version,
            snapshot_kind,
            enqueued: Instant::now(),
        }
    }
}

impl std::fmt::Debug for SnapshotPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotPackage")
            .field("type", &self.snapshot_kind)
            .field("slot", &self.slot())
            .field("block_height", &self.block_height)
            .finish_non_exhaustive()
    }
}

impl SnapshotArchiveInfoGetter for SnapshotPackage {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo {
        &self.snapshot_archive_info
    }
}

/// Snapshots come in two kinds, Full and Incremental.  The IncrementalSnapshot has a Slot field,
/// which is the incremental snapshot base slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotKind {
    FullSnapshot,
    IncrementalSnapshot(Slot),
}

impl SnapshotKind {
    pub fn is_full_snapshot(&self) -> bool {
        matches!(self, SnapshotKind::FullSnapshot)
    }
    pub fn is_incremental_snapshot(&self) -> bool {
        matches!(self, SnapshotKind::IncrementalSnapshot(_))
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
