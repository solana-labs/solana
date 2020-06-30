use crate::{parse_nonce::parse_nonce, parse_vote::parse_vote};
use inflector::Inflector;
use serde_json::{json, Value};
use solana_sdk::{instruction::InstructionError, pubkey::Pubkey, system_program};
use std::{collections::HashMap, str::FromStr};
use thiserror::Error;

lazy_static! {
    static ref SYSTEM_PROGRAM_ID: Pubkey =
        Pubkey::from_str(&system_program::id().to_string()).unwrap();
    static ref VOTE_PROGRAM_ID: Pubkey =
        Pubkey::from_str(&solana_vote_program::id().to_string()).unwrap();
    pub static ref PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableAccount> = {
        let mut m = HashMap::new();
        m.insert(*SYSTEM_PROGRAM_ID, ParsableAccount::Nonce);
        m.insert(*VOTE_PROGRAM_ID, ParsableAccount::Vote);
        m
    };
}

#[derive(Error, Debug)]
pub enum ParseAccountError {
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
    Vote,
}

pub fn parse_account_data(program_id: &Pubkey, data: &[u8]) -> Result<Value, ParseAccountError> {
    let program_name = PARSABLE_PROGRAM_IDS
        .get(program_id)
        .ok_or_else(|| ParseAccountError::ProgramNotParsable)?;
    let parsed_json = match program_name {
        ParsableAccount::Nonce => serde_json::to_value(parse_nonce(data)?)?,
        ParsableAccount::Vote => serde_json::to_value(parse_vote(data)?)?,
    };
    Ok(json!({
        format!("{:?}", program_name).to_kebab_case(): parsed_json
    }))
}
