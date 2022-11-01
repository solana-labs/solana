use {
    crate::parse_instruction::{
        check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
    },
    bincode::deserialize,
    serde_json::json,
    solana_sdk::{
        instruction::CompiledInstruction, loader_instruction::LoaderInstruction,
        loader_upgradeable_instruction::UpgradeableLoaderInstruction, message::AccountKeys,
    },
};

pub fn parse_bpf_loader(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let bpf_loader_instruction: LoaderInstruction = deserialize(&instruction.data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::BpfLoader))?;
    if instruction.accounts.is_empty() || instruction.accounts[0] as usize >= account_keys.len() {
        return Err(ParseInstructionError::InstructionKeyMismatch(
            ParsableProgram::BpfLoader,
        ));
    }
    match bpf_loader_instruction {
        LoaderInstruction::Write { offset, bytes } => {
            check_num_bpf_loader_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(bytes),
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                }),
            })
        }
        LoaderInstruction::Finalize => {
            check_num_bpf_loader_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "finalize".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                }),
            })
        }
    }
}

pub fn parse_bpf_upgradeable_loader(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
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
        UpgradeableLoaderInstruction::SetAuthorityChecked => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "setAuthorityChecked".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "authority": account_keys[instruction.accounts[1] as usize].to_string(),
                    "newAuthority": account_keys[instruction.accounts[2] as usize].to_string(),
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
                    "authority": account_keys[instruction.accounts[2] as usize].to_string(),
                    "programAccount": if instruction.accounts.len() > 3 {
                        Some(account_keys[instruction.accounts[3] as usize].to_string())
                    } else {
                        None
                    }
                }),
            })
        }
        UpgradeableLoaderInstruction::ExtendProgram { additional_bytes } => {
            check_num_bpf_upgradeable_loader_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "extendProgram".to_string(),
                info: json!({
                    "additionalBytes": additional_bytes,
                    "programDataAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "programAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "systemProgram": if instruction.accounts.len() > 3 {
                        Some(account_keys[instruction.accounts[2] as usize].to_string())
                    } else {
                        None
                    },
                    "payerAccount": if instruction.accounts.len() > 4 {
                        Some(account_keys[instruction.accounts[3] as usize].to_string())
                    } else {
                        None
                    },
                }),
            })
        }
    }
}

fn check_num_bpf_loader_accounts(accounts: &[u8], num: usize) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::BpfLoader)
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
        solana_sdk::{
            bpf_loader_upgradeable,
            message::Message,
            pubkey::{self, Pubkey},
            system_program, sysvar,
        },
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
        let mut message = Message::new(&[instruction], Some(&fee_payer));
        assert_eq!(
            parse_bpf_loader(
                &message.instructions[0],
                &AccountKeys::new(&account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(&bytes),
                    "account": account_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_bpf_loader(
            &message.instructions[0],
            &AccountKeys::new(&missing_account_keys, None)
        )
        .is_err());
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_loader(
            &message.instructions[0],
            &AccountKeys::new(&account_keys, None)
        )
        .is_err());

        let instruction = solana_sdk::loader_instruction::finalize(&account_pubkey, &program_id);
        let mut message = Message::new(&[instruction], Some(&fee_payer));
        assert_eq!(
            parse_bpf_loader(
                &message.instructions[0],
                &AccountKeys::new(&account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "finalize".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_bpf_loader(
            &message.instructions[0],
            &AccountKeys::new(&missing_account_keys, None)
        )
        .is_err());
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_loader(
            &message.instructions[0],
            &AccountKeys::new(&account_keys, None)
        )
        .is_err());

        let bad_compiled_instruction = CompiledInstruction {
            program_id_index: 3,
            accounts: vec![1, 2],
            data: vec![2, 0, 0, 0], // LoaderInstruction enum only has 2 variants
        };
        assert!(parse_bpf_loader(
            &bad_compiled_instruction,
            &AccountKeys::new(&account_keys, None)
        )
        .is_err());

        let bad_compiled_instruction = CompiledInstruction {
            program_id_index: 3,
            accounts: vec![],
            data: vec![1, 0, 0, 0],
        };
        assert!(parse_bpf_loader(
            &bad_compiled_instruction,
            &AccountKeys::new(&account_keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_create_buffer_ix() {
        let max_data_len = 54321;

        let payer_address = Pubkey::new_unique();
        let buffer_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let instructions = bpf_loader_upgradeable::create_buffer(
            &payer_address,
            &buffer_address,
            &authority_address,
            55,
            max_data_len,
        )
        .unwrap();
        let mut message = Message::new(&instructions, None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[1],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeBuffer".to_string(),
                info: json!({
                    "account": buffer_address.to_string(),
                    "authority": authority_address.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[1],
            &AccountKeys::new(&message.account_keys[0..2], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[1].accounts.pop();
        message.instructions[1].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[1],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_write_ix() {
        let offset = 4242;
        let bytes = vec![8; 99];

        let buffer_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let instruction = bpf_loader_upgradeable::write(
            &buffer_address,
            &authority_address,
            offset,
            bytes.clone(),
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "write".to_string(),
                info: json!({
                    "offset": offset,
                    "bytes": base64::encode(&bytes),
                    "account": buffer_address.to_string(),
                    "authority": authority_address.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_deploy_ix() {
        let max_data_len = 54321;

        let payer_address = Pubkey::new_unique();
        let program_address = Pubkey::new_unique();
        let buffer_address = Pubkey::new_unique();
        let upgrade_authority_address = Pubkey::new_unique();
        let programdata_address = Pubkey::find_program_address(
            &[program_address.as_ref()],
            &bpf_loader_upgradeable::id(),
        )
        .0;
        let instructions = bpf_loader_upgradeable::deploy_with_max_program_len(
            &payer_address,
            &program_address,
            &buffer_address,
            &upgrade_authority_address,
            55,
            max_data_len,
        )
        .unwrap();
        let mut message = Message::new(&instructions, None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[1],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "deployWithMaxDataLen".to_string(),
                info: json!({
                    "maxDataLen": max_data_len,
                    "payerAccount": payer_address.to_string(),
                    "programAccount": program_address.to_string(),
                    "authority": upgrade_authority_address.to_string(),
                    "programDataAccount": programdata_address.to_string(),
                    "bufferAccount": buffer_address.to_string(),
                    "rentSysvar": sysvar::rent::ID.to_string(),
                    "clockSysvar": sysvar::clock::ID.to_string(),
                    "systemProgram": system_program::ID.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[1],
            &AccountKeys::new(&message.account_keys[0..7], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[1].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[1],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_upgrade_ix() {
        let program_address = Pubkey::new_unique();
        let buffer_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let spill_address = Pubkey::new_unique();
        let programdata_address = Pubkey::find_program_address(
            &[program_address.as_ref()],
            &bpf_loader_upgradeable::id(),
        )
        .0;
        let instruction = bpf_loader_upgradeable::upgrade(
            &program_address,
            &buffer_address,
            &authority_address,
            &spill_address,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "upgrade".to_string(),
                info: json!({
                    "authority": authority_address.to_string(),
                    "programDataAccount": programdata_address.to_string(),
                    "programAccount": program_address.to_string(),
                    "bufferAccount": buffer_address.to_string(),
                    "spillAccount": spill_address.to_string(),
                    "rentSysvar": sysvar::rent::ID.to_string(),
                    "clockSysvar": sysvar::clock::ID.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..6], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_set_buffer_authority_ix() {
        let buffer_address = Pubkey::new_unique();
        let current_authority_address = Pubkey::new_unique();
        let new_authority_address = Pubkey::new_unique();
        let instruction = bpf_loader_upgradeable::set_buffer_authority(
            &buffer_address,
            &current_authority_address,
            &new_authority_address,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": buffer_address.to_string(),
                    "authority": current_authority_address.to_string(),
                    "newAuthority": new_authority_address.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_set_buffer_authority_checked_ix() {
        let buffer_address = Pubkey::new_unique();
        let current_authority_address = Pubkey::new_unique();
        let new_authority_address = Pubkey::new_unique();
        let instruction = bpf_loader_upgradeable::set_buffer_authority_checked(
            &buffer_address,
            &current_authority_address,
            &new_authority_address,
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthorityChecked".to_string(),
                info: json!({
                    "account": buffer_address.to_string(),
                    "authority": current_authority_address.to_string(),
                    "newAuthority": new_authority_address.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..2], None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_set_upgrade_authority_ix() {
        let program_address = Pubkey::new_unique();
        let current_authority_address = Pubkey::new_unique();
        let new_authority_address = Pubkey::new_unique();
        let (programdata_address, _) = Pubkey::find_program_address(
            &[program_address.as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let instruction = bpf_loader_upgradeable::set_upgrade_authority(
            &program_address,
            &current_authority_address,
            Some(&new_authority_address),
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": programdata_address.to_string(),
                    "authority": current_authority_address.to_string(),
                    "newAuthority": new_authority_address.to_string(),
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());

        let instruction = bpf_loader_upgradeable::set_upgrade_authority(
            &program_address,
            &current_authority_address,
            None,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": programdata_address.to_string(),
                    "authority": current_authority_address.to_string(),
                    "newAuthority": Value::Null,
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_set_upgrade_authority_checked_ix() {
        let program_address = Pubkey::new_unique();
        let current_authority_address = Pubkey::new_unique();
        let new_authority_address = Pubkey::new_unique();
        let (programdata_address, _) = Pubkey::find_program_address(
            &[program_address.as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let instruction = bpf_loader_upgradeable::set_upgrade_authority_checked(
            &program_address,
            &current_authority_address,
            &new_authority_address,
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthorityChecked".to_string(),
                info: json!({
                    "account": programdata_address.to_string(),
                    "authority": current_authority_address.to_string(),
                    "newAuthority": new_authority_address.to_string(),
                }),
            }
        );

        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..2], None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_close_buffer_ix() {
        let close_address = Pubkey::new_unique();
        let recipient_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let instruction =
            bpf_loader_upgradeable::close(&close_address, &recipient_address, &authority_address);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "close".to_string(),
                info: json!({
                    "account": close_address.to_string(),
                    "recipient": recipient_address.to_string(),
                    "authority": authority_address.to_string(),
                    "programAccount": Value::Null
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_bpf_upgradeable_loader_close_program_ix() {
        let close_address = Pubkey::new_unique();
        let recipient_address = Pubkey::new_unique();
        let authority_address = Pubkey::new_unique();
        let program_address = Pubkey::new_unique();
        let instruction = bpf_loader_upgradeable::close_any(
            &close_address,
            &recipient_address,
            Some(&authority_address),
            Some(&program_address),
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_bpf_upgradeable_loader(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "close".to_string(),
                info: json!({
                    "account": close_address.to_string(),
                    "recipient": recipient_address.to_string(),
                    "authority": authority_address.to_string(),
                    "programAccount": program_address.to_string()
                }),
            }
        );
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        message.instructions[0].accounts.pop();
        assert!(parse_bpf_upgradeable_loader(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }
}
