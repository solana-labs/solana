use {super::*, spl_token_2022::solana_program::pubkey::Pubkey};

pub(in crate::parse_token) fn parse_initialize_permanent_delegate_instruction(
    delegate: Pubkey,
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    check_num_token_accounts(account_indexes, 1)?;
    Ok(ParsedInstructionEnum {
        instruction_type: "initializePermanentDelegate".to_string(),
        info: json!({
            "mint": account_keys[account_indexes[0] as usize].to_string(),
            "delegate": delegate.to_string(),
        }),
    })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::pubkey::Pubkey,
        spl_token_2022::{instruction::*, solana_program::message::Message},
    };

    #[test]
    fn test_parse_initialize_permanent_delegate_instruction() {
        let mint_pubkey = Pubkey::new_unique();
        let delegate = Pubkey::new_unique();
        let permanent_delegate_ix =
            initialize_permanent_delegate(&spl_token_2022::id(), &mint_pubkey, &delegate).unwrap();
        let message = Message::new(&[permanent_delegate_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializePermanentDelegate".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "delegate": delegate.to_string(),
                })
            }
        );
    }
}
