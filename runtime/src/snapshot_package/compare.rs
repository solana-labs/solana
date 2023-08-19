use {
    super::{
        AccountsPackage, AccountsPackageType, SnapshotArchiveInfoGetter, SnapshotKind,
        SnapshotPackage,
    },
    std::cmp::Ordering::{self, Equal, Greater, Less},
};

/// Compare snapshot packages by priority; first by type, then by slot
#[must_use]
pub fn cmp_snapshot_packages_by_priority(a: &SnapshotPackage, b: &SnapshotPackage) -> Ordering {
    cmp_snapshot_kinds_by_priority(&a.snapshot_kind, &b.snapshot_kind).then(a.slot().cmp(&b.slot()))
}

/// Compare accounts packages by priority; first by type, then by slot
#[must_use]
pub fn cmp_accounts_packages_by_priority(a: &AccountsPackage, b: &AccountsPackage) -> Ordering {
    cmp_accounts_package_types_by_priority(&a.package_type, &b.package_type)
        .then(a.slot.cmp(&b.slot))
}

/// Compare accounts package types by priority
///
/// Priority, from highest to lowest:
/// - Epoch Accounts Hash
/// - Full Snapshot
/// - Incremental Snapshot
/// - Accounts Hash Verifier
///
/// If two `Snapshot`s are compared, their snapshot kinds are the tiebreaker.
#[must_use]
pub fn cmp_accounts_package_types_by_priority(
    a: &AccountsPackageType,
    b: &AccountsPackageType,
) -> Ordering {
    use AccountsPackageType::*;
    match (a, b) {
        // Epoch Accounts Hash packages
        (EpochAccountsHash, EpochAccountsHash) => Equal,
        (EpochAccountsHash, _) => Greater,
        (_, EpochAccountsHash) => Less,

        // Snapshot packages
        (Snapshot(snapshot_kind_a), Snapshot(snapshot_kind_b)) => {
            cmp_snapshot_kinds_by_priority(snapshot_kind_a, snapshot_kind_b)
        }
        (Snapshot(_), _) => Greater,
        (_, Snapshot(_)) => Less,

        // Accounts Hash Verifier packages
        (AccountsHashVerifier, AccountsHashVerifier) => Equal,
    }
}

/// Compare snapshot kinds by priority
///
/// Full snapshots are higher in priority than incremental snapshots.
/// If two `IncrementalSnapshot`s are compared, their base slots are the tiebreaker.
#[must_use]
pub fn cmp_snapshot_kinds_by_priority(a: &SnapshotKind, b: &SnapshotKind) -> Ordering {
    use SnapshotKind::*;
    match (a, b) {
        (FullSnapshot, FullSnapshot) => Equal,
        (FullSnapshot, IncrementalSnapshot(_)) => Greater,
        (IncrementalSnapshot(_), FullSnapshot) => Less,
        (IncrementalSnapshot(base_slot_a), IncrementalSnapshot(base_slot_b)) => {
            base_slot_a.cmp(base_slot_b)
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            snapshot_archive_info::SnapshotArchiveInfo,
            snapshot_hash::SnapshotHash,
            snapshot_utils::{ArchiveFormat, SnapshotVersion},
        },
        solana_sdk::{clock::Slot, hash::Hash},
        std::{path::PathBuf, time::Instant},
    };

    #[test]
    fn test_cmp_snapshot_packages_by_priority() {
        fn new(snapshot_kind: SnapshotKind, slot: Slot) -> SnapshotPackage {
            SnapshotPackage {
                snapshot_archive_info: SnapshotArchiveInfo {
                    path: PathBuf::default(),
                    slot,
                    hash: SnapshotHash(Hash::default()),
                    archive_format: ArchiveFormat::Tar,
                },
                block_height: slot,
                bank_snapshot_dir: PathBuf::default(),
                snapshot_storages: Vec::default(),
                snapshot_version: SnapshotVersion::default(),
                snapshot_kind,
                enqueued: Instant::now(),
            }
        }

        for (snapshot_package_a, snapshot_package_b, expected_result) in [
            (
                new(SnapshotKind::FullSnapshot, 11),
                new(SnapshotKind::FullSnapshot, 22),
                Less,
            ),
            (
                new(SnapshotKind::FullSnapshot, 22),
                new(SnapshotKind::FullSnapshot, 22),
                Equal,
            ),
            (
                new(SnapshotKind::FullSnapshot, 33),
                new(SnapshotKind::FullSnapshot, 22),
                Greater,
            ),
            (
                new(SnapshotKind::FullSnapshot, 22),
                new(SnapshotKind::IncrementalSnapshot(88), 99),
                Greater,
            ),
            (
                new(SnapshotKind::IncrementalSnapshot(11), 55),
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                Less,
            ),
            (
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                Equal,
            ),
            (
                new(SnapshotKind::IncrementalSnapshot(33), 55),
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                Greater,
            ),
            (
                new(SnapshotKind::IncrementalSnapshot(22), 44),
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                Less,
            ),
            (
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                Equal,
            ),
            (
                new(SnapshotKind::IncrementalSnapshot(22), 66),
                new(SnapshotKind::IncrementalSnapshot(22), 55),
                Greater,
            ),
        ] {
            let actual_result =
                cmp_snapshot_packages_by_priority(&snapshot_package_a, &snapshot_package_b);
            assert_eq!(expected_result, actual_result);
        }
    }

    #[test]
    fn test_cmp_accounts_packages_by_priority() {
        fn new(package_type: AccountsPackageType, slot: Slot) -> AccountsPackage {
            AccountsPackage {
                package_type,
                slot,
                block_height: slot,
                ..AccountsPackage::default_for_tests()
            }
        }

        for (accounts_package_a, accounts_package_b, expected_result) in [
            (
                new(AccountsPackageType::EpochAccountsHash, 11),
                new(AccountsPackageType::EpochAccountsHash, 22),
                Less,
            ),
            (
                new(AccountsPackageType::EpochAccountsHash, 22),
                new(AccountsPackageType::EpochAccountsHash, 22),
                Equal,
            ),
            (
                new(AccountsPackageType::EpochAccountsHash, 33),
                new(AccountsPackageType::EpochAccountsHash, 22),
                Greater,
            ),
            (
                new(AccountsPackageType::EpochAccountsHash, 123),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                Greater,
            ),
            (
                new(AccountsPackageType::EpochAccountsHash, 123),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                Greater,
            ),
            (
                new(AccountsPackageType::EpochAccountsHash, 123),
                new(AccountsPackageType::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                new(AccountsPackageType::EpochAccountsHash, 123),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    11,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                Equal,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    33,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                new(AccountsPackageType::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(AccountsPackageType::EpochAccountsHash, 123),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(6)),
                    123,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    11,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                Equal,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    33,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(4)),
                    123,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(AccountsPackageType::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(AccountsPackageType::AccountsHashVerifier, 11),
                new(AccountsPackageType::AccountsHashVerifier, 22),
                Less,
            ),
            (
                new(AccountsPackageType::AccountsHashVerifier, 22),
                new(AccountsPackageType::AccountsHashVerifier, 22),
                Equal,
            ),
            (
                new(AccountsPackageType::AccountsHashVerifier, 33),
                new(AccountsPackageType::AccountsHashVerifier, 22),
                Greater,
            ),
        ] {
            let actual_result =
                cmp_accounts_packages_by_priority(&accounts_package_a, &accounts_package_b);
            assert_eq!(expected_result, actual_result);
        }
    }

    #[test]
    fn test_cmp_accounts_package_types_by_priority() {
        for (accounts_package_type_a, accounts_package_type_b, expected_result) in [
            (
                AccountsPackageType::EpochAccountsHash,
                AccountsPackageType::EpochAccountsHash,
                Equal,
            ),
            (
                AccountsPackageType::EpochAccountsHash,
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                Greater,
            ),
            (
                AccountsPackageType::EpochAccountsHash,
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                Greater,
            ),
            (
                AccountsPackageType::EpochAccountsHash,
                AccountsPackageType::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageType::EpochAccountsHash,
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                Equal,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageType::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageType::EpochAccountsHash,
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotKind::FullSnapshot),
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(6)),
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                Equal,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(4)),
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageType::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageType::AccountsHashVerifier,
                AccountsPackageType::AccountsHashVerifier,
                Equal,
            ),
        ] {
            let actual_result = cmp_accounts_package_types_by_priority(
                &accounts_package_type_a,
                &accounts_package_type_b,
            );
            assert_eq!(expected_result, actual_result);
        }
    }

    #[test]
    fn test_cmp_snapshot_kinds_by_priority() {
        for (snapshot_kind_a, snapshot_kind_b, expected_result) in [
            (
                SnapshotKind::FullSnapshot,
                SnapshotKind::FullSnapshot,
                Equal,
            ),
            (
                SnapshotKind::FullSnapshot,
                SnapshotKind::IncrementalSnapshot(5),
                Greater,
            ),
            (
                SnapshotKind::IncrementalSnapshot(5),
                SnapshotKind::FullSnapshot,
                Less,
            ),
            (
                SnapshotKind::IncrementalSnapshot(5),
                SnapshotKind::IncrementalSnapshot(6),
                Less,
            ),
            (
                SnapshotKind::IncrementalSnapshot(5),
                SnapshotKind::IncrementalSnapshot(5),
                Equal,
            ),
            (
                SnapshotKind::IncrementalSnapshot(5),
                SnapshotKind::IncrementalSnapshot(4),
                Greater,
            ),
        ] {
            let actual_result = cmp_snapshot_kinds_by_priority(&snapshot_kind_a, &snapshot_kind_b);
            assert_eq!(expected_result, actual_result);
        }
    }
}
