use {
    crate::parse_instruction::{
        check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
    },
    borsh::BorshDeserialize,
    serde_json::json,
    solana_sdk::{instruction::CompiledInstruction, message::AccountKeys, pubkey::Pubkey},
    spl_associated_token_account::instruction::AssociatedTokenAccountInstruction,
};

// A helper function to convert spl_associated_token_account::id() as spl_sdk::pubkey::Pubkey
// to solana_sdk::pubkey::Pubkey
pub fn spl_associated_token_id() -> Pubkey {
    Pubkey::new_from_array(spl_associated_token_account::id().to_bytes())
}

pub fn parse_associated_token(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    match instruction.accounts.iter().max() {
        Some(index) if (*index as usize) < account_keys.len() => {}
        _ => {
            // Runtime should prevent this from ever happening
            return Err(ParseInstructionError::InstructionKeyMismatch(
                ParsableProgram::SplAssociatedTokenAccount,
            ));
        }
    }
    let ata_instruction = if instruction.data.is_empty() {
        AssociatedTokenAccountInstruction::Create
    } else {
        AssociatedTokenAccountInstruction::try_from_slice(&instruction.data)
            .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?
    };

    match ata_instruction {
        AssociatedTokenAccountInstruction::Create => {
            check_num_associated_token_accounts(&instruction.accounts, 6)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "create".to_string(),
                info: json!({
                    "source": account_keys[instruction.accounts[0] as usize].to_string(),
                    "account": account_keys[instruction.accounts[1] as usize].to_string(),
                    "wallet": account_keys[instruction.accounts[2] as usize].to_string(),
                    "mint": account_keys[instruction.accounts[3] as usize].to_string(),
                    "systemProgram": account_keys[instruction.accounts[4] as usize].to_string(),
                    "tokenProgram": account_keys[instruction.accounts[5] as usize].to_string(),
                }),
            })
        }
        AssociatedTokenAccountInstruction::CreateIdempotent => {
            check_num_associated_token_accounts(&instruction.accounts, 6)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "createIdempotent".to_string(),
                info: json!({
                    "source": account_keys[instruction.accounts[0] as usize].to_string(),
                    "account": account_keys[instruction.accounts[1] as usize].to_string(),
                    "wallet": account_keys[instruction.accounts[2] as usize].to_string(),
                    "mint": account_keys[instruction.accounts[3] as usize].to_string(),
                    "systemProgram": account_keys[instruction.accounts[4] as usize].to_string(),
                    "tokenProgram": account_keys[instruction.accounts[5] as usize].to_string(),
                }),
            })
        }
        AssociatedTokenAccountInstruction::RecoverNested => {
            check_num_associated_token_accounts(&instruction.accounts, 7)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "recoverNested".to_string(),
                info: json!({
                    "nestedSource": account_keys[instruction.accounts[0] as usize].to_string(),
                    "nestedMint": account_keys[instruction.accounts[1] as usize].to_string(),
                    "destination": account_keys[instruction.accounts[2] as usize].to_string(),
                    "nestedOwner": account_keys[instruction.accounts[3] as usize].to_string(),
                    "ownerMint": account_keys[instruction.accounts[4] as usize].to_string(),
                    "wallet": account_keys[instruction.accounts[5] as usize].to_string(),
                    "tokenProgram": account_keys[instruction.accounts[6] as usize].to_string(),
                }),
            })
        }
    }
}

fn check_num_associated_token_accounts(
    accounts: &[u8],
    num: usize,
) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::SplAssociatedTokenAccount)
}

#[cfg(test)]
mod test {
    #[allow(deprecated)]
    use spl_associated_token_account::create_associated_token_account as create_associated_token_account_deprecated;
    use {
        super::*,
        solana_sdk::{message::Message, sysvar},
        spl_associated_token_account::{
            get_associated_token_address, get_associated_token_address_with_program_id,
            instruction::{
                create_associated_token_account, create_associated_token_account_idempotent,
                recover_nested,
            },
        },
    };

    #[test]
    fn test_parse_create_deprecated() {
        let funder = Pubkey::new_unique();
        let wallet_address = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let associated_account_address = get_associated_token_address(&wallet_address, &mint);
        #[allow(deprecated)]
        let create_ix = create_associated_token_account_deprecated(&funder, &wallet_address, &mint);
        let mut message = Message::new(&[create_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        let expected_parsed_ix = ParsedInstructionEnum {
            instruction_type: "create".to_string(),
            info: json!({
                "source": funder.to_string(),
                "account": associated_account_address.to_string(),
                "wallet": wallet_address.to_string(),
                "mint": mint.to_string(),
                "systemProgram": solana_sdk::system_program::id().to_string(),
                "tokenProgram": spl_token::id().to_string(),
            }),
        };
        assert_eq!(
            parse_associated_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            expected_parsed_ix,
        );

        // after popping rent account, parsing should still succeed
        let rent_account_index = compiled_instruction
            .accounts
            .iter()
            .position(|index| message.account_keys[*index as usize] == sysvar::rent::id())
            .unwrap();
        compiled_instruction.accounts.remove(rent_account_index);
        assert_eq!(
            parse_associated_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            expected_parsed_ix,
        );

        // after popping another account, parsing should fail
        compiled_instruction.accounts.pop();
        assert!(parse_associated_token(
            compiled_instruction,
            &AccountKeys::new(&message.account_keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_create() {
        let funder = Pubkey::new_unique();
        let wallet_address = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();
        let associated_account_address =
            get_associated_token_address_with_program_id(&wallet_address, &mint, &token_program_id);
        let create_ix =
            create_associated_token_account(&funder, &wallet_address, &mint, &token_program_id);
        let mut message = Message::new(&[create_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_associated_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "create".to_string(),
                info: json!({
                    "source": funder.to_string(),
                    "account": associated_account_address.to_string(),
                    "wallet": wallet_address.to_string(),
                    "mint": mint.to_string(),
                    "systemProgram": solana_sdk::system_program::id().to_string(),
                    "tokenProgram": token_program_id.to_string(),
                })
            }
        );
        compiled_instruction.accounts.pop();
        assert!(parse_associated_token(
            compiled_instruction,
            &AccountKeys::new(&message.account_keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_create_idempotent() {
        let funder = Pubkey::new_unique();
        let wallet_address = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();
        let associated_account_address =
            get_associated_token_address_with_program_id(&wallet_address, &mint, &token_program_id);
        let create_ix = create_associated_token_account_idempotent(
            &funder,
            &wallet_address,
            &mint,
            &token_program_id,
        );
        let mut message = Message::new(&[create_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_associated_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "createIdempotent".to_string(),
                info: json!({
                    "source": funder.to_string(),
                    "account": associated_account_address.to_string(),
                    "wallet": wallet_address.to_string(),
                    "mint": mint.to_string(),
                    "systemProgram": solana_sdk::system_program::id().to_string(),
                    "tokenProgram": token_program_id.to_string(),
                })
            }
        );
        compiled_instruction.accounts.pop();
        assert!(parse_associated_token(
            compiled_instruction,
            &AccountKeys::new(&message.account_keys, None)
        )
        .is_err());
    }

    #[test]
    fn test_parse_recover_nested() {
        let wallet_address = Pubkey::new_unique();
        let owner_mint = Pubkey::new_unique();
        let nested_mint = Pubkey::new_unique();
        let token_program_id = Pubkey::new_unique();
        let owner_associated_account_address = get_associated_token_address_with_program_id(
            &wallet_address,
            &owner_mint,
            &token_program_id,
        );
        let nested_associated_account_address = get_associated_token_address_with_program_id(
            &owner_associated_account_address,
            &nested_mint,
            &token_program_id,
        );
        let destination_associated_account_address = get_associated_token_address_with_program_id(
            &wallet_address,
            &nested_mint,
            &token_program_id,
        );
        let recover_ix = recover_nested(
            &wallet_address,
            &owner_mint,
            &nested_mint,
            &token_program_id,
        );
        let mut message = Message::new(&[recover_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert_eq!(
            parse_associated_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "recoverNested".to_string(),
                info: json!({
                    "nestedSource": nested_associated_account_address.to_string(),
                    "nestedMint": nested_mint.to_string(),
                    "destination": destination_associated_account_address.to_string(),
                    "nestedOwner": owner_associated_account_address.to_string(),
                    "ownerMint": owner_mint.to_string(),
                    "wallet": wallet_address.to_string(),
                    "tokenProgram": token_program_id.to_string(),
                })
            }
        );
        compiled_instruction.accounts.pop();
        assert!(parse_associated_token(
            compiled_instruction,
            &AccountKeys::new(&message.account_keys, None)
        )
        .is_err());
    }
}
