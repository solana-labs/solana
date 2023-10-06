use {
    super::*,
    solana_account_decoder::parse_token_extension::UiConfidentialTransferMint,
    spl_token_2022::{
        extension::confidential_transfer::{instruction::*, ConfidentialTransferMint},
        instruction::{decode_instruction_data, decode_instruction_type},
    },
};

pub(in crate::parse_token) fn parse_confidential_transfer_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match decode_instruction_type(instruction_data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    {
        ConfidentialTransferInstruction::InitializeMint => {
            check_num_token_accounts(account_indexes, 1)?;
            let confidential_transfer_mint: ConfidentialTransferMint =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let confidential_transfer_mint: UiConfidentialTransferMint =
                confidential_transfer_mint.into();
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            map.append(json!(confidential_transfer_mint).as_object_mut().unwrap());
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeConfidentialTransferMint".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::UpdateMint => {
            check_num_token_accounts(account_indexes, 3)?;
            let confidential_transfer_mint: ConfidentialTransferMint =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let confidential_transfer_mint: UiConfidentialTransferMint =
                confidential_transfer_mint.into();
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "confidentialTransferMintAuthority": account_keys[account_indexes[1] as usize].to_string(),
                "newConfidentialTransferMintAuthority": account_keys[account_indexes[2] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            map.append(json!(confidential_transfer_mint).as_object_mut().unwrap());
            Ok(ParsedInstructionEnum {
                instruction_type: "updateConfidentialTransferMint".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::ConfigureAccount => {
            check_num_token_accounts(account_indexes, 3)?;
            let configure_account_data: ConfigureAccountInstructionData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let maximum_pending_balance_credit_counter: u64 = configure_account_data
                .maximum_pending_balance_credit_counter
                .into();
            let mut value = json!({
                "account": account_keys[account_indexes[0] as usize].to_string(),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "decryptableZeroBalance": format!("{}", configure_account_data.decryptable_zero_balance),
                "maximumPendingBalanceCreditCounter": maximum_pending_balance_credit_counter,

            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                2,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "configureConfidentialTransferAccount".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::ApproveAccount => {
            check_num_token_accounts(account_indexes, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "approveConfidentialTransferAccount".to_string(),
                info: json!({
                    "account": account_keys[account_indexes[0] as usize].to_string(),
                    "mint": account_keys[account_indexes[1] as usize].to_string(),
                    "confidentialTransferAuditorAuthority": account_keys[account_indexes[2] as usize].to_string(),
                }),
            })
        }
        ConfidentialTransferInstruction::EmptyAccount => {
            check_num_token_accounts(account_indexes, 3)?;
            let empty_account_data: EmptyAccountInstructionData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let proof_instruction_offset: i8 = empty_account_data.proof_instruction_offset;
            let mut value = json!({
                "account": account_keys[account_indexes[0] as usize].to_string(),
                "instructionsSysvar": account_keys[account_indexes[1] as usize].to_string(),
                "proofInstructionOffset": proof_instruction_offset,

            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                2,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "emptyConfidentialTransferAccount".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::Deposit => {
            check_num_token_accounts(account_indexes, 4)?;
            let deposit_data: DepositInstructionData = *decode_instruction_data(instruction_data)
                .map_err(|_| {
                ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
            })?;
            let amount: u64 = deposit_data.amount.into();
            let mut value = json!({
                "source": account_keys[account_indexes[0] as usize].to_string(),
                "destination": account_keys[account_indexes[1] as usize].to_string(),
                "mint": account_keys[account_indexes[2] as usize].to_string(),
                "amount": amount,
                "decimals": deposit_data.decimals,

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
                instruction_type: "depositConfidentialTransfer".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::Withdraw => {
            check_num_token_accounts(account_indexes, 5)?;
            let withdrawal_data: WithdrawInstructionData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let amount: u64 = withdrawal_data.amount.into();
            let proof_instruction_offset: i8 = withdrawal_data.proof_instruction_offset;
            let mut value = json!({
                "source": account_keys[account_indexes[0] as usize].to_string(),
                "destination": account_keys[account_indexes[1] as usize].to_string(),
                "mint": account_keys[account_indexes[2] as usize].to_string(),
                "instructionsSysvar": account_keys[account_indexes[3] as usize].to_string(),
                "amount": amount,
                "decimals": withdrawal_data.decimals,
                "newDecryptableAvailableBalance": format!("{}", withdrawal_data.new_decryptable_available_balance),
                "proofInstructionOffset": proof_instruction_offset,

            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                4,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawConfidentialTransfer".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::Transfer => {
            check_num_token_accounts(account_indexes, 5)?;
            let transfer_data: TransferInstructionData = *decode_instruction_data(instruction_data)
                .map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let proof_instruction_offset: i8 = transfer_data.proof_instruction_offset;
            let mut value = json!({
                "source": account_keys[account_indexes[0] as usize].to_string(),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "destination": account_keys[account_indexes[2] as usize].to_string(),
                "instructionsSysvar": account_keys[account_indexes[3] as usize].to_string(),
                "newSourceDecryptableAvailableBalance": format!("{}", transfer_data.new_source_decryptable_available_balance),
                "proofInstructionOffset": proof_instruction_offset,

            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                4,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "confidentialTransfer".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::ApplyPendingBalance => {
            check_num_token_accounts(account_indexes, 2)?;
            let apply_pending_balance_data: ApplyPendingBalanceData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let expected_pending_balance_credit_counter: u64 = apply_pending_balance_data
                .expected_pending_balance_credit_counter
                .into();
            let mut value = json!({
                "account": account_keys[account_indexes[0] as usize].to_string(),
                "newDecryptableAvailableBalance": format!("{}", apply_pending_balance_data.new_decryptable_available_balance),
                "expectedPendingBalanceCreditCounter": expected_pending_balance_credit_counter,

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
                instruction_type: "applyPendingConfidentialTransferBalance".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::EnableConfidentialCredits => {
            check_num_token_accounts(account_indexes, 2)?;
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
                instruction_type: "enableConfidentialTransferConfidentialCredits".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::DisableConfidentialCredits => {
            check_num_token_accounts(account_indexes, 2)?;
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
                instruction_type: "disableConfidentialTransferConfidentialCredits".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::EnableNonConfidentialCredits => {
            check_num_token_accounts(account_indexes, 2)?;
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
                instruction_type: "enableConfidentialTransferNonConfidentialCredits".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::DisableNonConfidentialCredits => {
            check_num_token_accounts(account_indexes, 2)?;
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
                instruction_type: "disableNonConfidentialTransferConfidentialCredits".to_string(),
                info: value,
            })
        }
        ConfidentialTransferInstruction::TransferWithSplitProofs => {
            check_num_token_accounts(account_indexes, 7)?;
            let transfer_data: TransferWithSplitProofsInstructionData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "source": account_keys[account_indexes[0] as usize].to_string(),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "destination": account_keys[account_indexes[2] as usize].to_string(),
                "ciphertextCommitmentEqualityContext": account_keys[account_indexes[3] as usize].to_string(),
                "batchedGroupedCiphertext2HandlesValidityContext": account_keys[account_indexes[4] as usize].to_string(),
                "batchedRangeProofContext": account_keys[account_indexes[5] as usize].to_string(),
                "owner": account_keys[account_indexes[6] as usize].to_string(),
                "newSourceDecryptableAvailableBalance": format!("{}", transfer_data.new_source_decryptable_available_balance),
                "noOpOnUninitializedSplitContextState": bool::from(transfer_data.no_op_on_uninitialized_split_context_state),
                "closeSplitContextStateOnExecution": bool::from(transfer_data.close_split_context_state_on_execution),
            });
            let map = value.as_object_mut().unwrap();
            if transfer_data.close_split_context_state_on_execution.into() {
                map.insert(
                    "lamportDestination".to_string(),
                    json!(account_keys[account_indexes[7] as usize].to_string()),
                );
                map.insert(
                    "contextStateOwner".to_string(),
                    json!(account_keys[account_indexes[8] as usize].to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "confidentialTransferWithSplitProofs".to_string(),
                info: value,
            })
        }
    }
}
