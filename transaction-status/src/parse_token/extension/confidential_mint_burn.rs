use {
    super::*,
    spl_token_2022::{
        extension::confidential_mint_burn::instruction::*,
        instruction::{decode_instruction_data, decode_instruction_type},
    },
};

pub(in crate::parse_token) fn parse_confidential_mint_burn_instruction(
    instruction_data: &[u8],
    account_indexes: &[u8],
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match decode_instruction_type(instruction_data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    {
        ConfidentialMintBurnInstruction::InitializeMint => {
            check_num_token_accounts(account_indexes, 1)?;
            let initialize_mint_data: InitializeMintData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "supplyElGamalPubkey": initialize_mint_data.supply_elgamal_pubkey.to_string(),
                "decryptableSupply": initialize_mint_data.decryptable_supply.to_string(),
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeConfidentialMintBurnMint".to_string(),
                info: value,
            })
        }
        ConfidentialMintBurnInstruction::UpdateDecryptableSupply => {
            check_num_token_accounts(account_indexes, 2)?;
            let update_decryptable_supply: UpdateDecryptableSupplyData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "newDecryptableSupply": update_decryptable_supply.new_decryptable_supply.to_string(),

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
                instruction_type: "updateConfidentialMintBurnDecryptableSupply".to_string(),
                info: value,
            })
        }
        ConfidentialMintBurnInstruction::RotateSupplyElGamalPubkey => {
            check_num_token_accounts(account_indexes, 3)?;
            let rotate_supply_data: RotateSupplyElGamalPubkeyData =
                *decode_instruction_data(instruction_data).map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "mint": account_keys[account_indexes[0] as usize].to_string(),
                "newSupplyElGamalPubkey": rotate_supply_data.new_supply_elgamal_pubkey.to_string(),
                "proofInstructionOffset": rotate_supply_data.proof_instruction_offset,

            });
            let map = value.as_object_mut().unwrap();
            if rotate_supply_data.proof_instruction_offset == 0 {
                map.insert(
                    "proofAccount".to_string(),
                    json!(account_keys[account_indexes[1] as usize].to_string()),
                );
            } else {
                map.insert(
                    "instructionsSysvar".to_string(),
                    json!(account_keys[account_indexes[1] as usize].to_string()),
                );
            }
            parse_signers(
                map,
                2,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "rotateConfidentialMintBurnSupplyElGamalPubkey".to_string(),
                info: value,
            })
        }
        ConfidentialMintBurnInstruction::Mint => {
            check_num_token_accounts(account_indexes, 3)?;
            let mint_data: MintInstructionData = *decode_instruction_data(instruction_data)
                .map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "destination": account_keys[account_indexes[0] as usize].to_string(),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "newDecryptableSupply": mint_data.new_decryptable_supply.to_string(),
                "equalityProofInstructionOffset": mint_data.equality_proof_instruction_offset,
                "ciphertextValidityProofInstructionOffset": mint_data.ciphertext_validity_proof_instruction_offset,
                "rangeProofInstructionOffset": mint_data.range_proof_instruction_offset,

            });
            let mut offset = 2;
            let map = value.as_object_mut().unwrap();
            if mint_data.equality_proof_instruction_offset != 0
                || mint_data.ciphertext_validity_proof_instruction_offset != 0
                || mint_data.range_proof_instruction_offset != 0
            {
                map.insert(
                    "instructionsSysvar".to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }

            // Assume that extra accounts are proof accounts and not multisig
            // signers. This might be wrong, but it's the best possible option.
            if offset < account_indexes.len() - 1 {
                let label = if mint_data.equality_proof_instruction_offset == 0 {
                    "equalityProofContextStateAccount"
                } else {
                    "equalityProofRecordAccount"
                };
                map.insert(
                    label.to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }
            if offset < account_indexes.len() - 1 {
                let label = if mint_data.ciphertext_validity_proof_instruction_offset == 0 {
                    "ciphertextValidityProofContextStateAccount"
                } else {
                    "ciphertextValidityProofRecordAccount"
                };
                map.insert(
                    label.to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }
            if offset < account_indexes.len() - 1 {
                let label = if mint_data.range_proof_instruction_offset == 0 {
                    "rangeProofContextStateAccount"
                } else {
                    "rangeProofRecordAccount"
                };
                map.insert(
                    label.to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }

            parse_signers(
                map,
                offset,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "confidentialMint".to_string(),
                info: value,
            })
        }
        ConfidentialMintBurnInstruction::Burn => {
            check_num_token_accounts(account_indexes, 3)?;
            let burn_data: BurnInstructionData = *decode_instruction_data(instruction_data)
                .map_err(|_| {
                    ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken)
                })?;
            let mut value = json!({
                "destination": account_keys[account_indexes[0] as usize].to_string(),
                "mint": account_keys[account_indexes[1] as usize].to_string(),
                "newDecryptableAvailableBalance": burn_data.new_decryptable_available_balance.to_string(),
                "equalityProofInstructionOffset": burn_data.equality_proof_instruction_offset,
                "ciphertextValidityProofInstructionOffset": burn_data.ciphertext_validity_proof_instruction_offset,
                "rangeProofInstructionOffset": burn_data.range_proof_instruction_offset,

            });
            let mut offset = 2;
            let map = value.as_object_mut().unwrap();
            if burn_data.equality_proof_instruction_offset != 0
                || burn_data.ciphertext_validity_proof_instruction_offset != 0
                || burn_data.range_proof_instruction_offset != 0
            {
                map.insert(
                    "instructionsSysvar".to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }

            // Assume that extra accounts are proof accounts and not multisig
            // signers. This might be wrong, but it's the best possible option.
            if offset < account_indexes.len() - 1 {
                let label = if burn_data.equality_proof_instruction_offset == 0 {
                    "equalityProofContextStateAccount"
                } else {
                    "equalityProofRecordAccount"
                };
                map.insert(
                    label.to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }
            if offset < account_indexes.len() - 1 {
                let label = if burn_data.ciphertext_validity_proof_instruction_offset == 0 {
                    "ciphertextValidityProofContextStateAccount"
                } else {
                    "ciphertextValidityProofRecordAccount"
                };
                map.insert(
                    label.to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }
            if offset < account_indexes.len() - 1 {
                let label = if burn_data.range_proof_instruction_offset == 0 {
                    "rangeProofContextStateAccount"
                } else {
                    "rangeProofRecordAccount"
                };
                map.insert(
                    label.to_string(),
                    json!(account_keys[account_indexes[offset] as usize].to_string()),
                );
                offset += 1;
            }

            parse_signers(
                map,
                offset,
                account_keys,
                account_indexes,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "confidentialBurn".to_string(),
                info: value,
            })
        }
    }
}
