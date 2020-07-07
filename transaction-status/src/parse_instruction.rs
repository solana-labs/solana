use inflector::Inflector;
use serde_json::{json, Value};
use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};
use std::{
    collections::HashMap,
    str::{from_utf8, FromStr},
};

lazy_static! {
    static ref MEMO_PROGRAM_ID: Pubkey = Pubkey::from_str(&spl_memo::id().to_string()).unwrap();
    static ref PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableProgram> = {
        let mut m = HashMap::new();
        m.insert(*MEMO_PROGRAM_ID, ParsableProgram::SplMemo);
        m
    };
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ParsableProgram {
    SplMemo,
}

pub fn parse(program_id: &Pubkey, instruction: &CompiledInstruction) -> Option<Value> {
    PARSABLE_PROGRAM_IDS.get(program_id).map(|program_name| {
        let parsed_json = match program_name {
            ParsableProgram::SplMemo => parse_memo(instruction),
        };
        json!({ format!("{:?}", program_name).to_kebab_case(): parsed_json })
    })
}

fn parse_memo(instruction: &CompiledInstruction) -> Value {
    Value::String(from_utf8(&instruction.data).unwrap().to_string())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() {
        let memo_instruction = CompiledInstruction {
            program_id_index: 0,
            accounts: vec![],
            data: vec![240, 159, 166, 150],
        };
        let expected_json = json!({
            "spl-memo": "🦖"
        });
        assert_eq!(
            parse(&MEMO_PROGRAM_ID, &memo_instruction),
            Some(expected_json)
        );

        let non_parsable_program_id = Pubkey::new(&[1; 32]);
        assert_eq!(parse(&non_parsable_program_id, &memo_instruction), None);
    }
}
