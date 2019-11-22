//! This account contains the current cluster rewards point values
//!
use crate::{account::Account, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarRewards111111111111111111111111111111", Rewards);

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
