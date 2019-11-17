use bincode::serialize_into;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{instruction::InstructionError, pubkey::Pubkey, short_vec};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct NftState {
    pub issuer_pubkey: Pubkey,
    pub owner_pubkey: Pubkey,
    #[serde(with = "short_vec")]
    pub id: Vec<u8>,
}

impl NftState {
    pub fn serialize(&self, output: &mut [u8]) -> Result<(), InstructionError> {
        serialize_into(output, self).map_err(|_| InstructionError::AccountDataTooSmall)
    }
}
