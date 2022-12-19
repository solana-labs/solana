//! The native loader native program.

use crate::{
    account::{
        Account, AccountSharedData, InheritableAccountFields, DUMMY_INHERITABLE_ACCOUNT_FIELDS,
    },
    clock::INITIAL_RENT_EPOCH,
};

crate::declare_id!("NativeLoader1111111111111111111111111111111");

/// Create an executable account with the given shared object name.
#[deprecated(
    since = "1.5.17",
    note = "Please use `create_loadable_account_for_test` instead"
)]
pub fn create_loadable_account(name: &str, lamports: u64) -> AccountSharedData {
    create_loadable_account_with_fields(name, (lamports, INITIAL_RENT_EPOCH, false, 0))
}

pub fn create_loadable_account_with_fields(
    name: &str,
    (lamports, rent_epoch, has_application_fees, application_fees): InheritableAccountFields,
) -> AccountSharedData {
    AccountSharedData::from(Account {
        lamports,
        owner: id(),
        data: name.as_bytes().to_vec(),
        executable: true,
        has_application_fees: has_application_fees,
        rent_epoch_or_application_fees: if has_application_fees {
            application_fees
        } else {
            rent_epoch
        },
    })
}

pub fn create_loadable_account_for_test(name: &str) -> AccountSharedData {
    create_loadable_account_with_fields(name, DUMMY_INHERITABLE_ACCOUNT_FIELDS)
}
