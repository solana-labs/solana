//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::pubkey::Pubkey;

pub mod clock;
pub mod epoch_schedule;
pub mod fees;
pub mod recent_blockhashes;
pub mod rent;
pub mod rewards;
pub mod slot_hashes;
pub mod stake_history;

pub fn is_sysvar_id(id: &Pubkey) -> bool {
    clock::check_id(id)
        || epoch_schedule::check_id(id)
        || fees::check_id(id)
        || recent_blockhashes::check_id(id)
        || rent::check_id(id)
        || rewards::check_id(id)
        || slot_hashes::check_id(id)
        || stake_history::check_id(id)
}

#[macro_export]
macro_rules! solana_sysvar_id(
    ($id:ident, $name:expr) => (
        $crate::solana_name_id!($id, $name);

        #[cfg(test)]
        #[test]
        fn test_sysvar_id() {
            if !$crate::sysvar::is_sysvar_id(&id()) {
                panic!("sysvar::is_sysvar_id() doesn't know about {}", $name);
            }
        }
    )
);

/// "Sysvar1111111111111111111111111111111111111"
///   owner pubkey for sysvar accounts
const ID: [u8; 32] = [
    6, 167, 213, 23, 24, 117, 247, 41, 199, 61, 147, 64, 143, 33, 97, 32, 6, 126, 216, 140, 118,
    224, 140, 40, 127, 193, 148, 96, 0, 0, 0, 0,
];

crate::solana_name_id!(ID, "Sysvar1111111111111111111111111111111111111");
