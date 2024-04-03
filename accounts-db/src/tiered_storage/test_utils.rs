#![cfg(test)]
//! Helper functions for TieredStorage tests
use {
    super::footer::TieredStorageFooter,
    crate::{
        account_storage::meta::{StoredAccountMeta, StoredMeta},
        accounts_hash::AccountHash,
        tiered_storage::owners::OWNER_NO_OWNER,
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        hash::Hash,
        pubkey::Pubkey,
        rent_collector::RENT_EXEMPT_RENT_EPOCH,
    },
};

/// Create a test account based on the specified seed.
/// The created test account might have default rent_epoch
/// and write_version.
///
/// When the seed is zero, then a zero-lamport test account will be
/// created.
pub(super) fn create_test_account(seed: u64) -> (StoredMeta, AccountSharedData) {
    let data_byte = seed as u8;
    let owner_byte = u8::MAX - data_byte;
    let account = Account {
        lamports: seed,
        data: std::iter::repeat(data_byte).take(seed as usize).collect(),
        // this will allow some test account sharing the same owner.
        owner: [owner_byte; 32].into(),
        executable: seed % 2 > 0,
        rent_epoch: if seed % 3 > 0 {
            seed
        } else {
            RENT_EXEMPT_RENT_EPOCH
        },
    };

    let stored_meta = StoredMeta {
        write_version_obsolete: u64::MAX,
        pubkey: Pubkey::new_unique(),
        data_len: seed,
    };
    (stored_meta, AccountSharedData::from(account))
}

pub(super) fn verify_test_account(
    stored_meta: &StoredAccountMeta<'_>,
    account: Option<&impl ReadableAccount>,
    address: &Pubkey,
) {
    let (lamports, owner, data, executable) = account
        .map(|acc| (acc.lamports(), acc.owner(), acc.data(), acc.executable()))
        .unwrap_or((0, &OWNER_NO_OWNER, &[], false));

    assert_eq!(stored_meta.lamports(), lamports);
    assert_eq!(stored_meta.data().len(), data.len());
    assert_eq!(stored_meta.data(), data);
    assert_eq!(stored_meta.executable(), executable);
    assert_eq!(stored_meta.owner(), owner);
    assert_eq!(stored_meta.pubkey(), address);
    assert_eq!(*stored_meta.hash(), AccountHash(Hash::default()));
}

pub(super) fn verify_test_account_with_footer(
    stored_meta: &StoredAccountMeta<'_>,
    account: Option<&impl ReadableAccount>,
    address: &Pubkey,
    footer: &TieredStorageFooter,
) {
    verify_test_account(stored_meta, account, address);
    assert!(footer.min_account_address <= *address);
    assert!(footer.max_account_address >= *address);
}
