use {
    super::*,
    spl_token_2022::{
        extension::cpi_guard::instruction::CpiGuardInstruction,
        instruction::decode_instruction_type,
    },
};

pub(in crate::parse_token) fn parse_cpi_guard_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    check_num_token_accounts(account_indexes, 2)?;
    let instruction_type_str = match decode_instruction_type(instruction_data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    {
        CpiGuardInstruction::Enable => "enable",
        CpiGuardInstruction::Disable => "disable",
    };
    let mut value = json!({
        "account": account_keys[account_indexes[0] as usize].to_string(),
    });
    let map = value.as_object_mut().unwrap();
    parse_signers(
        map,
        1,
        account_keys,
        account_indexes,
        "owner",
        "multisigOwner",
    );
    Ok(ParsedInstructionEnum {
        instruction_type: format!("{instruction_type_str}CpiGuard"),
        info: value,
    })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::parse_token::test::*,
        solana_sdk::pubkey::Pubkey,
        spl_token_2022::{
            extension::cpi_guard::instruction::{disable_cpi_guard, enable_cpi_guard},
            solana_program::message::Message,
        },
    };

    #[test]
    fn test_parse_cpi_guard_instruction() {
        let account_pubkey = Pubkey::new_unique();

        // Enable, single owner
        let owner_pubkey = Pubkey::new_unique();
        let enable_cpi_guard_ix = enable_cpi_guard(
            &spl_token_2022::id(),
            &convert_pubkey(account_pubkey),
            &convert_pubkey(owner_pubkey),
            &[],
        )
        .unwrap();
        let message = Message::new(&[enable_cpi_guard_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(
                &compiled_instruction,
                &AccountKeys::new(&convert_account_keys(&message), None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "enableCpiGuard".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "owner": owner_pubkey.to_string(),
                })
            }
        );

        // Enable, multisig owner
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let enable_cpi_guard_ix = enable_cpi_guard(
            &spl_token_2022::id(),
            &convert_pubkey(account_pubkey),
            &convert_pubkey(multisig_pubkey),
            &[
                &convert_pubkey(multisig_signer0),
                &convert_pubkey(multisig_signer1),
            ],
        )
        .unwrap();
        let message = Message::new(&[enable_cpi_guard_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(
                &compiled_instruction,
                &AccountKeys::new(&convert_account_keys(&message), None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "enableCpiGuard".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "multisigOwner": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                })
            }
        );

        // Disable, single owner
        let enable_cpi_guard_ix = disable_cpi_guard(
            &spl_token_2022::id(),
            &convert_pubkey(account_pubkey),
            &convert_pubkey(owner_pubkey),
            &[],
        )
        .unwrap();
        let message = Message::new(&[enable_cpi_guard_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(
                &compiled_instruction,
                &AccountKeys::new(&convert_account_keys(&message), None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "disableCpiGuard".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "owner": owner_pubkey.to_string(),
                })
            }
        );

        // Enable, multisig owner
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let enable_cpi_guard_ix = disable_cpi_guard(
            &spl_token_2022::id(),
            &convert_pubkey(account_pubkey),
            &convert_pubkey(multisig_pubkey),
            &[
                &convert_pubkey(multisig_signer0),
                &convert_pubkey(multisig_signer1),
            ],
        )
        .unwrap();
        let message = Message::new(&[enable_cpi_guard_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(
                &compiled_instruction,
                &AccountKeys::new(&convert_account_keys(&message), None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "disableCpiGuard".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "multisigOwner": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                })
            }
        );
    }
}
