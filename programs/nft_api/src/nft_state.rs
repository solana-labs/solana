use bincode::serialize_into;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{instruction::InstructionError, pubkey::Pubkey};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct NftState {
    pub owner_pubkey: Pubkey,
}

impl NftState {
    pub fn serialize(&self, output: &mut [u8]) -> Result<(), InstructionError> {
        serialize_into(output, self).map_err(|_| InstructionError::AccountDataTooSmall)
    }
}
