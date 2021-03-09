use crate::account::{AccountSharedData, WritableAccount};

crate::declare_id!("NativeLoader1111111111111111111111111111111");

/// Create an executable account with the given shared object name.
pub fn create_loadable_account(name: &str, lamports: u64) -> AccountSharedData {
    AccountSharedData::create(lamports, name.as_bytes().to_vec(), id(), true, 0)
}
