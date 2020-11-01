/// Runtime features.
///
/// Feature activation is accomplished by:
/// 1. Activation is requested by the feature authority, who issues a transaction to create the
///    feature account.  The newly created feature account will have the value of
///    `Feature::default()`
/// 2. When the next epoch is entered the runtime will check for new activation requests and
///    active them.  When this occurs, the activation slot is recorded in the feature account
///
use crate::{
    account_info::AccountInfo, clock::Slot, instruction::Instruction, program_error::ProgramError,
    pubkey::Pubkey, system_instruction,
};

crate::declare_id!("Feature111111111111111111111111111111111111");

#[derive(Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct Feature {
    pub activated_at: Option<Slot>,
}

impl Feature {
    pub fn size_of() -> usize {
        bincode::serialized_size(&Feature {
            activated_at: Some(0),
        })
        .unwrap() as usize
    }

    pub fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        if *account_info.owner != id() {
            return Err(ProgramError::InvalidArgument);
        }
        bincode::deserialize(&account_info.data.borrow()).map_err(|_| ProgramError::InvalidArgument)
    }
}

/// Propose a feature
///
/// Accounts expected:
/// * `feature_id: [writeable,signer]` - The account representing the feature
/// * `funding_address: [writeable,signer]` - The funding account for the feature
///
/// It's recommended that the feature account be funded with
/// `rent.minimum_balance(Feature::size_of())` lamports.
///
pub fn propose(feature_id: &Pubkey, funding_address: &Pubkey, lamports: u64) -> Vec<Instruction> {
    vec![
        system_instruction::transfer(funding_address, feature_id, lamports),
        system_instruction::allocate(feature_id, Feature::size_of() as u64),
    ]
}

/// Approve a proposed feature.
///
/// Accounts expected:
/// * `feature_id: [writeable,signer]` - The account representing the feature
///
pub fn approve(feature_id: &Pubkey) -> Instruction {
    system_instruction::assign(feature_id, &id())
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_program::clock::Slot;

    #[test]
    fn feature_sizeof() {
        assert!(
            Feature::size_of() >= bincode::serialized_size(&Feature::default()).unwrap() as usize
        );
        assert_eq!(Feature::default(), Feature { activated_at: None });

        let features = [
            Feature {
                activated_at: Some(0),
            },
            Feature {
                activated_at: Some(Slot::MAX),
            },
        ];
        for feature in &features {
            assert_eq!(
                Feature::size_of(),
                bincode::serialized_size(feature).unwrap() as usize
            );
        }
    }
}
