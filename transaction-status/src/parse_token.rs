use crate::parse_instruction::{ParsableProgram, ParseInstructionError, ParsedInstructionEnum};
use serde_json::{json, Map, Value};
use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};
use spl_token_v2_0::{
    instruction::{AuthorityType, TokenInstruction},
    option::COption,
};

pub fn parse_token(
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let token_instruction = TokenInstruction::unpack(&instruction.data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?;
    if instruction.accounts.len() > account_keys.len() {
        // Runtime should prevent this from ever happening
        return Err(ParseInstructionError::InstructionKeyMismatch(
            ParsableProgram::SplToken,
        ));
    }
    match token_instruction {
        TokenInstruction::InitializeMint {
            decimals,
            mint_authority,
            freeze_authority,
        } => {
            if instruction.accounts.len() < 2 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                "decimals": decimals,
                "mintAuthority": mint_authority.to_string(),
                "rentSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if let COption::Some(freeze_authority) = freeze_authority {
                map.insert(
                    "freezeAuthority".to_string(),
                    json!(freeze_authority.to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeMint".to_string(),
                info: value,
            })
        }
        TokenInstruction::InitializeAccount => {
            if instruction.accounts.len() < 4 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeAccount".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                    "owner": account_keys[instruction.accounts[2] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[3] as usize].to_string(),
                }),
            })
        }
        TokenInstruction::InitializeMultisig { m } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut signers: Vec<String> = vec![];
            for i in instruction.accounts[2..].iter() {
                signers.push(account_keys[*i as usize].to_string());
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeMultisig".to_string(),
                info: json!({
                    "multisig": account_keys[instruction.accounts[0] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
                    "signers": signers,
                    "m": m,
                }),
            })
        }
        TokenInstruction::Transfer { amount } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "destination": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: value,
            })
        }
        TokenInstruction::Approve { amount } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "delegate": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "approve".to_string(),
                info: value,
            })
        }
        TokenInstruction::Revoke => {
            if instruction.accounts.len() < 2 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                1,
                account_keys,
                &instruction.accounts,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "revoke".to_string(),
                info: value,
            })
        }
        TokenInstruction::SetAuthority {
            authority_type,
            new_authority,
        } => {
            if instruction.accounts.len() < 2 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let owned = match authority_type {
                AuthorityType::MintTokens | AuthorityType::FreezeAccount => "mint",
                AuthorityType::AccountOwner | AuthorityType::CloseAccount => "account",
            };
            let mut value = json!({
                owned: account_keys[instruction.accounts[0] as usize].to_string(),
                "authorityType": Into::<UiAuthorityType>::into(authority_type),
                "newAuthority": match new_authority {
                    COption::Some(authority) => Some(authority.to_string()),
                    COption::None => None,
                },
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                1,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: value,
            })
        }
        TokenInstruction::MintTo { amount } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                "account": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "mintAuthority",
                "multisigMintAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "mintTo".to_string(),
                info: value,
            })
        }
        TokenInstruction::Burn { amount } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "burn".to_string(),
                info: value,
            })
        }
        TokenInstruction::CloseAccount => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "destination": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "closeAccount".to_string(),
                info: value,
            })
        }
        TokenInstruction::FreezeAccount => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "freezeAuthority",
                "multisigFreezeAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "freezeAccount".to_string(),
                info: value,
            })
        }
        TokenInstruction::ThawAccount => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "freezeAuthority",
                "multisigFreezeAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "thawAccount".to_string(),
                info: value,
            })
        }
        TokenInstruction::Transfer2 { amount, decimals } => {
            if instruction.accounts.len() < 4 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "destination": account_keys[instruction.accounts[2] as usize].to_string(),
                "amount": amount,
                "decimals": decimals,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                3,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "transfer2".to_string(),
                info: value,
            })
        }
        TokenInstruction::Approve2 { amount, decimals } => {
            if instruction.accounts.len() < 4 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "delegate": account_keys[instruction.accounts[2] as usize].to_string(),
                "amount": amount,
                "decimals": decimals,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                3,
                account_keys,
                &instruction.accounts,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "approve2".to_string(),
                info: value,
            })
        }
        TokenInstruction::MintTo2 { amount, decimals } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                "account": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount,
                "decimals": decimals,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "mintAuthority",
                "multisigMintAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "mintTo2".to_string(),
                info: value,
            })
        }
        TokenInstruction::Burn2 { amount, decimals } => {
            if instruction.accounts.len() < 3 {
                return Err(ParseInstructionError::InstructionKeyMismatch(
                    ParsableProgram::SplToken,
                ));
            }
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount,
                "decimals": decimals,
            });
            let mut map = value.as_object_mut().unwrap();
            parse_signers(
                &mut map,
                2,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "burn2".to_string(),
                info: value,
            })
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiAuthorityType {
    MintTokens,
    FreezeAccount,
    AccountOwner,
    CloseAccount,
}

impl From<AuthorityType> for UiAuthorityType {
    fn from(authority_type: AuthorityType) -> Self {
        match authority_type {
            AuthorityType::MintTokens => UiAuthorityType::MintTokens,
            AuthorityType::FreezeAccount => UiAuthorityType::FreezeAccount,
            AuthorityType::AccountOwner => UiAuthorityType::AccountOwner,
            AuthorityType::CloseAccount => UiAuthorityType::CloseAccount,
        }
    }
}

fn parse_signers(
    map: &mut Map<String, Value>,
    last_nonsigner_index: usize,
    account_keys: &[Pubkey],
    accounts: &[u8],
    owner_field_name: &str,
    multisig_field_name: &str,
) {
    if accounts.len() > last_nonsigner_index + 1 {
        let mut signers: Vec<String> = vec![];
        for i in accounts[last_nonsigner_index + 1..].iter() {
            signers.push(account_keys[*i as usize].to_string());
        }
        map.insert(
            multisig_field_name.to_string(),
            json!(account_keys[accounts[last_nonsigner_index] as usize].to_string()),
        );
        map.insert("signers".to_string(), json!(signers));
    } else {
        map.insert(
            owner_field_name.to_string(),
            json!(account_keys[accounts[last_nonsigner_index] as usize].to_string()),
        );
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::instruction::CompiledInstruction;
    use spl_token_v2_0::{
        instruction::*,
        solana_sdk::{
            instruction::CompiledInstruction as SplTokenCompiledInstruction, message::Message,
            pubkey::Pubkey as SplTokenPubkey,
        },
    };
    use std::str::FromStr;

    fn convert_pubkey(pubkey: Pubkey) -> SplTokenPubkey {
        SplTokenPubkey::from_str(&pubkey.to_string()).unwrap()
    }

    fn convert_compiled_instruction(
        instruction: &SplTokenCompiledInstruction,
    ) -> CompiledInstruction {
        CompiledInstruction {
            program_id_index: instruction.program_id_index,
            accounts: instruction.accounts.clone(),
            data: instruction.data.clone(),
        }
    }

    #[test]
    #[allow(clippy::same_item_push)]
    fn test_parse_token() {
        let mut keys: Vec<Pubkey> = vec![];
        for _ in 0..10 {
            keys.push(Pubkey::new_rand());
        }

        // Test InitializeMint variations
        let initialize_mint_ix = initialize_mint(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[2]),
            Some(&convert_pubkey(keys[3])),
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMint".to_string(),
                info: json!({
                    "mint": keys[0].to_string(),
                    "decimals": 2,
                    "mintAuthority": keys[2].to_string(),
                    "freezeAuthority": keys[3].to_string(),
                    "rentSysvar": keys[1].to_string(),
                })
            }
        );

        let initialize_mint_ix = initialize_mint(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[2]),
            None,
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMint".to_string(),
                info: json!({
                   "mint": keys[0].to_string(),
                   "decimals": 2,
                   "mintAuthority": keys[2].to_string(),
                   "rentSysvar": keys[1].to_string(),
                })
            }
        );

        // Test InitializeAccount
        let initialize_account_ix = initialize_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
        )
        .unwrap();
        let message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeAccount".to_string(),
                info: json!({
                   "account": keys[0].to_string(),
                   "mint": keys[1].to_string(),
                   "owner": keys[2].to_string(),
                   "rentSysvar": keys[3].to_string(),
                })
            }
        );

        // Test InitializeMultisig
        let initialize_multisig_ix = initialize_multisig(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &[
                &convert_pubkey(keys[2]),
                &convert_pubkey(keys[3]),
                &convert_pubkey(keys[4]),
            ],
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_multisig_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMultisig".to_string(),
                info: json!({
                   "multisig": keys[0].to_string(),
                   "m": 2,
                   "rentSysvar": keys[1].to_string(),
                   "signers": keys[2..5].iter().map(|key| key.to_string()).collect::<Vec<String>>(),
                })
            }
        );

        // Test Transfer, incl multisig
        let transfer_ix = transfer(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: json!({
                   "source": keys[1].to_string(),
                   "destination": keys[2].to_string(),
                   "authority": keys[0].to_string(),
                   "amount": 42,
                })
            }
        );

        let transfer_ix = transfer(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: json!({
                   "source": keys[2].to_string(),
                   "destination": keys[3].to_string(),
                   "multisigAuthority": keys[4].to_string(),
                   "signers": keys[0..2].iter().map(|key| key.to_string()).collect::<Vec<String>>(),
                   "amount": 42,
                })
            }
        );

        // Test Approve, incl multisig
        let approve_ix = approve(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approve".to_string(),
                info: json!({
                   "source": keys[1].to_string(),
                   "delegate": keys[2].to_string(),
                   "owner": keys[0].to_string(),
                   "amount": 42,
                })
            }
        );

        let approve_ix = approve(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approve".to_string(),
                info: json!({
                   "source": keys[2].to_string(),
                   "delegate": keys[3].to_string(),
                   "multisigOwner": keys[4].to_string(),
                   "signers": keys[0..2].iter().map(|key| key.to_string()).collect::<Vec<String>>(),
                   "amount": 42,
                })
            }
        );

        // Test Revoke
        let revoke_ix = revoke(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[revoke_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "revoke".to_string(),
                info: json!({
                   "source": keys[1].to_string(),
                   "owner": keys[0].to_string(),
                })
            }
        );

        // Test SetOwner
        let set_authority_ix = set_authority(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            Some(&convert_pubkey(keys[2])),
            AuthorityType::FreezeAccount,
            &convert_pubkey(keys[1]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[set_authority_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                   "mint": keys[1].to_string(),
                   "newAuthority": keys[2].to_string(),
                   "authority": keys[0].to_string(),
                   "authorityType": "freezeAccount".to_string(),
                })
            }
        );

        let set_authority_ix = set_authority(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            None,
            AuthorityType::CloseAccount,
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[set_authority_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        let new_authority: Option<String> = None;
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                   "account": keys[1].to_string(),
                   "newAuthority": new_authority,
                   "authority": keys[0].to_string(),
                   "authorityType": "closeAccount".to_string(),
                })
            }
        );

        // Test MintTo
        let mint_to_ix = mint_to(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[mint_to_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "mintTo".to_string(),
                info: json!({
                   "mint": keys[1].to_string(),
                   "account": keys[2].to_string(),
                   "mintAuthority": keys[0].to_string(),
                   "amount": 42,
                })
            }
        );

        // Test Burn
        let burn_ix = burn(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[burn_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "burn".to_string(),
                info: json!({
                   "account": keys[1].to_string(),
                   "mint": keys[2].to_string(),
                   "authority": keys[0].to_string(),
                   "amount": 42,
                })
            }
        );

        // Test CloseAccount
        let close_account_ix = close_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[close_account_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "closeAccount".to_string(),
                info: json!({
                   "account": keys[1].to_string(),
                   "destination": keys[2].to_string(),
                   "owner": keys[0].to_string(),
                })
            }
        );

        // Test FreezeAccount
        let freeze_account_ix = freeze_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[freeze_account_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "freezeAccount".to_string(),
                info: json!({
                   "account": keys[1].to_string(),
                   "mint": keys[2].to_string(),
                   "freezeAuthority": keys[0].to_string(),
                })
            }
        );

        // Test ThawAccount
        let thaw_account_ix = thaw_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[thaw_account_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "thawAccount".to_string(),
                info: json!({
                   "account": keys[1].to_string(),
                   "mint": keys[2].to_string(),
                   "freezeAuthority": keys[0].to_string(),
                })
            }
        );

        // Test Transfer2, incl multisig
        let transfer_ix = transfer2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer2".to_string(),
                info: json!({
                   "source": keys[1].to_string(),
                   "destination": keys[2].to_string(),
                   "mint": keys[3].to_string(),
                   "authority": keys[0].to_string(),
                   "amount": 42,
                   "decimals": 2,
                })
            }
        );

        let transfer_ix = transfer2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &convert_pubkey(keys[5]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer2".to_string(),
                info: json!({
                   "source": keys[2].to_string(),
                   "destination": keys[3].to_string(),
                   "mint": keys[4].to_string(),
                   "multisigAuthority": keys[5].to_string(),
                   "signers": keys[0..2].iter().map(|key| key.to_string()).collect::<Vec<String>>(),
                   "amount": 42,
                   "decimals": 2,
                })
            }
        );

        // Test Approve2, incl multisig
        let approve_ix = approve2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approve2".to_string(),
                info: json!({
                   "source": keys[1].to_string(),
                   "mint": keys[2].to_string(),
                   "delegate": keys[3].to_string(),
                   "owner": keys[0].to_string(),
                   "amount": 42,
                   "decimals": 2,
                })
            }
        );

        let approve_ix = approve2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &convert_pubkey(keys[5]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approve2".to_string(),
                info: json!({
                   "source": keys[2].to_string(),
                   "mint": keys[3].to_string(),
                   "delegate": keys[4].to_string(),
                   "multisigOwner": keys[5].to_string(),
                   "signers": keys[0..2].iter().map(|key| key.to_string()).collect::<Vec<String>>(),
                   "amount": 42,
                   "decimals": 2,
                })
            }
        );

        // Test MintTo2
        let mint_to_ix = mint_to2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[mint_to_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "mintTo2".to_string(),
                info: json!({
                   "mint": keys[1].to_string(),
                   "account": keys[2].to_string(),
                   "mintAuthority": keys[0].to_string(),
                   "amount": 42,
                   "decimals": 2,
                })
            }
        );

        // Test Burn2
        let burn_ix = burn2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[burn_ix], None);
        let compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert_eq!(
            parse_token(&compiled_instruction, &keys).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "burn2".to_string(),
                info: json!({
                   "account": keys[1].to_string(),
                   "mint": keys[2].to_string(),
                   "authority": keys[0].to_string(),
                   "amount": 42,
                   "decimals": 2,
                })
            }
        );
    }

    #[test]
    #[allow(clippy::same_item_push)]
    fn test_token_ix_not_enough_keys() {
        let mut keys: Vec<Pubkey> = vec![];
        for _ in 0..10 {
            keys.push(Pubkey::new_rand());
        }

        // Test InitializeMint variations
        let initialize_mint_ix = initialize_mint(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[1]),
            Some(&convert_pubkey(keys[2])),
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..1]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        let initialize_mint_ix = initialize_mint(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[1]),
            None,
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..1]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test InitializeAccount
        let initialize_account_ix = initialize_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
        )
        .unwrap();
        let message = Message::new(&[initialize_account_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..3]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test InitializeMultisig
        let initialize_multisig_ix = initialize_multisig(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[0]),
            &[
                &convert_pubkey(keys[1]),
                &convert_pubkey(keys[2]),
                &convert_pubkey(keys[3]),
            ],
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_multisig_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..4]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Transfer, incl multisig
        let transfer_ix = transfer(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        let transfer_ix = transfer(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..4]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Approve, incl multisig
        let approve_ix = approve(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        let approve_ix = approve(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..4]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Revoke
        let revoke_ix = revoke(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[revoke_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..1]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test SetAuthority
        let set_authority_ix = set_authority(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            Some(&convert_pubkey(keys[2])),
            AuthorityType::FreezeAccount,
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[set_authority_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..1]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test MintTo
        let mint_to_ix = mint_to(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[mint_to_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Burn
        let burn_ix = burn(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[burn_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test CloseAccount
        let close_account_ix = close_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[close_account_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test FreezeAccount
        let freeze_account_ix = freeze_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[freeze_account_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test ThawAccount
        let thaw_account_ix = thaw_account(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
        )
        .unwrap();
        let message = Message::new(&[thaw_account_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Transfer2, incl multisig
        let transfer_ix = transfer2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..3]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        let transfer_ix = transfer2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &convert_pubkey(keys[5]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..5]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Approve2, incl multisig
        let approve_ix = approve2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..3]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        let approve_ix = approve2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[3]),
            &convert_pubkey(keys[4]),
            &convert_pubkey(keys[5]),
            &[&convert_pubkey(keys[0]), &convert_pubkey(keys[1])],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..5]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test MintTo2
        let mint_to_ix = mint_to2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[mint_to_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());

        // Test Burn2
        let burn_ix = burn2(
            &spl_token_v2_0::id(),
            &convert_pubkey(keys[1]),
            &convert_pubkey(keys[2]),
            &convert_pubkey(keys[0]),
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[burn_ix], None);
        let mut compiled_instruction = convert_compiled_instruction(&message.instructions[0]);
        assert!(parse_token(&compiled_instruction, &keys[0..2]).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(&compiled_instruction, &keys).is_err());
    }
}
