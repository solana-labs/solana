use crate::account::AccountSharedData;
use std::sync::Arc;

crate::declare_id!("NativeLoader1111111111111111111111111111111");

/// Create an executable account with the given shared object name.
pub fn create_loadable_account(name: &str, lamports: u64) -> AccountSharedData {
    AccountSharedData {
        lamports,
        owner: id(),
        data: Arc::new(name.as_bytes().to_vec()),
        executable: true,
        rent_epoch: 0,
    }
}
