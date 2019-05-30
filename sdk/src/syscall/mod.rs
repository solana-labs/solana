//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::pubkey::Pubkey;

pub mod fees;
pub mod slot_hashes;

/// "Sysca11111111111111111111111111111111111111"
///   owner pubkey for syscall accounts
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 216, 137, 185, 198, 189, 33, 204, 111, 12, 217, 220, 229, 201, 34, 52,
    253, 202, 87, 144, 232, 16, 195, 192, 0, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&ID)
}

pub fn check_id(id: &Pubkey) -> bool {
    id.as_ref() == ID
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_syscall_ids() {
        let ids = [("Sysca11111111111111111111111111111111111111", id())];
        // to get the bytes above:
        //        ids.iter().for_each(|(name, _)| {
        //            dbg!((name, bs58::decode(name).into_vec().unwrap()));
        //        });
        assert!(ids.iter().all(|(name, id)| *name == id.to_string()));
        assert!(check_id(&id()));
    }
}
