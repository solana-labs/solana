use bincode::{self, deserialize, serialize_into};
use num_derive::FromPrimitive;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::DecodeError;
use solana_sdk::pubkey::Pubkey;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ValidatorInfoState {
    pub validator_pubkey: Pubkey,
    pub validator_info: String,
}

impl ValidatorInfoState {
    pub fn new(validator_pubkey: &Pubkey, validator_info: String) -> Self {
        Self {
            validator_pubkey: *validator_pubkey,
            validator_info,
        }
    }

    pub fn serialize(&self, output: &mut [u8]) -> Result<(), InstructionError> {
        serialize_into(output, self).map_err(|_| InstructionError::AccountDataTooSmall)
    }

    pub fn deserialize(input: &[u8]) -> Result<Self, InstructionError> {
        deserialize(input).map_err(|_| InstructionError::InvalidAccountData)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive)]
pub enum ValidatorInfoError {
    ValidatorMismatch,
}

impl<T> DecodeError<T> for ValidatorInfoError {
    fn type_of(&self) -> &'static str {
        "ValidatorInfoError"
    }
}

impl std::fmt::Display for ValidatorInfoError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
impl std::error::Error for ValidatorInfoError {}

#[cfg(test)]
mod test {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;

    #[test]
    fn test_serializer() {
        let mut a = Account::new(0, 512, &id());
        let b = ValidatorInfoState::default();
        b.serialize(&mut a.data).unwrap();
        let c = ValidatorInfoState::deserialize(&a.data).unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn test_serializer_data_too_small() {
        let mut a = Account::new(0, 1, &id());
        let b = ValidatorInfoState::default();
        assert_eq!(
            b.serialize(&mut a.data),
            Err(InstructionError::AccountDataTooSmall)
        );
    }
}
