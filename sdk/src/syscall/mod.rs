//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::pubkey::Pubkey;

pub mod current;
pub mod fees;
pub mod rewards;
pub mod slot_hashes;

pub fn is_syscall_id(id: &Pubkey) -> bool {
    current::check_id(id)
        || fees::check_id(id)
        || rewards::check_id(id)
        || slot_hashes::check_id(id)
}

/// "Sysca11111111111111111111111111111111111111"
///   owner pubkey for syscall accounts
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 216, 137, 185, 198, 189, 33, 204, 111, 12, 217, 220, 229, 201, 34, 52,
    253, 202, 87, 144, 232, 16, 195, 192, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "Sysca11111111111111111111111111111111111111");
