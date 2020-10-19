use crate::parse_instruction::{ParsableProgram, ParseInstructionError, ParsedInstructionEnum};
use bincode::deserialize;
use serde_json::{json, Value};
use solana_sdk::{instruction::CompiledInstruction, loader_instruction::LoaderInstruction};

pub fn parse_bpf_loader(
    instruction: &CompiledInstruction,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let bpf_loader_instruction: LoaderInstruction =
        deserialize(&instruction.data).map_err(|err| {
            println!("{:?}", err);
            ParseInstructionError::InstructionNotParsable(ParsableProgram::BpfLoader)
        })?;
    match bpf_loader_instruction {
        LoaderInstruction::Write { offset, bytes } => Ok(ParsedInstructionEnum {
            instruction_type: "write".to_string(),
            info: json!({
                "offset": offset,
                "bytes": base64::encode(bytes),
            }),
        }),
        LoaderInstruction::Finalize => Ok(ParsedInstructionEnum {
            instruction_type: "finalize".to_string(),
            info: Value::Null,
        }),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{message::Message, pubkey::Pubkey};

    #[test]
    fn test_parse_bpf_loader_instructions() {
        let account_pubkey = Pubkey::new_rand();
        let program_id = Pubkey::new_rand();
        let offset = 4242;
        let bytes = vec![8; 99];

        let instruction = solana_sdk::loader_instruction::write(
            &account_pubkey,
            &program_id,
            offset,
            bytes.clone(),
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_loader(&message.instructions[0]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(&bytes),
                }),
            }
        );

        let instruction = solana_sdk::loader_instruction::finalize(&account_pubkey, &program_id);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_loader(&message.instructions[0]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "finalize".to_string(),
                info: Value::Null,
            }
        );

        let bad_compiled_instruction = CompiledInstruction {
            program_id_index: 3,
            accounts: vec![1, 2],
            data: vec![2, 0, 0, 0], // LoaderInstruction enum only has 2 variants
        };
        assert!(parse_bpf_loader(&bad_compiled_instruction).is_err());
    }
}
