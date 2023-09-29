use {
    super::*,
    spl_token_2022::extension::default_account_state::instruction::{
        decode_instruction, DefaultAccountStateInstruction,
    },
};

pub(in crate::parse_token) fn parse_default_account_state_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let (default_account_state_instruction, account_state) = decode_instruction(instruction_data)
        .map_err(|_| {
        ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
    })?;
    let instruction_type = "DefaultAccountState";
    match default_account_state_instruction {
        DefaultAccountStateInstruction::Initialize => {
            check_num_token_accounts(account_indexes, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: format!("initialize{instruction_type}"),
                info: json!({
                    "mint": account_keys[account_indexes[0] as usize].to_string(),
                    "accountState": UiAccountState::from(account_state),
                }),
            })
        }
        DefaultAccountStateInstruction::Update => {
            check_num_token_accounts(account_indexes, 2)?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "accountState": UiAccountState::from(account_state),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                1,
                account_keys,
                account_indexes,
                "freezeAuthority",
                "multisigFreezeAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: format!("update{instruction_type}"),
                info: value,
            })
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::pubkey::Pubkey,
        spl_token_2022::{
            extension::default_account_state::instruction::{
                initialize_default_account_state, update_default_account_state,
            },
            solana_program::message::Message,
            state::AccountState,
        },
    };

    #[test]
    fn test_parse_default_account_state_instruction() {
        let mint_pubkey = Pubkey::new_unique();
        let init_default_account_state_ix = initialize_default_account_state(
            &spl_token_2022::id(),
            &mint_pubkey,
            &AccountState::Frozen,
        )
        .unwrap();
        let message = Message::new(&[init_default_account_state_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeDefaultAccountState".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "accountState": "frozen",
                })
            }
        );

        // Single mint freeze_authority
        let mint_freeze_authority = Pubkey::new_unique();
        let update_default_account_state_ix = update_default_account_state(
            &spl_token_2022::id(),
            &mint_pubkey,
            &mint_freeze_authority,
            &[],
            &AccountState::Initialized,
        )
        .unwrap();
        let message = Message::new(&[update_default_account_state_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateDefaultAccountState".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "accountState": "initialized",
                    "freezeAuthority": mint_freeze_authority.to_string(),
                })
            }
        );

        // Multisig mint freeze_authority
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let update_default_account_state_ix = update_default_account_state(
            &spl_token_2022::id(),
            &mint_pubkey,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            &AccountState::Initialized,
        )
        .unwrap();
        let message = Message::new(&[update_default_account_state_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "updateDefaultAccountState".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "accountState": "initialized",
                    "multisigFreezeAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                })
            }
        );
    }
}
