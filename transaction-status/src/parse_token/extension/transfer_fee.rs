use {super::*, spl_token_2022::extension::transfer_fee::instruction::TransferFeeInstruction};

pub(in crate::parse_token) fn parse_transfer_fee_instruction(
    transfer_fee_instruction: TransferFeeInstruction,
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match transfer_fee_instruction {
        TransferFeeInstruction::InitializeTransferFeeConfig {
            transfer_fee_config_authority,
            withdraw_withheld_authority,
            transfer_fee_basis_points,
            maximum_fee,
        } => {
            check_num_token_accounts(account_indexes, 1)?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "transferFeeBasisPoints": transfer_fee_basis_points,
                "maximumFee": maximum_fee,
            });
            let map = value.as_object_mut().unwrap();
            if let COption::Some(transfer_fee_config_authority) = transfer_fee_config_authority {
                map.insert(
                    "transferFeeConfigAuthority".to_string(),
                    json!(transfer_fee_config_authority.to_string()),
                );
            }
            if let COption::Some(withdraw_withheld_authority) = withdraw_withheld_authority {
                map.insert(
                    "withdrawWithheldAuthority".to_string(),
                    json!(withdraw_withheld_authority.to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeTransferFeeConfig".to_string(),
                info: value,
            })
        }
        TransferFeeInstruction::TransferCheckedWithFee {
            amount,
            decimals,
            fee,
        } => {
            check_num_token_accounts(account_indexes, 4)?;
            let mut value = json!({
                "source": account_keys[account_indexes[0] as usize].to_string(),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "destination": account_keys[account_indexes[2] as usize].to_string(),
                "tokenAmount": token_amount_to_ui_amount(amount, decimals),
                "feeAmount": token_amount_to_ui_amount(fee, decimals),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                3,
                account_keys,
                account_indexes,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "transferCheckedWithFee".to_string(),
                info: value,
            })
        }
        TransferFeeInstruction::WithdrawWithheldTokensFromMint => {
            check_num_token_accounts(account_indexes, 3)?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "feeRecipient": account_keys[account_indexes[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                2,
                account_keys,
                account_indexes,
                "withdrawWithheldAuthority",
                "multisigWithdrawWithheldAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawWithheldTokensFromMint".to_string(),
                info: value,
            })
        }
        TransferFeeInstruction::WithdrawWithheldTokensFromAccounts { num_token_accounts } => {
            check_num_token_accounts(account_indexes, 3 + num_token_accounts as usize)?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "feeRecipient": account_keys[account_indexes[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            let mut source_accounts: Vec<String> = vec![];
            let first_source_account_index = account_indexes
                .len()
                .saturating_sub(num_token_accounts as usize);
            for i in account_indexes[first_source_account_index..].iter() {
                source_accounts.push(account_keys[*i as usize].to_string());
            }
            map.insert("sourceAccounts".to_string(), json!(source_accounts));
            parse_signers(
                map,
                2,
                account_keys,
                &account_indexes[..first_source_account_index],
                "withdrawWithheldAuthority",
                "multisigWithdrawWithheldAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawWithheldTokensFromAccounts".to_string(),
                info: value,
            })
        }
        TransferFeeInstruction::HarvestWithheldTokensToMint => {
            check_num_token_accounts(account_indexes, 1)?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            let mut source_accounts: Vec<String> = vec![];
            for i in account_indexes.iter().skip(1) {
                source_accounts.push(account_keys[*i as usize].to_string());
            }
            map.insert("sourceAccounts".to_string(), json!(source_accounts));
            Ok(ParsedInstructionEnum {
                instruction_type: "harvestWithheldTokensToMint".to_string(),
                info: value,
            })
        }
        TransferFeeInstruction::SetTransferFee {
            transfer_fee_basis_points,
            maximum_fee,
        } => {
            check_num_token_accounts(account_indexes, 2)?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "transferFeeBasisPoints": transfer_fee_basis_points,
                "maximumFee": maximum_fee,
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                1,
                account_keys,
                account_indexes,
                "transferFeeConfigAuthority",
                "multisigtransferFeeConfigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "setTransferFee".to_string(),
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
            extension::transfer_fee::instruction::*, solana_program::message::Message,
        },
    };

    #[test]
    fn test_parse_transfer_fee_instruction() {
        let mint_pubkey = Pubkey::new_unique();
        let transfer_fee_config_authority = Pubkey::new_unique();
        let withdraw_withheld_authority = Pubkey::new_unique();
        let transfer_fee_basis_points = 42;
        let maximum_fee = 2121;

        // InitializeTransferFeeConfig variations
        let init_transfer_fee_config_ix = initialize_transfer_fee_config(
            &spl_token_2022::id(),
            &mint_pubkey,
            Some(&transfer_fee_config_authority),
            Some(&withdraw_withheld_authority),
            transfer_fee_basis_points,
            maximum_fee,
        )
        .unwrap();
        let message = Message::new(&[init_transfer_fee_config_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeTransferFeeConfig".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "transferFeeConfigAuthority": transfer_fee_config_authority.to_string(),
                    "withdrawWithheldAuthority": withdraw_withheld_authority.to_string(),
                    "transferFeeBasisPoints": transfer_fee_basis_points,
                    "maximumFee": maximum_fee,
                })
            }
        );

        let init_transfer_fee_config_ix = initialize_transfer_fee_config(
            &spl_token_2022::id(),
            &mint_pubkey,
            None,
            None,
            transfer_fee_basis_points,
            maximum_fee,
        )
        .unwrap();
        let message = Message::new(&[init_transfer_fee_config_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeTransferFeeConfig".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "transferFeeBasisPoints": transfer_fee_basis_points,
                    "maximumFee": maximum_fee,
                })
            }
        );

        // Single owner TransferCheckedWithFee
        let account_pubkey = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        let amount = 55;
        let decimals = 2;
        let fee = 5;
        let transfer_checked_with_fee_ix = transfer_checked_with_fee(
            &spl_token_2022::id(),
            &account_pubkey,
            &mint_pubkey,
            &recipient,
            &owner,
            &[],
            amount,
            decimals,
            fee,
        )
        .unwrap();
        let message = Message::new(&[transfer_checked_with_fee_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transferCheckedWithFee".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "authority": owner.to_string(),
                    "tokenAmount": {
                        "uiAmount": 0.55,
                        "decimals": 2,
                        "amount": "55",
                        "uiAmountString": "0.55",
                   },
                    "feeAmount": {
                        "uiAmount": 0.05,
                        "decimals": 2,
                        "amount": "5",
                        "uiAmountString": "0.05",
                   },
                })
            }
        );

        // Multisig TransferCheckedWithFee
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let transfer_checked_with_fee_ix = transfer_checked_with_fee(
            &spl_token_2022::id(),
            &account_pubkey,
            &mint_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            amount,
            decimals,
            fee,
        )
        .unwrap();
        let message = Message::new(&[transfer_checked_with_fee_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transferCheckedWithFee".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "multisigAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "tokenAmount": {
                        "uiAmount": 0.55,
                        "decimals": 2,
                        "amount": "55",
                        "uiAmountString": "0.55",
                   },
                    "feeAmount": {
                        "uiAmount": 0.05,
                        "decimals": 2,
                        "amount": "5",
                        "uiAmountString": "0.05",
                   },
                })
            }
        );

        // Single authority WithdrawWithheldTokensFromMint
        let withdraw_withheld_tokens_from_mint_ix = withdraw_withheld_tokens_from_mint(
            &spl_token_2022::id(),
            &mint_pubkey,
            &recipient,
            &withdraw_withheld_authority,
            &[],
        )
        .unwrap();
        let message = Message::new(&[withdraw_withheld_tokens_from_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdrawWithheldTokensFromMint".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "feeRecipient": recipient.to_string(),
                    "withdrawWithheldAuthority": withdraw_withheld_authority.to_string(),
                })
            }
        );

        // Multisig WithdrawWithheldTokensFromMint
        let withdraw_withheld_tokens_from_mint_ix = withdraw_withheld_tokens_from_mint(
            &spl_token_2022::id(),
            &mint_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
        )
        .unwrap();
        let message = Message::new(&[withdraw_withheld_tokens_from_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdrawWithheldTokensFromMint".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "feeRecipient": recipient.to_string(),
                    "multisigWithdrawWithheldAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                })
            }
        );

        // Single authority WithdrawWithheldTokensFromAccounts
        let fee_account0 = Pubkey::new_unique();
        let fee_account1 = Pubkey::new_unique();
        let withdraw_withheld_tokens_from_accounts_ix = withdraw_withheld_tokens_from_accounts(
            &spl_token_2022::id(),
            &mint_pubkey,
            &recipient,
            &withdraw_withheld_authority,
            &[],
            &[&fee_account0, &fee_account1],
        )
        .unwrap();
        let message = Message::new(&[withdraw_withheld_tokens_from_accounts_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdrawWithheldTokensFromAccounts".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "feeRecipient": recipient.to_string(),
                    "withdrawWithheldAuthority": withdraw_withheld_authority.to_string(),
                    "sourceAccounts": vec![
                        fee_account0.to_string(),
                        fee_account1.to_string(),
                    ],
                })
            }
        );

        // Multisig WithdrawWithheldTokensFromAccounts
        let withdraw_withheld_tokens_from_accounts_ix = withdraw_withheld_tokens_from_accounts(
            &spl_token_2022::id(),
            &mint_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            &[&fee_account0, &fee_account1],
        )
        .unwrap();
        let message = Message::new(&[withdraw_withheld_tokens_from_accounts_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdrawWithheldTokensFromAccounts".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "feeRecipient": recipient.to_string(),
                    "multisigWithdrawWithheldAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "sourceAccounts": vec![
                        fee_account0.to_string(),
                        fee_account1.to_string(),
                    ],
                })
            }
        );

        // HarvestWithheldTokensToMint
        let harvest_withheld_tokens_to_mint_ix = harvest_withheld_tokens_to_mint(
            &spl_token_2022::id(),
            &mint_pubkey,
            &[&fee_account0, &fee_account1],
        )
        .unwrap();
        let message = Message::new(&[harvest_withheld_tokens_to_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "harvestWithheldTokensToMint".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "sourceAccounts": vec![
                        fee_account0.to_string(),
                        fee_account1.to_string(),
                    ],
                })
            }
        );

        // Single authority SetTransferFee
        let set_transfer_fee_ix = set_transfer_fee(
            &spl_token_2022::id(),
            &mint_pubkey,
            &transfer_fee_config_authority,
            &[],
            transfer_fee_basis_points,
            maximum_fee,
        )
        .unwrap();
        let message = Message::new(&[set_transfer_fee_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setTransferFee".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "transferFeeBasisPoints": transfer_fee_basis_points,
                    "maximumFee": maximum_fee,
                    "transferFeeConfigAuthority": transfer_fee_config_authority.to_string(),
                })
            }
        );

        // Multisig WithdrawWithheldTokensFromMint
        let set_transfer_fee_ix = set_transfer_fee(
            &spl_token_2022::id(),
            &mint_pubkey,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            transfer_fee_basis_points,
            maximum_fee,
        )
        .unwrap();
        let message = Message::new(&[set_transfer_fee_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setTransferFee".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "transferFeeBasisPoints": transfer_fee_basis_points,
                    "maximumFee": maximum_fee,
                    "multisigtransferFeeConfigAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                })
            }
        );
    }
}
