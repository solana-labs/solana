use {
    super::{AccountsPackage, AccountsPackageType, SnapshotType},
    std::cmp::Ordering::{self, Equal, Greater, Less},
};

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
/// If two `Snapshot`s are compared, their snapshot types are the tiebreaker.
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
        (Snapshot(snapshot_type_a), Snapshot(snapshot_type_b)) => {
            cmp_snapshot_types_by_priority(snapshot_type_a, snapshot_type_b)
        }
        (Snapshot(_), _) => Greater,
        (_, Snapshot(_)) => Less,

        // Accounts Hash Verifier packages
        (AccountsHashVerifier, AccountsHashVerifier) => Equal,
    }
}

/// Compare snapshot types by priority
///
/// Full snapshots are higher in priority than incremental snapshots.
/// If two `IncrementalSnapshot`s are compared, their base slots are the tiebreaker.
#[must_use]
pub fn cmp_snapshot_types_by_priority(a: &SnapshotType, b: &SnapshotType) -> Ordering {
    use SnapshotType::*;
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
    use {super::*, solana_sdk::clock::Slot};

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
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    123,
                ),
                Greater,
            ),
            (
                new(AccountsPackageType::EpochAccountsHash, 123),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
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
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    123,
                ),
                new(AccountsPackageType::EpochAccountsHash, 123),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    11,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    22,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    22,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    22,
                ),
                Equal,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    33,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    22,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    123,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    123,
                ),
                new(AccountsPackageType::AccountsHashVerifier, 123),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    123,
                ),
                new(AccountsPackageType::EpochAccountsHash, 123),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                    123,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(6)),
                    123,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    11,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    22,
                ),
                Less,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    22,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    22,
                ),
                Equal,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    33,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    22,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                    123,
                ),
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(4)),
                    123,
                ),
                Greater,
            ),
            (
                new(
                    AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
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
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                Greater,
            ),
            (
                AccountsPackageType::EpochAccountsHash,
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                Greater,
            ),
            (
                AccountsPackageType::EpochAccountsHash,
                AccountsPackageType::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                AccountsPackageType::EpochAccountsHash,
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                Equal,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                AccountsPackageType::AccountsHashVerifier,
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                AccountsPackageType::EpochAccountsHash,
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(6)),
                Less,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                Equal,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(4)),
                Greater,
            ),
            (
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(5)),
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
    fn test_cmp_snapshot_types_by_priority() {
        for (snapshot_type_a, snapshot_type_b, expected_result) in [
            (
                SnapshotType::FullSnapshot,
                SnapshotType::FullSnapshot,
                Equal,
            ),
            (
                SnapshotType::FullSnapshot,
                SnapshotType::IncrementalSnapshot(5),
                Greater,
            ),
            (
                SnapshotType::IncrementalSnapshot(5),
                SnapshotType::FullSnapshot,
                Less,
            ),
            (
                SnapshotType::IncrementalSnapshot(5),
                SnapshotType::IncrementalSnapshot(6),
                Less,
            ),
            (
                SnapshotType::IncrementalSnapshot(5),
                SnapshotType::IncrementalSnapshot(5),
                Equal,
            ),
            (
                SnapshotType::IncrementalSnapshot(5),
                SnapshotType::IncrementalSnapshot(4),
                Greater,
            ),
        ] {
            let actual_result = cmp_snapshot_types_by_priority(&snapshot_type_a, &snapshot_type_b);
            assert_eq!(expected_result, actual_result);
        }
    }
}
