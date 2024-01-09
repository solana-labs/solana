use {
    super::*,
    spl_token_2022::{
        extension::group_member_pointer::instruction::*,
        instruction::{decode_instruction_data, decode_instruction_type},
    },
};

pub(in crate::parse_token) fn parse_group_member_pointer_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match decode_instruction_type(instruction_data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    {
        GroupMemberPointerInstruction::Initialize => {
            check_num_token_accounts(account_indexes, 1)?;
            let InitializeInstructionData {
                authority,
                member_address,
            } = *decode_instruction_data(instruction_data).map_err(|_| {
                ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
            })?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if let Some(authority) = Option::<Pubkey>::from(authority) {
                map.insert("authority".to_string(), json!(authority.to_string()));
            }
            if let Some(member_address) = Option::<Pubkey>::from(member_address) {
                map.insert(
                    "memberAddress".to_string(),
                    json!(member_address.to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeGroupMemberPointer".to_string(),
                info: value,
            })
        }
        GroupMemberPointerInstruction::Update => {
            check_num_token_accounts(account_indexes, 2)?;
            let UpdateInstructionData { member_address } =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if let Some(member_address) = Option::<Pubkey>::from(member_address) {
                map.insert(
                    "memberAddress".to_string(),
                    json!(member_address.to_string()),
                );
            }
            parse_signers(
                map,
                1,
                account_keys,
                account_indexes,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "updateGroupMemberPointer".to_string(),
                info: value,
            })
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, solana_sdk::pubkey::Pubkey, spl_token_2022::solana_program::message::Message};

    #[test]
    fn test_parse_group_member_pointer_instruction() {
        let mint_pubkey = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let member_address = Pubkey::new_unique();

        // Initialize variations
        let init_ix = initialize(
            &spl_token_2022::id(),
            &mint_pubkey,
            Some(authority),
            Some(member_address),
        )
        .unwrap();
        let mut message = Message::new(&[init_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeGroupMemberPointer".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "authority": authority.to_string(),
                    "memberAddress": member_address.to_string(),
                })
            }
        );

        let init_ix = initialize(&spl_token_2022::id(), &mint_pubkey, None, None).unwrap();
        let mut message = Message::new(&[init_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeGroupMemberPointer".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                })
            }
        );

        // Single owner Update
        let update_ix = update(
            &spl_token_2022::id(),
            &mint_pubkey,
            &authority,
            &[],
            Some(member_address),
        )
        .unwrap();
        let mut message = Message::new(&[update_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateGroupMemberPointer".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "authority": authority.to_string(),
                    "memberAddress": member_address.to_string(),
                })
            }
        );

        // Multisig Update
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let update_ix = update(
            &spl_token_2022::id(),
            &mint_pubkey,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            Some(member_address),
        )
        .unwrap();
        let mut message = Message::new(&[update_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateGroupMemberPointer".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "memberAddress": member_address.to_string(),
                    "multisigAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                })
            }
        );
    }
}
