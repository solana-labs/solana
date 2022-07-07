use {
    crate::{parse_account_data::ParseAccountError, UiFeeCalculator},
    solana_sdk::{
        instruction::InstructionError,
        nonce::{state::Versions, State},
    },
};

pub fn parse_nonce(data: &[u8]) -> Result<UiNonceState, ParseAccountError> {
    let nonce_versions: Versions = bincode::deserialize(data)
        .map_err(|_| ParseAccountError::from(InstructionError::InvalidAccountData))?;
    match nonce_versions.state() {
        // This prevents parsing an allocated System-owned account with empty data of any non-zero
        // length as `uninitialized` nonce. An empty account of the wrong length can never be
        // initialized as a nonce account, and an empty account of the correct length may not be an
        // uninitialized nonce account, since it can be assigned to another program.
        State::Uninitialized => Err(ParseAccountError::from(
            InstructionError::InvalidAccountData,
        )),
        State::Initialized(data) => Ok(UiNonceState::Initialized(UiNonceData {
            authority: data.authority.to_string(),
            blockhash: data.blockhash().to_string(),
            fee_calculator: data.fee_calculator.into(),
        })),
    }
}

/// A duplicate representation of NonceState for pretty JSON serialization
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum UiNonceState {
    Uninitialized,
    Initialized(UiNonceData),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiNonceData {
    pub authority: String,
    pub blockhash: String,
    pub fee_calculator: UiFeeCalculator,
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            nonce::{
                state::{Data, Versions},
                State,
            },
            pubkey::Pubkey,
        },
    };

    #[test]
    fn test_parse_nonce() {
        let nonce_data = Versions::new(State::Initialized(Data::default()));
        let nonce_account_data = bincode::serialize(&nonce_data).unwrap();
        assert_eq!(
            parse_nonce(&nonce_account_data).unwrap(),
            UiNonceState::Initialized(UiNonceData {
                authority: Pubkey::default().to_string(),
                blockhash: Hash::default().to_string(),
                fee_calculator: UiFeeCalculator {
                    lamports_per_signature: 0.to_string(),
                },
            }),
        );

        let bad_data = vec![0; 4];
        assert!(parse_nonce(&bad_data).is_err());
    }
}
