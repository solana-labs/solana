use crate::parse_instruction::{ParsableProgram, ParseInstructionError, ParsedInstructionEnum};
use bincode::deserialize;
use serde_json::json;
use solana_sdk::{
    instruction::CompiledInstruction, loader_instruction::LoaderInstruction, pubkey::Pubkey,
};

pub fn parse_bpf_loader(
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let bpf_loader_instruction: LoaderInstruction = deserialize(&instruction.data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::BpfLoader))?;
    if instruction.accounts.is_empty() || instruction.accounts[0] as usize >= account_keys.len() {
        return Err(ParseInstructionError::InstructionKeyMismatch(
            ParsableProgram::BpfLoader,
        ));
    }
    match bpf_loader_instruction {
        LoaderInstruction::Write { offset, bytes } => Ok(ParsedInstructionEnum {
            instruction_type: "write".to_string(),
            info: json!({
                "offset": offset,
                "bytes": base64::encode(bytes),
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
            }),
        }),
        LoaderInstruction::Finalize => Ok(ParsedInstructionEnum {
            instruction_type: "finalize".to_string(),
            info: json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
            }),
        }),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{message::Message, pubkey};

    #[test]
    fn test_parse_bpf_loader_instructions() {
        let account_pubkey = pubkey::new_rand();
        let program_id = pubkey::new_rand();
        let offset = 4242;
        let bytes = vec![8; 99];
        let fee_payer = pubkey::new_rand();
        let account_keys = vec![fee_payer, account_pubkey];
        let missing_account_keys = vec![account_pubkey];

        let instruction = solana_sdk::loader_instruction::write(
            &account_pubkey,
            &program_id,
            offset,
            bytes.clone(),
        );
        let message = Message::new(&[instruction], Some(&fee_payer));
        assert_eq!(
            parse_bpf_loader(&message.instructions[0], &account_keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(&bytes),
                    "account": account_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_bpf_loader(&message.instructions[0], &missing_account_keys).is_err());

        let instruction = solana_sdk::loader_instruction::finalize(&account_pubkey, &program_id);
        let message = Message::new(&[instruction], Some(&fee_payer));
        assert_eq!(
            parse_bpf_loader(&message.instructions[0], &account_keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "finalize".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_bpf_loader(&message.instructions[0], &missing_account_keys).is_err());

        let bad_compiled_instruction = CompiledInstruction {
            program_id_index: 3,
            accounts: vec![1, 2],
            data: vec![2, 0, 0, 0], // LoaderInstruction enum only has 2 variants
        };
        assert!(parse_bpf_loader(&bad_compiled_instruction, &account_keys).is_err());

        let bad_compiled_instruction = CompiledInstruction {
            program_id_index: 3,
            accounts: vec![],
            data: vec![1, 0, 0, 0],
        };
        assert!(parse_bpf_loader(&bad_compiled_instruction, &account_keys).is_err());
    }
}
