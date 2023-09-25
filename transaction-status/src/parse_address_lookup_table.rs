use {
    crate::parse_instruction::{
        check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
    },
    bincode::deserialize,
    serde_json::json,
    solana_sdk::{
        address_lookup_table::instruction::ProgramInstruction, instruction::CompiledInstruction,
        message::AccountKeys,
    },
};

pub fn parse_address_lookup_table(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let address_lookup_table_instruction: ProgramInstruction = deserialize(&instruction.data)
        .map_err(|_| {
            ParseInstructionError::InstructionNotParsable(ParsableProgram::AddressLookupTable)
        })?;
    match instruction.accounts.iter().max() {
        Some(index) if (*index as usize) < account_keys.len() => {}
        _ => {
            // Runtime should prevent this from ever happening
            return Err(ParseInstructionError::InstructionKeyMismatch(
                ParsableProgram::AddressLookupTable,
            ));
        }
    }
    match address_lookup_table_instruction {
        ProgramInstruction::CreateLookupTable {
            recent_slot,
            bump_seed,
        } => {
            check_num_address_lookup_table_accounts(&instruction.accounts, 4)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "createLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "lookupTableAuthority": account_keys[instruction.accounts[1] as usize].to_string(),
                    "payerAccount": account_keys[instruction.accounts[2] as usize].to_string(),
                    "systemProgram": account_keys[instruction.accounts[3] as usize].to_string(),
                    "recentSlot": recent_slot,
                    "bumpSeed": bump_seed,
                }),
            })
        }
        ProgramInstruction::FreezeLookupTable => {
            check_num_address_lookup_table_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "freezeLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "lookupTableAuthority": account_keys[instruction.accounts[1] as usize].to_string(),
                }),
            })
        }
        ProgramInstruction::ExtendLookupTable { new_addresses } => {
            check_num_address_lookup_table_accounts(&instruction.accounts, 2)?;
            let new_addresses: Vec<String> = new_addresses
                .into_iter()
                .map(|address| address.to_string())
                .collect();
            let mut value = json!({
                "lookupTableAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                "lookupTableAuthority": account_keys[instruction.accounts[1] as usize].to_string(),
                "newAddresses": new_addresses,
            });
            let map = value.as_object_mut().unwrap();
            if instruction.accounts.len() >= 4 {
                map.insert(
                    "payerAccount".to_string(),
                    json!(account_keys[instruction.accounts[2] as usize].to_string()),
                );
                map.insert(
                    "systemProgram".to_string(),
                    json!(account_keys[instruction.accounts[3] as usize].to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "extendLookupTable".to_string(),
                info: value,
            })
        }
        ProgramInstruction::DeactivateLookupTable => {
            check_num_address_lookup_table_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "deactivateLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "lookupTableAuthority": account_keys[instruction.accounts[1] as usize].to_string(),
                }),
            })
        }
        ProgramInstruction::CloseLookupTable => {
            check_num_address_lookup_table_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "closeLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "lookupTableAuthority": account_keys[instruction.accounts[1] as usize].to_string(),
                    "recipient": account_keys[instruction.accounts[2] as usize].to_string(),
                }),
            })
        }
    }
}

fn check_num_address_lookup_table_accounts(
    accounts: &[u8],
    num: usize,
) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::AddressLookupTable)
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{
            address_lookup_table::instruction, message::Message, pubkey::Pubkey, system_program,
        },
        std::str::FromStr,
    };

    #[test]
    fn test_parse_create_address_lookup_table_ix() {
        let from_pubkey = Pubkey::new_unique();
        // use explicit key to have predicatble bump_seed
        let authority = Pubkey::from_str("HkxY6vXdrKzoCQLmdJ3cYo9534FdZQxzBNWTyrJzzqJM").unwrap();
        let slot = 42;

        let (instruction, lookup_table_pubkey) =
            instruction::create_lookup_table(authority, from_pubkey, slot);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_address_lookup_table(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "createLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": lookup_table_pubkey.to_string(),
                    "lookupTableAuthority": authority.to_string(),
                    "payerAccount": from_pubkey.to_string(),
                    "systemProgram": system_program::id().to_string(),
                    "recentSlot": slot,
                    "bumpSeed": 254,
                }),
            }
        );
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..3], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_freeze_lookup_table_ix() {
        let lookup_table_pubkey = Pubkey::new_unique();
        let authority = Pubkey::new_unique();

        let instruction = instruction::freeze_lookup_table(lookup_table_pubkey, authority);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_address_lookup_table(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "freezeLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": lookup_table_pubkey.to_string(),
                    "lookupTableAuthority": authority.to_string(),
                }),
            }
        );
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_extend_lookup_table_ix() {
        let lookup_table_pubkey = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let from_pubkey = Pubkey::new_unique();
        let no_addresses = vec![];
        let address0 = Pubkey::new_unique();
        let address1 = Pubkey::new_unique();
        let some_addresses = vec![address0, address1];

        // No payer, no addresses
        let instruction =
            instruction::extend_lookup_table(lookup_table_pubkey, authority, None, no_addresses);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_address_lookup_table(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "extendLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": lookup_table_pubkey.to_string(),
                    "lookupTableAuthority": authority.to_string(),
                    "newAddresses": [],
                }),
            }
        );
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());

        // Some payer, some addresses
        let instruction = instruction::extend_lookup_table(
            lookup_table_pubkey,
            authority,
            Some(from_pubkey),
            some_addresses,
        );
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_address_lookup_table(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "extendLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": lookup_table_pubkey.to_string(),
                    "lookupTableAuthority": authority.to_string(),
                    "payerAccount": from_pubkey.to_string(),
                    "systemProgram": system_program::id().to_string(),
                    "newAddresses": [
                        address0.to_string(),
                        address1.to_string(),
                    ],
                }),
            }
        );
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        message.instructions[0].accounts.pop();
        message.instructions[0].accounts.pop();
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_deactivate_lookup_table_ix() {
        let lookup_table_pubkey = Pubkey::new_unique();
        let authority = Pubkey::new_unique();

        let instruction = instruction::deactivate_lookup_table(lookup_table_pubkey, authority);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_address_lookup_table(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "deactivateLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": lookup_table_pubkey.to_string(),
                    "lookupTableAuthority": authority.to_string(),
                }),
            }
        );
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..1], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_close_lookup_table_ix() {
        let lookup_table_pubkey = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();

        let instruction =
            instruction::close_lookup_table(lookup_table_pubkey, authority, recipient);
        let mut message = Message::new(&[instruction], None);
        assert_eq!(
            parse_address_lookup_table(
                &message.instructions[0],
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "closeLookupTable".to_string(),
                info: json!({
                    "lookupTableAccount": lookup_table_pubkey.to_string(),
                    "lookupTableAuthority": authority.to_string(),
                    "recipient": recipient.to_string(),
                }),
            }
        );
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&message.account_keys[0..2], None)
        )
        .is_err());
        let keys = message.account_keys.clone();
        message.instructions[0].accounts.pop();
        assert!(parse_address_lookup_table(
            &message.instructions[0],
            &AccountKeys::new(&keys, None)
        )
        .is_err());
    }
}
