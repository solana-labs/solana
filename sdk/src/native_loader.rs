use crate::account::Account;
use crate::hash::Hash;

const ID: [u8; 32] = [
    5, 135, 132, 191, 20, 139, 164, 40, 47, 176, 18, 87, 72, 136, 169, 241, 83, 160, 125, 173, 247,
    101, 192, 69, 92, 154, 151, 3, 128, 0, 0, 0,
];

crate::solana_name_id!(ID, "NativeLoader1111111111111111111111111111111");

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
