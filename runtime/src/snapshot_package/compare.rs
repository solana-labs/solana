use {
    super::{
        AccountsPackage, AccountsPackageKind, SnapshotArchiveInfoGetter, SnapshotKind,
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
    cmp_accounts_package_kinds_by_priority(&a.package_kind, &b.package_kind)
        .then(a.slot.cmp(&b.slot))
}

/// Compare accounts package kinds by priority
///
/// Priority, from highest to lowest:
/// - Epoch Accounts Hash
/// - Full Snapshot
/// - Incremental Snapshot
/// - Accounts Hash Verifier
///
/// If two `Snapshot`s are compared, their snapshot kinds are the tiebreaker.
#[must_use]
pub fn cmp_accounts_package_kinds_by_priority(
    a: &AccountsPackageKind,
    b: &AccountsPackageKind,
) -> Ordering {
    use AccountsPackageKind::*;
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
        fn new(package_kind: AccountsPackageKind, slot: Slot) -> AccountsPackage {
            AccountsPackage {
                package_kind,
                slot,
                block_height: slot,
                ..AccountsPackage::default_for_tests()
            }
        }

        for (accounts_package_a, accounts_package_b, expected_result) in [
            (
                new(AccountsPackageKind::EpochAccountsHash, 11),
                new(AccountsPackageKind::EpochAccountsHash, 22),
                Less,
            ),
            (
                new(AccountsPackageKind::EpochAccountsHash, 22),
                new(AccountsPackageKind::EpochAccountsHash, 22),
                Equal,
            ),
            (
                new(AccountsPackageKind::EpochAccountsHash, 33),
                new(AccountsPackageKind::EpochAccountsHash, 22),
                Greater,
            ),
            (
                new(AccountsPackageKind::EpochAccountsHash, 123),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                Greater,
            ),
            (
                new(AccountsPackageKind::EpochAccountsHash, 123),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                Greater,
            ),
            (
                new(AccountsPackageKind::EpochAccountsHash, 123),
                new(AccountsPackageKind::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                new(AccountsPackageKind::EpochAccountsHash, 123),
                Less,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    11,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                Equal,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    33,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    22,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                new(AccountsPackageKind::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(AccountsPackageKind::EpochAccountsHash, 123),
                Less,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                    123,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(6)),
                    123,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    11,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                Equal,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    33,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    22,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(4)),
                    123,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                    123,
                ),
                new(AccountsPackageKind::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(AccountsPackageKind::AccountsHashVerifier, 11),
                new(AccountsPackageKind::AccountsHashVerifier, 22),
                Less,
            ),
            (
                new(AccountsPackageKind::AccountsHashVerifier, 22),
                new(AccountsPackageKind::AccountsHashVerifier, 22),
                Equal,
            ),
            (
                new(AccountsPackageKind::AccountsHashVerifier, 33),
                new(AccountsPackageKind::AccountsHashVerifier, 22),
                Greater,
            ),
        ] {
            let actual_result =
                cmp_accounts_packages_by_priority(&accounts_package_a, &accounts_package_b);
            assert_eq!(expected_result, actual_result);
        }
    }

    #[test]
    fn test_cmp_accounts_package_kinds_by_priority() {
        for (accounts_package_kind_a, accounts_package_kind_b, expected_result) in [
            (
                AccountsPackageKind::EpochAccountsHash,
                AccountsPackageKind::EpochAccountsHash,
                Equal,
            ),
            (
                AccountsPackageKind::EpochAccountsHash,
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                Greater,
            ),
            (
                AccountsPackageKind::EpochAccountsHash,
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                Greater,
            ),
            (
                AccountsPackageKind::EpochAccountsHash,
                AccountsPackageKind::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageKind::EpochAccountsHash,
                Less,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                Equal,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                Greater,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                AccountsPackageKind::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageKind::EpochAccountsHash,
                Less,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
                Less,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(6)),
                Less,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                Equal,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(4)),
                Greater,
            ),
            (
                AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(5)),
                AccountsPackageKind::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageKind::AccountsHashVerifier,
                AccountsPackageKind::AccountsHashVerifier,
                Equal,
            ),
        ] {
            let actual_result = cmp_accounts_package_kinds_by_priority(
                &accounts_package_kind_a,
                &accounts_package_kind_b,
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
