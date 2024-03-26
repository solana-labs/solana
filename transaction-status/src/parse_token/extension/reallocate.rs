use {super::*, spl_token_2022::extension::ExtensionType};

pub(in crate::parse_token) fn parse_reallocate_instruction(
    extension_types: Vec<ExtensionType>,
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    check_num_token_accounts(account_indexes, 4)?;
    let mut value = json!({
        "account": account_keys[account_indexes[0] as usize].to_string(),
        "payer": account_keys[account_indexes[1] as usize].to_string(),
        "systemProgram": account_keys[account_indexes[2] as usize].to_string(),
        "extensionTypes": extension_types.into_iter().map(UiExtensionType::from).collect::<Vec<_>>(),
    });
    let map = value.as_object_mut().unwrap();
    parse_signers(
        map,
        3,
        account_keys,
        account_indexes,
        "owner",
        "multisigOwner",
    );
    Ok(ParsedInstructionEnum {
        instruction_type: "reallocate".to_string(),
        info: value,
    })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::pubkey::Pubkey,
        spl_token_2022::{instruction::reallocate, solana_program::message::Message},
    };

    #[test]
    fn test_parse_reallocate_instruction() {
        let account_pubkey = Pubkey::new_unique();
        let payer_pubkey = Pubkey::new_unique();

        let extension_types = vec![
            ExtensionType::TransferFeeAmount,
            ExtensionType::MemoTransfer,
        ];

        // Single owner
        let owner_pubkey = Pubkey::new_unique();
        let reallocate_ix = reallocate(
            &spl_token_2022::id(),
            &account_pubkey,
            &payer_pubkey,
            &owner_pubkey,
            &[],
            &extension_types,
        )
        .unwrap();
        let message = Message::new(&[reallocate_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "reallocate".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "payer": payer_pubkey.to_string(),
                    "owner": owner_pubkey.to_string(),
                    "systemProgram": solana_sdk::system_program::id().to_string(),
                    "extensionTypes": ["transferFeeAmount", "memoTransfer"],
                })
            }
        );

        // Multisig owner
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let reallocate_ix = reallocate(
            &spl_token_2022::id(),
            &account_pubkey,
            &payer_pubkey,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            &extension_types,
        )
        .unwrap();
        let message = Message::new(&[reallocate_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "reallocate".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "payer": payer_pubkey.to_string(),
                    "multisigOwner": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "systemProgram": solana_sdk::system_program::id().to_string(),
                    "extensionTypes": ["transferFeeAmount", "memoTransfer"],
                })
            }
        );
    }
}
