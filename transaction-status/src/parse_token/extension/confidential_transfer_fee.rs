use {
    super::*,
    solana_account_decoder::parse_token_extension::UiConfidentialTransferFeeConfig,
    spl_token_2022::{
        extension::confidential_transfer_fee::{instruction::*, ConfidentialTransferFeeConfig},
        instruction::{decode_instruction_data, decode_instruction_type},
    },
};

pub(in crate::parse_token) fn parse_confidential_transfer_fee_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match decode_instruction_type(instruction_data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    {
        ConfidentialTransferFeeInstruction::InitializeConfidentialTransferFeeConfig => {
            check_num_token_accounts(account_indexes, 1)?;
            let confidential_transfer_mint: ConfidentialTransferFeeConfig =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let confidential_transfer_mint: UiConfidentialTransferFeeConfig =
                confidential_transfer_mint.into();
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            map.append(json!(confidential_transfer_mint).as_object_mut().unwrap());
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeConfidentialTransferFeeConfig".to_string(),
                info: value,
            })
        }
        ConfidentialTransferFeeInstruction::WithdrawWithheldTokensFromMint => {
            check_num_token_accounts(account_indexes, 4)?;
            let withdraw_withheld_data: WithdrawWithheldTokensFromMintData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let proof_instruction_offset: i8 = withdraw_withheld_data.proof_instruction_offset;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "feeRecipient": account_keys[account_indexes[1] as usize].to_string(),
                "instructionsSysvar": account_keys[account_indexes[2] as usize].to_string(),
                "proofInstructionOffset": proof_instruction_offset,

            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                3,
                account_keys,
                account_indexes,
                "withdrawWithheldAuthority",
                "multisigWithdrawWithheldAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawWithheldConfidentialTransferTokensFromMint".to_string(),
                info: value,
            })
        }
        ConfidentialTransferFeeInstruction::WithdrawWithheldTokensFromAccounts => {
            let withdraw_withheld_data: WithdrawWithheldTokensFromAccountsData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let num_token_accounts = withdraw_withheld_data.num_token_accounts;
            check_num_token_accounts(account_indexes, 4 + num_token_accounts as usize)?;
            let proof_instruction_offset: i8 = withdraw_withheld_data.proof_instruction_offset;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "feeRecipient": account_keys[account_indexes[1] as usize].to_string(),
                "instructionsSysvar": account_keys[account_indexes[2] as usize].to_string(),
                "proofInstructionOffset": proof_instruction_offset,
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
                3,
                account_keys,
                &account_indexes[..first_source_account_index],
                "withdrawWithheldAuthority",
                "multisigWithdrawWithheldAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawWithheldConfidentialTransferTokensFromAccounts"
                    .to_string(),
                info: value,
            })
        }
        ConfidentialTransferFeeInstruction::HarvestWithheldTokensToMint => {
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
                instruction_type: "harvestWithheldConfidentialTransferTokensToMint".to_string(),
                info: value,
            })
        }
        ConfidentialTransferFeeInstruction::EnableHarvestToMint => {
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
                instruction_type: "enableConfidentialTransferFeeHarvestToMint".to_string(),
                info: value,
            })
        }
        ConfidentialTransferFeeInstruction::DisableHarvestToMint => {
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
                instruction_type: "disableConfidentialTransferFeeHarvestToMint".to_string(),
                info: value,
            })
        }
    }
}
