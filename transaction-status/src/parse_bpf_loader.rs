use {
    crate::parse_instruction::{
        check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
    },
    bincode::deserialize,
    serde_json::json,
    solana_sdk::{
        instruction::CompiledInstruction, loader_instruction::LoaderInstruction,
        loader_upgradeable_instruction::UpgradeableLoaderInstruction, pubkey::Pubkey,
    },
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

pub fn parse_bpf_upgradeable_loader(
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let bpf_upgradeable_loader_instruction: UpgradeableLoaderInstruction =
        deserialize(&instruction.data).map_err(|_| {
            ParseInstructionError::InstructionNotParsable(ParsableProgram::BpfUpgradeableLoader)
        })?;
    match instruction.accounts.iter().max() {
        Some(index) if (*index as usize) < account_keys.len() => {}
        _ => {
            // Runtime should prevent this from ever happening
            return Err(ParseInstructionError::InstructionKeyMismatch(
                ParsableProgram::BpfUpgradeableLoader,
            ));
        }
    }
    match bpf_upgradeable_loader_instruction {
        UpgradeableLoaderInstruction::InitializeBuffer => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 1)?;
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if instruction.accounts.len() > 1 {
                map.insert(
                    "authority".to_string(),
                    json!(account_keys[instruction.accounts[1] as usize].to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeBuffer".to_string(),
                info: value,
            })
        }
        UpgradeableLoaderInstruction::Write { offset, bytes } => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(bytes),
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "authority": account_keys[instruction.accounts[1] as usize].to_string(),
                }),
            })
        }
        UpgradeableLoaderInstruction::DeployWithMaxDataLen { max_data_len } => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 8)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "deployWithMaxDataLen".to_string(),
                info: json!({
                    "maxDataLen": max_data_len,
                    "payerAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "programDataAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "programAccount": account_keys[instruction.accounts[2] as usize].to_string(),
                    "bufferAccount": account_keys[instruction.accounts[3] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[4] as usize].to_string(),
                    "clockSysvar": account_keys[instruction.accounts[5] as usize].to_string(),
                    "systemProgram": account_keys[instruction.accounts[6] as usize].to_string(),
                    "authority": account_keys[instruction.accounts[7] as usize].to_string(),
                }),
            })
        }
        UpgradeableLoaderInstruction::Upgrade => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 7)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "upgrade".to_string(),
                info: json!({
                    "programDataAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "programAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "bufferAccount": account_keys[instruction.accounts[2] as usize].to_string(),
                    "spillAccount": account_keys[instruction.accounts[3] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[4] as usize].to_string(),
                    "clockSysvar": account_keys[instruction.accounts[5] as usize].to_string(),
                    "authority": account_keys[instruction.accounts[6] as usize].to_string(),
                }),
            })
        }
        UpgradeableLoaderInstruction::SetAuthority => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "authority": account_keys[instruction.accounts[1] as usize].to_string(),
                    "newAuthority": if instruction.accounts.len() > 2 {
                        Some(account_keys[instruction.accounts[2] as usize].to_string())
                    } else {
                        None
                    },
                }),
            })
        }
        UpgradeableLoaderInstruction::Close => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "close".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "recipient": account_keys[instruction.accounts[1] as usize].to_string(),
                    "authority": account_keys[instruction.accounts[2] as usize].to_string()
                }),
            })
        }
    }
}

fn check_num_bpf_upgradeable_loader_accounts(
    accounts: &[u8],
    num: usize,
) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::BpfUpgradeableLoader)
}

#[cfg(test)]
mod test {
    use {
        super::*,
        serde_json::Value,
        solana_sdk::{message::Message, pubkey},
    };

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

    #[test]
    fn test_parse_bpf_upgradeable_loader_instructions() {
        let mut keys: Vec<Pubkey> = vec![];
        for _ in 0..8 {
            keys.push(Pubkey::new_unique());
        }
        let offset = 4242;
        let bytes = vec![8; 99];
        let max_data_len = 54321;

        let instructions = solana_sdk::bpf_loader_upgradeable::create_buffer(
            &keys[0],
            &keys[1],
            &keys[2],
            55,
            max_data_len,
        )
        .unwrap();
        let message = Message::new(&instructions, None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[1], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeBuffer".to_string(),
                info: json!({
                    "account": keys[1].to_string(),
                    "authority": keys[2].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[1], &keys[0..2]).is_err());

        let instruction =
            solana_sdk::bpf_loader_upgradeable::write(&keys[1], &keys[0], offset, bytes.clone());
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..2]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(&bytes),
                    "account": keys[1].to_string(),
                    "authority": keys[0].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..1]).is_err());

        let instructions = solana_sdk::bpf_loader_upgradeable::deploy_with_max_program_len(
            &keys[0],
            &keys[1],
            &keys[4],
            &keys[2],
            55,
            max_data_len,
        )
        .unwrap();
        let message = Message::new(&instructions, None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[1], &keys[0..8]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "deployWithMaxDataLen".to_string(),
                info: json!({
                    "maxDataLen": max_data_len,
                    "payerAccount": keys[0].to_string(),
                    "programAccount": keys[1].to_string(),
                    "authority": keys[2].to_string(),
                    "programDataAccount": keys[3].to_string(),
                    "bufferAccount": keys[4].to_string(),
                    "rentSysvar": keys[5].to_string(),
                    "clockSysvar": keys[6].to_string(),
                    "systemProgram": keys[7].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[1], &keys[0..7]).is_err());

        let instruction =
            solana_sdk::bpf_loader_upgradeable::upgrade(&keys[2], &keys[3], &keys[0], &keys[4]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..7]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "upgrade".to_string(),
                info: json!({
                    "authority": keys[0].to_string(),
                    "programDataAccount": keys[1].to_string(),
                    "programAccount": keys[2].to_string(),
                    "bufferAccount": keys[3].to_string(),
                    "spillAccount": keys[4].to_string(),
                    "rentSysvar": keys[5].to_string(),
                    "clockSysvar": keys[6].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..6]).is_err());

        let instruction =
            solana_sdk::bpf_loader_upgradeable::set_buffer_authority(&keys[1], &keys[0], &keys[2]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": keys[1].to_string(),
                    "authority": keys[0].to_string(),
                    "newAuthority": keys[2].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..1]).is_err());

        let instruction = solana_sdk::bpf_loader_upgradeable::set_upgrade_authority(
            &keys[1],
            &keys[0],
            Some(&keys[2]),
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": keys[1].to_string(),
                    "authority": keys[0].to_string(),
                    "newAuthority": keys[2].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..1]).is_err());

        let instruction =
            solana_sdk::bpf_loader_upgradeable::set_upgrade_authority(&keys[1], &keys[0], None);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..2]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": keys[1].to_string(),
                    "authority": keys[0].to_string(),
                    "newAuthority": Value::Null,
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..1]).is_err());

        let instruction = solana_sdk::bpf_loader_upgradeable::close(&keys[0], &keys[1], &keys[2]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(&message.instructions[0], &keys[..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "close".to_string(),
                info: json!({
                    "account": keys[1].to_string(),
                    "recipient": keys[2].to_string(),
                    "authority": keys[0].to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(&message.instructions[0], &keys[0..1]).is_err());
    }
}
