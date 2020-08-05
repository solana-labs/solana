use crate::parse_account_data::ParseAccountError;
use solana_sdk::{
    fee_calculator::FeeCalculator,
    instruction::InstructionError,
    nonce::{state::Versions, State},
};

pub fn parse_nonce(data: &[u8]) -> Result<UiNonceState, ParseAccountError> {
    let nonce_state: Versions = bincode::deserialize(data)
        .map_err(|_| ParseAccountError::from(InstructionError::InvalidAccountData))?;
    let nonce_state = nonce_state.convert_to_current();
    match nonce_state {
        State::Uninitialized => Ok(UiNonceState::Uninitialized),
        State::Initialized(data) => Ok(UiNonceState::Initialized(UiNonceData {
            authority: data.authority.to_string(),
            blockhash: data.blockhash.to_string(),
            fee_calculator: data.fee_calculator,
        })),
    }
}

/// A duplicate representation of NonceState for pretty JSON serialization
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum UiNonceState {
    Uninitialized,
    Initialized(UiNonceData),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiNonceData {
    pub authority: String,
    pub blockhash: String,
    pub fee_calculator: FeeCalculator,
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{
        hash::Hash,
        nonce::{
            state::{Data, Versions},
            State,
        },
        pubkey::Pubkey,
    };

    #[test]
    fn test_parse_nonce() {
        let nonce_data = Versions::new_current(State::Initialized(Data::default()));
        let nonce_account_data = bincode::serialize(&nonce_data).unwrap();
        assert_eq!(
            parse_nonce(&nonce_account_data).unwrap(),
            UiNonceState::Initialized(UiNonceData {
                authority: Pubkey::default().to_string(),
                blockhash: Hash::default().to_string(),
                fee_calculator: FeeCalculator::default(),
            }),
        );

        let bad_data = vec![0; 4];
        assert!(parse_nonce(&bad_data).is_err());
    }
}
