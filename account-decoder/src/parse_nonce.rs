use crate::parse_account_data::ParseAccountError;
use solana_sdk::{
    fee_calculator::FeeCalculator,
    instruction::InstructionError,
    nonce::{state::Versions, State},
};

pub fn parse_nonce(data: &[u8]) -> Result<DisplayNonceState, ParseAccountError> {
    let nonce_state: Versions = bincode::deserialize(data)
        .map_err(|_| ParseAccountError::from(InstructionError::InvalidAccountData))?;
    let nonce_state = nonce_state.convert_to_current();
    match nonce_state {
        State::Uninitialized => Ok(DisplayNonceState::Uninitialized),
        State::Initialized(data) => Ok(DisplayNonceState::Initialized(DisplayNonceData {
            authority: data.authority.to_string(),
            blockhash: data.blockhash.to_string(),
            fee_calculator: data.fee_calculator,
        })),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayNonceState {
    Uninitialized,
    Initialized(DisplayNonceData),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayNonceData {
    pub authority: String,
    pub blockhash: String,
    pub fee_calculator: FeeCalculator,
}
