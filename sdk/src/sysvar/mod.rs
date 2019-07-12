//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::pubkey::Pubkey;

pub mod clock;
pub mod fees;
pub mod rewards;
pub mod slot_hashes;

pub fn is_sysvar_id(id: &Pubkey) -> bool {
    clock::check_id(id) || fees::check_id(id) || rewards::check_id(id) || slot_hashes::check_id(id)
}

/// "Sysvar1111111111111111111111111111111111111"
///   owner pubkey for sysvar accounts
const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 117, 247, 41, 199, 61, 147, 64, 143, 33, 97, 32, 6, 126, 216, 140, 118,
    224, 140, 40, 127, 193, 148, 96, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "Sysvar1111111111111111111111111111111111111");
