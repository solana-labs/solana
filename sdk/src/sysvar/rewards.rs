//! This account contains the current cluster rewards point values
//!
use crate::{account::Account, sysvar::Sysvar};

///  account pubkey
const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 44, 97, 55, 206, 224, 146, 217, 182, 146, 62, 225, 204, 214, 25, 3, 250,
    130, 184, 161, 97, 145, 87, 141, 128, 0, 0, 0,
];

crate::solana_sysvar_id!(ID, "SysvarRewards111111111111111111111111111111", Rewards);

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Rewards {
    pub validator_point_value: f64,
    pub storage_point_value: f64,
}

impl Sysvar for Rewards {}

pub fn create_account(
    lamports: u64,
    validator_point_value: f64,
    storage_point_value: f64,
) -> Account {
    Rewards {
        validator_point_value,
        storage_point_value,
    }
    .create_account(lamports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let account = create_account(1, 0.0, 0.0);
        let rewards = Rewards::from_account(&account).unwrap();
        assert_eq!(rewards, Rewards::default());
    }
}
