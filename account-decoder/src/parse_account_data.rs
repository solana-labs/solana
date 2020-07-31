use crate::{
    parse_nonce::parse_nonce,
    parse_token::{parse_token, spl_token_id_v1_0},
    parse_vote::parse_vote,
};
use inflector::Inflector;
use serde_json::{json, Value};
use solana_sdk::{instruction::InstructionError, pubkey::Pubkey, system_program};
use std::collections::HashMap;
use thiserror::Error;

lazy_static! {
    static ref SYSTEM_PROGRAM_ID: Pubkey = system_program::id();
    static ref TOKEN_PROGRAM_ID: Pubkey = spl_token_id_v1_0();
    static ref VOTE_PROGRAM_ID: Pubkey = solana_vote_program::id();
    pub static ref PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableAccount> = {
        let mut m = HashMap::new();
        m.insert(*SYSTEM_PROGRAM_ID, ParsableAccount::Nonce);
        m.insert(*TOKEN_PROGRAM_ID, ParsableAccount::SplToken);
        m.insert(*VOTE_PROGRAM_ID, ParsableAccount::Vote);
        m
    };
}

#[derive(Error, Debug)]
pub enum ParseAccountError {
    #[error("{0:?} account not parsable")]
    AccountNotParsable(ParsableAccount),

    #[error("Program not parsable")]
    ProgramNotParsable,

    #[error("Instruction error")]
    InstructionError(#[from] InstructionError),

    #[error("Serde json error")]
    SerdeJsonError(#[from] serde_json::error::Error),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParsableAccount {
    Nonce,
    SplToken,
    Vote,
}

pub fn parse_account_data(program_id: &Pubkey, data: &[u8]) -> Result<Value, ParseAccountError> {
    let program_name = PARSABLE_PROGRAM_IDS
        .get(program_id)
        .ok_or_else(|| ParseAccountError::ProgramNotParsable)?;
    let parsed_json = match program_name {
        ParsableAccount::Nonce => serde_json::to_value(parse_nonce(data)?)?,
        ParsableAccount::SplToken => serde_json::to_value(parse_token(data)?)?,
        ParsableAccount::Vote => serde_json::to_value(parse_vote(data)?)?,
    };
    Ok(json!({
        format!("{:?}", program_name).to_kebab_case(): parsed_json
    }))
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::nonce::{
        state::{Data, Versions},
        State,
    };
    use solana_vote_program::vote_state::{VoteState, VoteStateVersions};

    #[test]
    fn test_parse_account_data() {
        let other_program = Pubkey::new_rand();
        let data = vec![0; 4];
        assert!(parse_account_data(&other_program, &data).is_err());

        let vote_state = VoteState::default();
        let mut vote_account_data: Vec<u8> = vec![0; VoteState::size_of()];
        let versioned = VoteStateVersions::Current(Box::new(vote_state));
        VoteState::serialize(&versioned, &mut vote_account_data).unwrap();
        let parsed = parse_account_data(&solana_vote_program::id(), &vote_account_data).unwrap();
        assert!(parsed.as_object().unwrap().contains_key("vote"));

        let nonce_data = Versions::new_current(State::Initialized(Data::default()));
        let nonce_account_data = bincode::serialize(&nonce_data).unwrap();
        let parsed = parse_account_data(&system_program::id(), &nonce_account_data).unwrap();
        assert!(parsed.as_object().unwrap().contains_key("nonce"));
    }
}
