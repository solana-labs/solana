use {
    super::*,
    spl_token_group_interface::instruction::{
        InitializeGroup, TokenGroupInstruction, UpdateGroupAuthority, UpdateGroupMaxSize,
    },
};

pub(in crate::parse_token) fn parse_token_group_instruction(
    instruction: &TokenGroupInstruction,
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match instruction {
        TokenGroupInstruction::InitializeGroup(group) => {
            check_num_token_accounts(account_indexes, 3)?;
            let InitializeGroup {
                max_size,
                update_authority,
            } = group;
            let value = json!({
                "group": account_keys[account_indexes[0] as usize].to_string(),
                "maxSize": u32::from(*max_size),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "mintAuthority": account_keys[account_indexes[2] as usize].to_string(),
                "updateAuthority": Option::<Pubkey>::from(*update_authority).map(|v| v.to_string())
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeTokenGroup".to_string(),
                info: value,
            })
        }
        TokenGroupInstruction::UpdateGroupMaxSize(update) => {
            check_num_token_accounts(account_indexes, 2)?;
            let UpdateGroupMaxSize { max_size } = update;
            let value = json!({
                "group": account_keys[account_indexes[0] as usize].to_string(),
                "maxSize": u32::from(*max_size),
                "updateAuthority": account_keys[account_indexes[1] as usize].to_string(),
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "updateTokenGroupMaxSize".to_string(),
                info: value,
            })
        }
        TokenGroupInstruction::UpdateGroupAuthority(update) => {
            check_num_token_accounts(account_indexes, 2)?;
            let UpdateGroupAuthority { new_authority } = update;
            let value = json!({
                "group": account_keys[account_indexes[0] as usize].to_string(),
                "updateAuthority": account_keys[account_indexes[1] as usize].to_string(),
                "newAuthority": Option::<Pubkey>::from(*new_authority).map(|v| v.to_string())
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "updateTokenGroupAuthority".to_string(),
                info: value,
            })
        }
        TokenGroupInstruction::InitializeMember(_) => {
            check_num_token_accounts(account_indexes, 5)?;
            let value = json!({
                "member": account_keys[account_indexes[0] as usize].to_string(),
                "memberMint": account_keys[account_indexes[1] as usize].to_string(),
                "memberMintAuthority": account_keys[account_indexes[2] as usize].to_string(),
                "group": account_keys[account_indexes[3] as usize].to_string(),
                "groupUpdateAuthority": account_keys[account_indexes[4] as usize].to_string(),
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeTokenGroupMember".to_string(),
                info: value,
            })
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, solana_sdk::pubkey::Pubkey, spl_token_2022::solana_program::message::Message};

    #[test]
    fn test_parse_token_group_instruction() {
        let group_mint = Pubkey::new_unique();
        let group_mint_authority = Pubkey::new_unique();
        let group_update_authority = Pubkey::new_unique();
        let group_address = Pubkey::new_unique();
        let member_mint = Pubkey::new_unique();
        let member_mint_authority = Pubkey::new_unique();
        let member_address = Pubkey::new_unique();

        // InitializeGroup
        // Initialize with an update authority.
        let max_size = 300;
        let ix = spl_token_group_interface::instruction::initialize_group(
            &spl_token_2022::id(),
            &group_address,
            &group_mint,
            &group_mint_authority,
            Some(group_update_authority),
            max_size,
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
                instruction_type: "initializeTokenGroup".to_string(),
                info: json!({
                    "group": group_address.to_string(),
                    "maxSize": max_size,
                    "mint": group_mint.to_string(),
                    "mintAuthority": group_mint_authority.to_string(),
                    "updateAuthority": group_update_authority.to_string(),
                })
            }
        );
        // Initialize without an update authority.
        let ix = spl_token_group_interface::instruction::initialize_group(
            &spl_token_2022::id(),
            &group_address,
            &group_mint,
            &group_mint_authority,
            None,
            max_size,
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
                instruction_type: "initializeTokenGroup".to_string(),
                info: json!({
                    "group": group_address.to_string(),
                    "maxSize": max_size,
                    "mint": group_mint.to_string(),
                    "mintAuthority": group_mint_authority.to_string(),
                    "updateAuthority": null,
                })
            }
        );

        // UpdateGroupMaxSize
        let new_max_size = 500;
        let ix = spl_token_group_interface::instruction::update_group_max_size(
            &spl_token_2022::id(),
            &group_address,
            &group_update_authority,
            new_max_size,
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
                instruction_type: "updateTokenGroupMaxSize".to_string(),
                info: json!({
                    "group": group_address.to_string(),
                    "maxSize": new_max_size,
                    "updateAuthority": group_update_authority.to_string(),
                })
            }
        );

        // UpdateGroupAuthority
        // Update authority to a new authority.
        let new_authority = Pubkey::new_unique();
        let ix = spl_token_group_interface::instruction::update_group_authority(
            &spl_token_2022::id(),
            &group_address,
            &group_update_authority,
            Some(new_authority),
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
                instruction_type: "updateTokenGroupAuthority".to_string(),
                info: json!({
                    "group": group_address.to_string(),
                    "updateAuthority": group_update_authority.to_string(),
                    "newAuthority": new_authority.to_string(),
                })
            }
        );
        // Update authority to None.
        let ix = spl_token_group_interface::instruction::update_group_authority(
            &spl_token_2022::id(),
            &group_address,
            &group_update_authority,
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
                instruction_type: "updateTokenGroupAuthority".to_string(),
                info: json!({
                    "group": group_address.to_string(),
                    "updateAuthority": group_update_authority.to_string(),
                    "newAuthority": null,
                })
            }
        );

        // InitializeMember
        let ix = spl_token_group_interface::instruction::initialize_member(
            &spl_token_2022::id(),
            &member_address,
            &member_mint,
            &member_mint_authority,
            &group_address,
            &group_update_authority,
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
                instruction_type: "initializeTokenGroupMember".to_string(),
                info: json!({
                    "member": member_address.to_string(),
                    "memberMint": member_mint.to_string(),
                    "memberMintAuthority": member_mint_authority.to_string(),
                    "group": group_address.to_string(),
                    "groupUpdateAuthority": group_update_authority.to_string(),
                })
            }
        );
    }
}
