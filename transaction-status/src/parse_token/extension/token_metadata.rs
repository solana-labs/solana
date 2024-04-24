use {
    super::*,
    spl_token_metadata_interface::{
        instruction::{
            Emit, Initialize, RemoveKey, TokenMetadataInstruction, UpdateAuthority, UpdateField,
        },
        state::Field,
    },
};

fn token_metadata_field_to_string(field: &Field) -> String {
    match field {
        Field::Name => "name".to_string(),
        Field::Symbol => "symbol".to_string(),
        Field::Uri => "uri".to_string(),
        Field::Key(key) => key.clone(),
    }
}

pub(in crate::parse_token) fn parse_token_metadata_instruction(
    instruction: &TokenMetadataInstruction,
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match instruction {
        TokenMetadataInstruction::Initialize(metadata) => {
            check_num_token_accounts(account_indexes, 4)?;
            let Initialize { name, symbol, uri } = metadata;
            let value = json!({
                "metadata": account_keys[account_indexes[0] as usize].to_string(),
                "updateAuthority": account_keys[account_indexes[1] as usize].to_string(),
                "mint": account_keys[account_indexes[2] as usize].to_string(),
                "mintAuthority": account_keys[account_indexes[3] as usize].to_string(),
                "name": name,
                "symbol": symbol,
                "uri": uri,
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeTokenMetadata".to_string(),
                info: value,
            })
        }
        TokenMetadataInstruction::UpdateField(update) => {
            check_num_token_accounts(account_indexes, 2)?;
            let UpdateField { field, value } = update;
            let value = json!({
                "metadata": account_keys[account_indexes[0] as usize].to_string(),
                "updateAuthority": account_keys[account_indexes[1] as usize].to_string(),
                "field": token_metadata_field_to_string(field),
                "value": value,
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "updateTokenMetadataField".to_string(),
                info: value,
            })
        }
        TokenMetadataInstruction::RemoveKey(remove) => {
            check_num_token_accounts(account_indexes, 2)?;
            let RemoveKey { key, idempotent } = remove;
            let value = json!({
                "metadata": account_keys[account_indexes[0] as usize].to_string(),
                "updateAuthority": account_keys[account_indexes[1] as usize].to_string(),
                "key": key,
                "idempotent": *idempotent,
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "removeTokenMetadataKey".to_string(),
                info: value,
            })
        }
        TokenMetadataInstruction::UpdateAuthority(update) => {
            check_num_token_accounts(account_indexes, 2)?;
            let UpdateAuthority { new_authority } = update;
            let value = json!({
                "metadata": account_keys[account_indexes[0] as usize].to_string(),
                "updateAuthority": account_keys[account_indexes[1] as usize].to_string(),
                "newAuthority": Option::<Pubkey>::from(*new_authority).map(|v| v.to_string()),
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "updateTokenMetadataAuthority".to_string(),
                info: value,
            })
        }
        TokenMetadataInstruction::Emit(emit) => {
            check_num_token_accounts(account_indexes, 1)?;
            let Emit { start, end } = emit;
            let mut value = json!({
                "metadata": account_keys[account_indexes[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if let Some(start) = *start {
                map.insert("start".to_string(), json!(start));
            }
            if let Some(end) = *end {
                map.insert("end".to_string(), json!(end));
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "emitTokenMetadata".to_string(),
                info: value,
            })
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, solana_sdk::pubkey::Pubkey, spl_token_2022::solana_program::message::Message};

    #[test]
    fn test_parse_token_metadata_instruction() {
        let mint = Pubkey::new_unique();
        let mint_authority = Pubkey::new_unique();
        let update_authority = Pubkey::new_unique();
        let metadata = Pubkey::new_unique();

        let name = "Mega Token".to_string();
        let symbol = "MEGA".to_string();
        let uri = "https://mega.com".to_string();

        // Initialize
        let ix = spl_token_metadata_interface::instruction::initialize(
            &spl_token_2022::id(),
            &metadata,
            &update_authority,
            &mint,
            &mint_authority,
            name.clone(),
            symbol.clone(),
            uri.clone(),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeTokenMetadata".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "updateAuthority": update_authority.to_string(),
                    "mint": mint.to_string(),
                    "mintAuthority": mint_authority.to_string(),
                    "name": name,
                    "symbol": symbol,
                    "uri": uri,
                })
            }
        );

        // UpdateField
        // Update one of the fixed fields.
        let ix = spl_token_metadata_interface::instruction::update_field(
            &spl_token_2022::id(),
            &metadata,
            &update_authority,
            spl_token_metadata_interface::state::Field::Uri,
            "https://ultra-mega.com".to_string(),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateTokenMetadataField".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "updateAuthority": update_authority.to_string(),
                    "field": "uri",
                    "value": "https://ultra-mega.com",
                })
            }
        );
        // Add a new field
        let ix = spl_token_metadata_interface::instruction::update_field(
            &spl_token_2022::id(),
            &metadata,
            &update_authority,
            spl_token_metadata_interface::state::Field::Key("new_field".to_string()),
            "new_value".to_string(),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateTokenMetadataField".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "updateAuthority": update_authority.to_string(),
                    "field": "new_field",
                    "value": "new_value",
                })
            }
        );

        // RemoveKey
        let ix = spl_token_metadata_interface::instruction::remove_key(
            &spl_token_2022::id(),
            &metadata,
            &update_authority,
            "new_field".to_string(),
            false,
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "removeTokenMetadataKey".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "updateAuthority": update_authority.to_string(),
                    "key": "new_field",
                    "idempotent": false,
                })
            }
        );

        // UpdateAuthority
        // Update authority to a new authority.
        let new_authority = Pubkey::new_unique();
        let ix = spl_token_metadata_interface::instruction::update_authority(
            &spl_token_2022::id(),
            &metadata,
            &update_authority,
            Some(new_authority).try_into().unwrap(),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateTokenMetadataAuthority".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "updateAuthority": update_authority.to_string(),
                    "newAuthority": new_authority.to_string(),
                })
            }
        );
        // Update authority to None.
        let ix = spl_token_metadata_interface::instruction::update_authority(
            &spl_token_2022::id(),
            &metadata,
            &update_authority,
            Option::<Pubkey>::None.try_into().unwrap(),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateTokenMetadataAuthority".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "updateAuthority": update_authority.to_string(),
                    "newAuthority": null,
                })
            }
        );

        // Emit
        // Emit with start and end.
        let ix = spl_token_metadata_interface::instruction::emit(
            &spl_token_2022::id(),
            &metadata,
            Some(1),
            Some(2),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "emitTokenMetadata".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "start": 1,
                    "end": 2,
                })
            }
        );
        // Emit with only start.
        let ix = spl_token_metadata_interface::instruction::emit(
            &spl_token_2022::id(),
            &metadata,
            Some(1),
            None,
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "emitTokenMetadata".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "start": 1,
                })
            }
        );
        // Emit with only end.
        let ix = spl_token_metadata_interface::instruction::emit(
            &spl_token_2022::id(),
            &metadata,
            None,
            Some(2),
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "emitTokenMetadata".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                    "end": 2,
                })
            }
        );
        // Emit with neither start nor end.
        let ix = spl_token_metadata_interface::instruction::emit(
            &spl_token_2022::id(),
            &metadata,
            None,
            None,
        );
        let mut message = Message::new(&[ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "emitTokenMetadata".to_string(),
                info: json!({
                    "metadata": metadata.to_string(),
                })
            }
        );
    }
}
