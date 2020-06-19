use serde_json::{json, Value};
use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};
use std::{
    collections::HashMap,
    str::{from_utf8, FromStr},
};

lazy_static! {
    static ref MEMO_PROGRAM_ID: Pubkey = Pubkey::from_str(&spl_memo::id().to_string()).unwrap();
    pub static ref PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableProgram> = {
        let mut m = HashMap::new();
        m.insert(*MEMO_PROGRAM_ID, ParsableProgram::Memo);
        m
    };
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParsableProgram {
    Memo,
}

pub fn parse(program_id: &Pubkey, instruction: &CompiledInstruction) -> Option<Value> {
    PARSABLE_PROGRAM_IDS.get(program_id).map(|program_name| {
        let parsed_json = match program_name {
            ParsableProgram::Memo => parse_memo(instruction),
        };
        json!({ format!("{:?}", program_name).to_lowercase(): parsed_json })
    })
}

fn parse_memo(instruction: &CompiledInstruction) -> Value {
    Value::String(from_utf8(&instruction.data).unwrap().to_string())
}
