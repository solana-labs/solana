use {
    crate::parse_instruction::{
        check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
    },
    bincode::deserialize,
    serde_json::json,
    solana_sdk::{
        instruction::CompiledInstruction, message::AccountKeys,
        system_instruction::SystemInstruction,
    },
};

pub fn parse_system(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let system_instruction: SystemInstruction = deserialize(&instruction.data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::System))?;
    match instruction.accounts.iter().max() {
        Some(index) if (*index as usize) < account_keys.len() => {}
        _ => {
            // Runtime should prevent this from ever happening
            return Err(ParseInstructionError::InstructionKeyMismatch(
                ParsableProgram::System,
            ));
        }
    }
    match system_instruction {
        SystemInstruction::CreateAccount {
            lamports,
            space,
            owner,
        } => {
            check_num_system_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "createAccount".to_string(),
                info: json!({
                    "source": account_keys[instruction.accounts[0] as usize].to_string(),
                    "newAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "lamports": lamports,
                    "space": space,
                    "owner": owner.to_string(),
                }),
            })
        }
        SystemInstruction::Assign { owner } => {
            check_num_system_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "assign".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "owner": owner.to_string(),
                }),
            })
        }
        SystemInstruction::Transfer { lamports } => {
            check_num_system_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: json!({
                    "source": account_keys[instruction.accounts[0] as usize].to_string(),
                    "destination": account_keys[instruction.accounts[1] as usize].to_string(),
                    "lamports": lamports,
                }),
            })
        }
        SystemInstruction::CreateAccountWithSeed {
            base,
            seed,
            lamports,
            space,
            owner,
        } => {
            check_num_system_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "createAccountWithSeed".to_string(),
                info: json!({
                    "source": account_keys[instruction.accounts[0] as usize].to_string(),
                    "newAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "base": base.to_string(),
                    "seed": seed,
                    "lamports": lamports,
                    "space": space,
                    "owner": owner.to_string(),
                }),
            })
        }
        SystemInstruction::AdvanceNonceAccount => {
            check_num_system_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "advanceNonce".to_string(),
                info: json!({
                    "nonceAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "recentBlockhashesSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
                    "nonceAuthority": account_keys[instruction.accounts[2] as usize].to_string(),
                }),
            })
        }
        SystemInstruction::WithdrawNonceAccount(lamports) => {
            check_num_system_accounts(&instruction.accounts, 5)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawFromNonce".to_string(),
                info: json!({
                    "nonceAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "destination": account_keys[instruction.accounts[1] as usize].to_string(),
                    "recentBlockhashesSysvar": account_keys[instruction.accounts[2] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[3] as usize].to_string(),
                    "nonceAuthority": account_keys[instruction.accounts[4] as usize].to_string(),
                    "lamports": lamports,
                }),
            })
        }
        SystemInstruction::InitializeNonceAccount(authority) => {
            check_num_system_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeNonce".to_string(),
                info: json!({
                    "nonceAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "recentBlockhashesSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[2] as usize].to_string(),
                    "nonceAuthority": authority.to_string(),
                }),
            })
        }
        SystemInstruction::AuthorizeNonceAccount(authority) => {
            check_num_system_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "authorizeNonce".to_string(),
                info: json!({
                    "nonceAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "nonceAuthority": account_keys[instruction.accounts[1] as usize].to_string(),
                    "newAuthorized": authority.to_string(),
                }),
            })
        }
        SystemInstruction::UpgradeNonceAccount => {
            check_num_system_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "upgradeNonce".to_string(),
                info: json!({
                    "nonceAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                }),
            })
        }
        SystemInstruction::Allocate { space } => {
            check_num_system_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "allocate".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "space": space,
                }),
            })
        }
        SystemInstruction::AllocateWithSeed {
            base,
            seed,
            space,
            owner,
        } => {
            check_num_system_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "allocateWithSeed".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "base": base.to_string(),
                    "seed": seed,
                    "space": space,
                    "owner": owner.to_string(),
                }),
            })
        }
        SystemInstruction::AssignWithSeed { base, seed, owner } => {
            check_num_system_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "assignWithSeed".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "base": base.to_string(),
                    "seed": seed,
                    "owner": owner.to_string(),
                }),
            })
        }
        SystemInstruction::TransferWithSeed {
            lamports,
            from_seed,
            from_owner,
        } => {
            check_num_system_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "transferWithSeed".to_string(),
                info: json!({
                    "source": account_keys[instruction.accounts[0] as usize].to_string(),
                    "sourceBase": account_keys[instruction.accounts[1] as usize].to_string(),
                    "destination": account_keys[instruction.accounts[2] as usize].to_string(),
                    "lamports": lamports,
                    "sourceSeed": from_seed,
                    "sourceOwner": from_owner.to_string(),
                }),
            })
        }
    }
}

fn check_num_system_accounts(accounts: &[u8], num: usize) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::System)
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{message::Message, pubkey::Pubkey, system_instruction, sysvar},
    };

    #[test]
    fn test_parse_system_create_account_ix() {
        let lamports = 55;
        let space = 128;
        let from_pubkey = Pubkey::new_unique();
        let to_pubkey = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();

        let instruction = system_instruction::create_account(
            &from_pubkey,
            &to_pubkey,
            lamports,
            space,
            &owner_pubkey,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "createAccount".to_string(),
                info: json!({
                    "source": from_pubkey.to_string(),
                    "newAccount": to_pubkey.to_string(),
                    "lamports": lamports,
                    "owner": owner_pubkey.to_string(),
                    "space": space,
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_assign_ix() {
        let account_pubkey = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::assign(&account_pubkey, &owner_pubkey);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "assign".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "owner": owner_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&[], None)).is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_transfer_ix() {
        let lamports = 55;
        let from_pubkey = Pubkey::new_unique();
        let to_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::transfer(&from_pubkey, &to_pubkey, lamports);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: json!({
                    "source": from_pubkey.to_string(),
                    "destination": to_pubkey.to_string(),
                    "lamports": lamports,
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_create_account_with_seed_ix() {
        let lamports = 55;
        let space = 128;
        let seed = "test_seed";
        let from_pubkey = Pubkey::new_unique();
        let to_pubkey = Pubkey::new_unique();
        let base_pubkey = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::create_account_with_seed(
            &from_pubkey,
            &to_pubkey,
            &base_pubkey,
            seed,
            lamports,
            space,
            &owner_pubkey,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "createAccountWithSeed".to_string(),
                info: json!({
                    "source": from_pubkey.to_string(),
                    "newAccount": to_pubkey.to_string(),
                    "lamports": lamports,
                    "base": base_pubkey.to_string(),
                    "seed": seed,
                    "owner": owner_pubkey.to_string(),
                    "space": space,
                }),
            }
        );

        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_allocate_ix() {
        let space = 128;
        let account_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::allocate(&account_pubkey, space);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "allocate".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "space": space,
                }),
            }
        );
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&[], None)).is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_allocate_with_seed_ix() {
        let space = 128;
        let seed = "test_seed";
        let account_pubkey = Pubkey::new_unique();
        let base_pubkey = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::allocate_with_seed(
            &account_pubkey,
            &base_pubkey,
            seed,
            space,
            &owner_pubkey,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "allocateWithSeed".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "base": base_pubkey.to_string(),
                    "seed": seed,
                    "owner": owner_pubkey.to_string(),
                    "space": space,
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_assign_with_seed_ix() {
        let seed = "test_seed";
        let account_pubkey = Pubkey::new_unique();
        let base_pubkey = Pubkey::new_unique();
        let owner_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::assign_with_seed(
            &account_pubkey,
            &base_pubkey,
            seed,
            &owner_pubkey,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "assignWithSeed".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "base": base_pubkey.to_string(),
                    "seed": seed,
                    "owner": owner_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_transfer_with_seed_ix() {
        let lamports = 55;
        let seed = "test_seed";
        let from_pubkey = Pubkey::new_unique();
        let from_base_pubkey = Pubkey::new_unique();
        let from_owner_pubkey = Pubkey::new_unique();
        let to_pubkey = Pubkey::new_unique();
        let instruction = system_instruction::transfer_with_seed(
            &from_pubkey,
            &from_base_pubkey,
            seed.to_string(),
            &from_owner_pubkey,
            &to_pubkey,
            lamports,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transferWithSeed".to_string(),
                info: json!({
                    "source": from_pubkey.to_string(),
                    "sourceBase": from_base_pubkey.to_string(),
                    "sourceSeed": seed,
                    "sourceOwner": from_owner_pubkey.to_string(),
                    "lamports": lamports,
                    "destination": to_pubkey.to_string()
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..2], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_advance_nonce_account_ix() {
        let nonce_pubkey = Pubkey::new_unique();
        let authorized_pubkey = Pubkey::new_unique();

        let instruction =
            system_instruction::advance_nonce_account(&nonce_pubkey, &authorized_pubkey);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "advanceNonce".to_string(),
                info: json!({
                    "nonceAccount": nonce_pubkey.to_string(),
                    "recentBlockhashesSysvar": sysvar::recent_blockhashes::ID.to_string(),
                    "nonceAuthority": authorized_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..2], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_withdraw_nonce_account_ix() {
        let nonce_pubkey = Pubkey::new_unique();
        let authorized_pubkey = Pubkey::new_unique();
        let to_pubkey = Pubkey::new_unique();

        let lamports = 55;
        let instruction = system_instruction::withdraw_nonce_account(
            &nonce_pubkey,
            &authorized_pubkey,
            &to_pubkey,
            lamports,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdrawFromNonce".to_string(),
                info: json!({
                    "nonceAccount": nonce_pubkey.to_string(),
                    "destination": to_pubkey.to_string(),
                    "recentBlockhashesSysvar": sysvar::recent_blockhashes::ID.to_string(),
                    "rentSysvar": sysvar::rent::ID.to_string(),
                    "nonceAuthority": authorized_pubkey.to_string(),
                    "lamports": lamports
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..4], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_initialize_nonce_ix() {
        let lamports = 55;
        let from_pubkey = Pubkey::new_unique();
        let nonce_pubkey = Pubkey::new_unique();
        let authorized_pubkey = Pubkey::new_unique();

        let instructions = system_instruction::create_nonce_account(
            &from_pubkey,
            &nonce_pubkey,
            &authorized_pubkey,
            lamports,
        );
        let mut message = Message::new(&instructions, None);
        assert_eq!(
            parse_system(
                &message.instructions[1],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeNonce".to_string(),
                info: json!({
                    "nonceAccount": nonce_pubkey.to_string(),
                    "recentBlockhashesSysvar": sysvar::recent_blockhashes::ID.to_string(),
                    "rentSysvar": sysvar::rent::ID.to_string(),
                    "nonceAuthority": authorized_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[1],
            &AccountKeys::new(&message.account_keys[0..3], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_parse_system_authorize_nonce_account_ix() {
        let nonce_pubkey = Pubkey::new_unique();
        let authorized_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Pubkey::new_unique();

        let instruction = system_instruction::authorize_nonce_account(
            &nonce_pubkey,
            &authorized_pubkey,
            &new_authority_pubkey,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_system(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "authorizeNonce".to_string(),
                info: json!({
                    "nonceAccount": nonce_pubkey.to_string(),
                    "newAuthorized": new_authority_pubkey.to_string(),
                    "nonceAuthority": authorized_pubkey.to_string(),
                }),
            }
        );
        assert!(parse_system(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_system(&message.instructions[0], &AccountKeys::new(&keys, None)).is_err());
    }
}
