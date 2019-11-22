use crate::account::Account;
use crate::hash::Hash;

crate::declare_id!("NativeLoader1111111111111111111111111111111");

/// Create an executable account with the given shared object name.
pub fn create_loadable_account(name: &str) -> Account {
    Account {
        lamports: 1,
        owner: id(),
        data: name.as_bytes().to_vec(),
        executable: true,
        rent_epoch: 0,
        hash: Hash::default(),
    }
}
