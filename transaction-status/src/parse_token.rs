use {
    crate::parse_instruction::{
        check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
    },
    extension::{
        confidential_transfer::*, confidential_transfer_fee::*, cpi_guard::*,
        default_account_state::*, group_member_pointer::*, group_pointer::*,
        interest_bearing_mint::*, memo_transfer::*, metadata_pointer::*, mint_close_authority::*,
        permanent_delegate::*, reallocate::*, transfer_fee::*, transfer_hook::*,
    },
    serde_json::{json, Map, Value},
    solana_account_decoder::parse_token::{token_amount_to_ui_amount, UiAccountState},
    solana_sdk::{
        instruction::{AccountMeta, CompiledInstruction, Instruction},
        message::AccountKeys,
    },
    spl_token_2022::{
        extension::ExtensionType,
        instruction::{AuthorityType, TokenInstruction},
        solana_program::{
            instruction::Instruction as SplTokenInstruction, program_option::COption,
            pubkey::Pubkey,
        },
    },
};

mod extension;

pub fn parse_token(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let token_instruction = TokenInstruction::unpack(&instruction.data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::SplToken))?;
    match instruction.accounts.iter().max() {
        Some(index) if (*index as usize) < account_keys.len() => {}
        _ => {
            // Runtime should prevent this from ever happening
            return Err(ParseInstructionError::InstructionKeyMismatch(
                ParsableProgram::SplToken,
            ));
        }
    }
    match token_instruction {
        TokenInstruction::InitializeMint {
            decimals,
            mint_authority,
            freeze_authority,
        } => {
            check_num_token_accounts(&instruction.accounts, 2)?;
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
        TokenInstruction::InitializeMint2 {
            decimals,
            mint_authority,
            freeze_authority,
        } => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                "decimals": decimals,
                "mintAuthority": mint_authority.to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if let COption::Some(freeze_authority) = freeze_authority {
                map.insert(
                    "freezeAuthority".to_string(),
                    json!(freeze_authority.to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeMint2".to_string(),
                info: value,
            })
        }
        TokenInstruction::InitializeAccount => {
            check_num_token_accounts(&instruction.accounts, 4)?;
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
        TokenInstruction::InitializeAccount2 { owner } => {
            check_num_token_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeAccount2".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                    "owner": owner.to_string(),
                    "rentSysvar": account_keys[instruction.accounts[2] as usize].to_string(),
                }),
            })
        }
        TokenInstruction::InitializeAccount3 { owner } => {
            check_num_token_accounts(&instruction.accounts, 2)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeAccount3".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                    "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                    "owner": owner.to_string(),
                }),
            })
        }
        TokenInstruction::InitializeMultisig { m } => {
            check_num_token_accounts(&instruction.accounts, 3)?;
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
        TokenInstruction::InitializeMultisig2 { m } => {
            check_num_token_accounts(&instruction.accounts, 2)?;
            let mut signers: Vec<String> = vec![];
            for i in instruction.accounts[1..].iter() {
                signers.push(account_keys[*i as usize].to_string());
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeMultisig2".to_string(),
                info: json!({
                    "multisig": account_keys[instruction.accounts[0] as usize].to_string(),
                    "signers": signers,
                    "m": m,
                }),
            })
        }
        #[allow(deprecated)]
        TokenInstruction::Transfer { amount } => {
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "destination": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount.to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "delegate": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount.to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 2)?;
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 2)?;
            let owned = match authority_type {
                AuthorityType::MintTokens
                | AuthorityType::FreezeAccount
                | AuthorityType::TransferFeeConfig
                | AuthorityType::WithheldWithdraw
                | AuthorityType::CloseMint
                | AuthorityType::InterestRate
                | AuthorityType::PermanentDelegate
                | AuthorityType::ConfidentialTransferMint
                | AuthorityType::TransferHookProgramId
                | AuthorityType::ConfidentialTransferFeeConfig
                | AuthorityType::MetadataPointer
                | AuthorityType::GroupPointer
                | AuthorityType::GroupMemberPointer => "mint",
                AuthorityType::AccountOwner | AuthorityType::CloseAccount => "account",
            };
            let mut value = json!({
                owned: account_keys[instruction.accounts[0] as usize].to_string(),
                "authorityType": Into::<UiAuthorityType>::into(authority_type),
                "newAuthority": map_coption_pubkey(new_authority),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                "account": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount.to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "amount": amount.to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "destination": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
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
        TokenInstruction::TransferChecked { amount, decimals } => {
            check_num_token_accounts(&instruction.accounts, 4)?;
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "destination": account_keys[instruction.accounts[2] as usize].to_string(),
                "tokenAmount": token_amount_to_ui_amount(amount, decimals),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                3,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "transferChecked".to_string(),
                info: value,
            })
        }
        TokenInstruction::ApproveChecked { amount, decimals } => {
            check_num_token_accounts(&instruction.accounts, 4)?;
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "delegate": account_keys[instruction.accounts[2] as usize].to_string(),
                "tokenAmount": token_amount_to_ui_amount(amount, decimals),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                3,
                account_keys,
                &instruction.accounts,
                "owner",
                "multisigOwner",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "approveChecked".to_string(),
                info: value,
            })
        }
        TokenInstruction::MintToChecked { amount, decimals } => {
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                "account": account_keys[instruction.accounts[1] as usize].to_string(),
                "tokenAmount": token_amount_to_ui_amount(amount, decimals),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                2,
                account_keys,
                &instruction.accounts,
                "mintAuthority",
                "multisigMintAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "mintToChecked".to_string(),
                info: value,
            })
        }
        TokenInstruction::BurnChecked { amount, decimals } => {
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "account": account_keys[instruction.accounts[0] as usize].to_string(),
                "mint": account_keys[instruction.accounts[1] as usize].to_string(),
                "tokenAmount": token_amount_to_ui_amount(amount, decimals),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                2,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "burnChecked".to_string(),
                info: value,
            })
        }
        TokenInstruction::SyncNative => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "syncNative".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                }),
            })
        }
        TokenInstruction::GetAccountDataSize { extension_types } => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            let mut value = json!({
                "mint": account_keys[instruction.accounts[0] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if !extension_types.is_empty() {
                map.insert(
                    "extensionTypes".to_string(),
                    json!(extension_types
                        .into_iter()
                        .map(UiExtensionType::from)
                        .collect::<Vec<_>>()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "getAccountDataSize".to_string(),
                info: value,
            })
        }
        TokenInstruction::InitializeImmutableOwner => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeImmutableOwner".to_string(),
                info: json!({
                    "account": account_keys[instruction.accounts[0] as usize].to_string(),
                }),
            })
        }
        TokenInstruction::AmountToUiAmount { amount } => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "amountToUiAmount".to_string(),
                info: json!({
                    "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                    "amount": amount,
                }),
            })
        }
        TokenInstruction::UiAmountToAmount { ui_amount } => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "uiAmountToAmount".to_string(),
                info: json!({
                    "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                    "uiAmount": ui_amount,
                }),
            })
        }
        TokenInstruction::InitializeMintCloseAuthority { close_authority } => {
            parse_initialize_mint_close_authority_instruction(
                close_authority,
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::TransferFeeExtension(transfer_fee_instruction) => {
            parse_transfer_fee_instruction(
                transfer_fee_instruction,
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::ConfidentialTransferExtension => parse_confidential_transfer_instruction(
            &instruction.data[1..],
            &instruction.accounts,
            account_keys,
        ),
        TokenInstruction::DefaultAccountStateExtension => {
            if instruction.data.len() <= 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_default_account_state_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::Reallocate { extension_types } => {
            parse_reallocate_instruction(extension_types, &instruction.accounts, account_keys)
        }
        TokenInstruction::MemoTransferExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_memo_transfer_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::CreateNativeMint => {
            check_num_token_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "createNativeMint".to_string(),
                info: json!({
                    "payer": account_keys[instruction.accounts[0] as usize].to_string(),
                    "nativeMint": account_keys[instruction.accounts[1] as usize].to_string(),
                    "systemProgram": account_keys[instruction.accounts[2] as usize].to_string(),
                }),
            })
        }
        TokenInstruction::InitializeNonTransferableMint => {
            check_num_token_accounts(&instruction.accounts, 1)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "initializeNonTransferableMint".to_string(),
                info: json!({
                    "mint": account_keys[instruction.accounts[0] as usize].to_string(),
                }),
            })
        }
        TokenInstruction::InterestBearingMintExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_interest_bearing_mint_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::CpiGuardExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_cpi_guard_instruction(&instruction.data[1..], &instruction.accounts, account_keys)
        }
        TokenInstruction::InitializePermanentDelegate { delegate } => {
            parse_initialize_permanent_delegate_instruction(
                delegate,
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::TransferHookExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_transfer_hook_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::ConfidentialTransferFeeExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_confidential_transfer_fee_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::WithdrawExcessLamports => {
            check_num_token_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "source": account_keys[instruction.accounts[0] as usize].to_string(),
                "destination": account_keys[instruction.accounts[1] as usize].to_string(),
            });
            let map = value.as_object_mut().unwrap();
            parse_signers(
                map,
                2,
                account_keys,
                &instruction.accounts,
                "authority",
                "multisigAuthority",
            );
            Ok(ParsedInstructionEnum {
                instruction_type: "withdrawExcessLamports".to_string(),
                info: value,
            })
        }
        TokenInstruction::MetadataPointerExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_metadata_pointer_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::GroupPointerExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_group_pointer_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
        TokenInstruction::GroupMemberPointerExtension => {
            if instruction.data.len() < 2 {
                return Err(ParseInstructionError::InstructionNotParsable(
                    ParsableProgram::SplToken,
                ));
            }
            parse_group_member_pointer_instruction(
                &instruction.data[1..],
                &instruction.accounts,
                account_keys,
            )
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UiAuthorityType {
    MintTokens,
    FreezeAccount,
    AccountOwner,
    CloseAccount,
    TransferFeeConfig,
    WithheldWithdraw,
    CloseMint,
    InterestRate,
    PermanentDelegate,
    ConfidentialTransferMint,
    TransferHookProgramId,
    ConfidentialTransferFeeConfig,
    MetadataPointer,
    GroupPointer,
    GroupMemberPointer,
}

impl From<AuthorityType> for UiAuthorityType {
    fn from(authority_type: AuthorityType) -> Self {
        match authority_type {
            AuthorityType::MintTokens => UiAuthorityType::MintTokens,
            AuthorityType::FreezeAccount => UiAuthorityType::FreezeAccount,
            AuthorityType::AccountOwner => UiAuthorityType::AccountOwner,
            AuthorityType::CloseAccount => UiAuthorityType::CloseAccount,
            AuthorityType::TransferFeeConfig => UiAuthorityType::TransferFeeConfig,
            AuthorityType::WithheldWithdraw => UiAuthorityType::WithheldWithdraw,
            AuthorityType::CloseMint => UiAuthorityType::CloseMint,
            AuthorityType::InterestRate => UiAuthorityType::InterestRate,
            AuthorityType::PermanentDelegate => UiAuthorityType::PermanentDelegate,
            AuthorityType::ConfidentialTransferMint => UiAuthorityType::ConfidentialTransferMint,
            AuthorityType::TransferHookProgramId => UiAuthorityType::TransferHookProgramId,
            AuthorityType::ConfidentialTransferFeeConfig => {
                UiAuthorityType::ConfidentialTransferFeeConfig
            }
            AuthorityType::MetadataPointer => UiAuthorityType::MetadataPointer,
            AuthorityType::GroupPointer => UiAuthorityType::GroupPointer,
            AuthorityType::GroupMemberPointer => UiAuthorityType::GroupMemberPointer,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UiExtensionType {
    Uninitialized,
    TransferFeeConfig,
    TransferFeeAmount,
    MintCloseAuthority,
    ConfidentialTransferMint,
    ConfidentialTransferAccount,
    DefaultAccountState,
    ImmutableOwner,
    MemoTransfer,
    NonTransferable,
    InterestBearingConfig,
    CpiGuard,
    PermanentDelegate,
    NonTransferableAccount,
    TransferHook,
    TransferHookAccount,
    ConfidentialTransferFeeConfig,
    ConfidentialTransferFeeAmount,
    MetadataPointer,
    TokenMetadata,
    GroupPointer,
    GroupMemberPointer,
    TokenGroup,
    TokenGroupMember,
}

impl From<ExtensionType> for UiExtensionType {
    fn from(extension_type: ExtensionType) -> Self {
        match extension_type {
            ExtensionType::Uninitialized => UiExtensionType::Uninitialized,
            ExtensionType::TransferFeeConfig => UiExtensionType::TransferFeeConfig,
            ExtensionType::TransferFeeAmount => UiExtensionType::TransferFeeAmount,
            ExtensionType::MintCloseAuthority => UiExtensionType::MintCloseAuthority,
            ExtensionType::ConfidentialTransferMint => UiExtensionType::ConfidentialTransferMint,
            ExtensionType::ConfidentialTransferAccount => {
                UiExtensionType::ConfidentialTransferAccount
            }
            ExtensionType::DefaultAccountState => UiExtensionType::DefaultAccountState,
            ExtensionType::ImmutableOwner => UiExtensionType::ImmutableOwner,
            ExtensionType::MemoTransfer => UiExtensionType::MemoTransfer,
            ExtensionType::NonTransferable => UiExtensionType::NonTransferable,
            ExtensionType::InterestBearingConfig => UiExtensionType::InterestBearingConfig,
            ExtensionType::CpiGuard => UiExtensionType::CpiGuard,
            ExtensionType::PermanentDelegate => UiExtensionType::PermanentDelegate,
            ExtensionType::NonTransferableAccount => UiExtensionType::NonTransferableAccount,
            ExtensionType::TransferHook => UiExtensionType::TransferHook,
            ExtensionType::TransferHookAccount => UiExtensionType::TransferHookAccount,
            ExtensionType::ConfidentialTransferFeeConfig => {
                UiExtensionType::ConfidentialTransferFeeConfig
            }
            ExtensionType::ConfidentialTransferFeeAmount => {
                UiExtensionType::ConfidentialTransferFeeAmount
            }
            ExtensionType::MetadataPointer => UiExtensionType::MetadataPointer,
            ExtensionType::TokenMetadata => UiExtensionType::TokenMetadata,
            ExtensionType::GroupPointer => UiExtensionType::GroupPointer,
            ExtensionType::GroupMemberPointer => UiExtensionType::GroupMemberPointer,
            ExtensionType::TokenGroup => UiExtensionType::TokenGroup,
            ExtensionType::TokenGroupMember => UiExtensionType::TokenGroupMember,
        }
    }
}

fn parse_signers(
    map: &mut Map<String, Value>,
    last_nonsigner_index: usize,
    account_keys: &AccountKeys,
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

fn check_num_token_accounts(accounts: &[u8], num: usize) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::SplToken)
}

#[deprecated(since = "1.16.0", note = "Instruction conversions no longer needed")]
pub fn spl_token_instruction(instruction: SplTokenInstruction) -> Instruction {
    Instruction {
        program_id: instruction.program_id,
        accounts: instruction
            .accounts
            .iter()
            .map(|meta| AccountMeta {
                pubkey: meta.pubkey,
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: instruction.data,
    }
}

fn map_coption_pubkey(pubkey: COption<Pubkey>) -> Option<String> {
    match pubkey {
        COption::Some(pubkey) => Some(pubkey.to_string()),
        COption::None => None,
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{message::Message, pubkey::Pubkey},
        spl_token_2022::instruction::*,
        std::iter::repeat_with,
    };

    fn test_parse_token(program_id: &Pubkey) {
        let mint_pubkey = Pubkey::new_unique();
        let mint_authority = Pubkey::new_unique();
        let freeze_authority = Pubkey::new_unique();
        let rent_sysvar = solana_sdk::sysvar::rent::id();

        // Test InitializeMint variations
        let initialize_mint_ix = initialize_mint(
            program_id,
            &mint_pubkey,
            &mint_authority,
            Some(&freeze_authority),
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMint".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "decimals": 2,
                    "mintAuthority": mint_authority.to_string(),
                    "freezeAuthority": freeze_authority.to_string(),
                    "rentSysvar": rent_sysvar.to_string(),
                })
            }
        );

        let initialize_mint_ix =
            initialize_mint(program_id, &mint_pubkey, &mint_authority, None, 2).unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMint".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "decimals": 2,
                    "mintAuthority": mint_authority.to_string(),
                    "rentSysvar": rent_sysvar.to_string(),
                })
            }
        );

        // Test InitializeMint2
        let initialize_mint_ix = initialize_mint2(
            program_id,
            &mint_pubkey,
            &mint_authority,
            Some(&freeze_authority),
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMint2".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "decimals": 2,
                    "mintAuthority": mint_authority.to_string(),
                    "freezeAuthority": freeze_authority.to_string(),
                })
            }
        );

        // Test InitializeAccount
        let account_pubkey = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let initialize_account_ix =
            initialize_account(program_id, &account_pubkey, &mint_pubkey, &owner).unwrap();
        let message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeAccount".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "owner": owner.to_string(),
                    "rentSysvar": rent_sysvar.to_string(),
                })
            }
        );

        // Test InitializeAccount2
        let initialize_account_ix =
            initialize_account2(program_id, &account_pubkey, &mint_pubkey, &owner).unwrap();
        let message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeAccount2".to_string(),
                info: json!({
                   "account": account_pubkey.to_string(),
                   "mint": mint_pubkey.to_string(),
                   "owner": owner.to_string(),
                   "rentSysvar": rent_sysvar.to_string(),
                })
            }
        );

        // Test InitializeAccount3
        let initialize_account_ix =
            initialize_account3(program_id, &account_pubkey, &mint_pubkey, &owner).unwrap();
        let message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeAccount3".to_string(),
                info: json!({
                   "account": account_pubkey.to_string(),
                   "mint": mint_pubkey.to_string(),
                   "owner": owner.to_string(),
                })
            }
        );

        // Test InitializeMultisig
        let multisig_pubkey = Pubkey::new_unique();
        let multisig_signer0 = Pubkey::new_unique();
        let multisig_signer1 = Pubkey::new_unique();
        let multisig_signer2 = Pubkey::new_unique();
        let initialize_multisig_ix = initialize_multisig(
            program_id,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1, &multisig_signer2],
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_multisig_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMultisig".to_string(),
                info: json!({
                    "multisig": multisig_pubkey.to_string(),
                    "m": 2,
                    "rentSysvar": rent_sysvar.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                        multisig_signer2.to_string(),
                    ],
                })
            }
        );

        // Test InitializeMultisig2
        let initialize_multisig_ix = initialize_multisig2(
            program_id,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1, &multisig_signer2],
            2,
        )
        .unwrap();
        let message = Message::new(&[initialize_multisig_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeMultisig2".to_string(),
                info: json!({
                    "multisig": multisig_pubkey.to_string(),
                    "m": 2,
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                        multisig_signer2.to_string(),
                    ],
                })
            }
        );

        // Test Transfer, incl multisig
        let recipient = Pubkey::new_unique();
        #[allow(deprecated)]
        let transfer_ix =
            transfer(program_id, &account_pubkey, &recipient, &owner, &[], 42).unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "authority": owner.to_string(),
                    "amount": "42",
                })
            }
        );

        #[allow(deprecated)]
        let transfer_ix = transfer(
            program_id,
            &account_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            42,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transfer".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "multisigAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "amount": "42",
                })
            }
        );

        // Test Approve, incl multisig
        let approve_ix = approve(program_id, &account_pubkey, &recipient, &owner, &[], 42).unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approve".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "delegate": recipient.to_string(),
                    "owner": owner.to_string(),
                    "amount": "42",
                })
            }
        );

        let approve_ix = approve(
            program_id,
            &account_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            42,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approve".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "delegate": recipient.to_string(),
                    "multisigOwner": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "amount": "42",
                })
            }
        );

        // Test Revoke
        let revoke_ix = revoke(program_id, &account_pubkey, &owner, &[]).unwrap();
        let message = Message::new(&[revoke_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "revoke".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "owner": owner.to_string(),
                })
            }
        );

        // Test SetOwner
        let new_freeze_authority = Pubkey::new_unique();
        let set_authority_ix = set_authority(
            program_id,
            &mint_pubkey,
            Some(&new_freeze_authority),
            AuthorityType::FreezeAccount,
            &freeze_authority,
            &[],
        )
        .unwrap();
        let message = Message::new(&[set_authority_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "newAuthority": new_freeze_authority.to_string(),
                    "authority": freeze_authority.to_string(),
                    "authorityType": "freezeAccount".to_string(),
                })
            }
        );

        let set_authority_ix = set_authority(
            program_id,
            &account_pubkey,
            None,
            AuthorityType::CloseAccount,
            &owner,
            &[],
        )
        .unwrap();
        let message = Message::new(&[set_authority_ix], None);
        let compiled_instruction = &message.instructions[0];
        let new_authority: Option<String> = None;
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setAuthority".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "newAuthority": new_authority,
                    "authority": owner.to_string(),
                    "authorityType": "closeAccount".to_string(),
                })
            }
        );

        // Test MintTo
        let mint_to_ix = mint_to(
            program_id,
            &mint_pubkey,
            &account_pubkey,
            &mint_authority,
            &[],
            42,
        )
        .unwrap();
        let message = Message::new(&[mint_to_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "mintTo".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "account": account_pubkey.to_string(),
                    "mintAuthority": mint_authority.to_string(),
                    "amount": "42",
                })
            }
        );

        // Test Burn
        let burn_ix = burn(program_id, &account_pubkey, &mint_pubkey, &owner, &[], 42).unwrap();
        let message = Message::new(&[burn_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "burn".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "authority": owner.to_string(),
                    "amount": "42",
                })
            }
        );

        // Test CloseAccount
        let close_account_ix =
            close_account(program_id, &account_pubkey, &recipient, &owner, &[]).unwrap();
        let message = Message::new(&[close_account_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "closeAccount".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "owner": owner.to_string(),
                })
            }
        );

        // Test FreezeAccount
        let freeze_account_ix = freeze_account(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &freeze_authority,
            &[],
        )
        .unwrap();
        let message = Message::new(&[freeze_account_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "freezeAccount".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "freezeAuthority": freeze_authority.to_string(),
                })
            }
        );

        // Test ThawAccount
        let thaw_account_ix = thaw_account(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &freeze_authority,
            &[],
        )
        .unwrap();
        let message = Message::new(&[thaw_account_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "thawAccount".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "freezeAuthority": freeze_authority.to_string(),
                })
            }
        );

        // Test TransferChecked, incl multisig
        let transfer_ix = transfer_checked(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &recipient,
            &owner,
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transferChecked".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "authority": owner.to_string(),
                    "tokenAmount": {
                        "uiAmount": 0.42,
                        "decimals": 2,
                        "amount": "42",
                        "uiAmountString": "0.42",
                   }
                })
            }
        );

        let transfer_ix = transfer_checked(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "transferChecked".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "destination": recipient.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "multisigAuthority": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "tokenAmount": {
                        "uiAmount": 0.42,
                        "decimals": 2,
                        "amount": "42",
                        "uiAmountString": "0.42",
                   }
                })
            }
        );

        // Test ApproveChecked, incl multisig
        let approve_ix = approve_checked(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &recipient,
            &owner,
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approveChecked".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "delegate": recipient.to_string(),
                    "owner": owner.to_string(),
                    "tokenAmount": {
                        "uiAmount": 0.42,
                        "decimals": 2,
                        "amount": "42",
                        "uiAmountString": "0.42",
                    }
                })
            }
        );

        let approve_ix = approve_checked(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &recipient,
            &multisig_pubkey,
            &[&multisig_signer0, &multisig_signer1],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[approve_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "approveChecked".to_string(),
                info: json!({
                    "source": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "delegate": recipient.to_string(),
                    "multisigOwner": multisig_pubkey.to_string(),
                    "signers": vec![
                        multisig_signer0.to_string(),
                        multisig_signer1.to_string(),
                    ],
                    "tokenAmount": {
                        "uiAmount": 0.42,
                        "decimals": 2,
                        "amount": "42",
                        "uiAmountString": "0.42",
                    }
                })
            }
        );

        // Test MintToChecked
        let mint_to_ix = mint_to_checked(
            program_id,
            &mint_pubkey,
            &account_pubkey,
            &mint_authority,
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[mint_to_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "mintToChecked".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "account": account_pubkey.to_string(),
                    "mintAuthority": mint_authority.to_string(),
                    "tokenAmount": {
                        "uiAmount": 0.42,
                        "decimals": 2,
                        "amount": "42",
                        "uiAmountString": "0.42",
                    }
                })
            }
        );

        // Test BurnChecked
        let burn_ix = burn_checked(
            program_id,
            &account_pubkey,
            &mint_pubkey,
            &owner,
            &[],
            42,
            2,
        )
        .unwrap();
        let message = Message::new(&[burn_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "burnChecked".to_string(),
                info: json!({
                    "account": account_pubkey.to_string(),
                    "mint": mint_pubkey.to_string(),
                    "authority": owner.to_string(),
                    "tokenAmount": {
                        "uiAmount": 0.42,
                        "decimals": 2,
                        "amount": "42",
                        "uiAmountString": "0.42",
                    }
                })
            }
        );

        // Test SyncNative
        let sync_native_ix = sync_native(program_id, &account_pubkey).unwrap();
        let message = Message::new(&[sync_native_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "syncNative".to_string(),
                info: json!({
                   "account": account_pubkey.to_string(),
                })
            }
        );

        // Test InitializeImmutableOwner
        let init_immutable_owner_ix =
            initialize_immutable_owner(program_id, &account_pubkey).unwrap();
        let message = Message::new(&[init_immutable_owner_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initializeImmutableOwner".to_string(),
                info: json!({
                   "account": account_pubkey.to_string(),
                })
            }
        );

        // Test GetAccountDataSize
        let get_account_data_size_ix = get_account_data_size(
            program_id,
            &mint_pubkey,
            &[], // This emulates the packed data of spl_token::instruction::get_account_data_size
        )
        .unwrap();
        let message = Message::new(&[get_account_data_size_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "getAccountDataSize".to_string(),
                info: json!({
                   "mint": mint_pubkey.to_string(),
                })
            }
        );

        let get_account_data_size_ix = get_account_data_size(
            program_id,
            &mint_pubkey,
            &[ExtensionType::ImmutableOwner, ExtensionType::MemoTransfer],
        )
        .unwrap();
        let message = Message::new(&[get_account_data_size_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "getAccountDataSize".to_string(),
                info: json!({
                    "mint": mint_pubkey.to_string(),
                    "extensionTypes": [
                        "immutableOwner",
                        "memoTransfer"
                    ]
                })
            }
        );

        // Test AmountToUiAmount
        let amount_to_ui_amount_ix = amount_to_ui_amount(program_id, &mint_pubkey, 4242).unwrap();
        let message = Message::new(&[amount_to_ui_amount_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "amountToUiAmount".to_string(),
                info: json!({
                   "mint": mint_pubkey.to_string(),
                   "amount": 4242,
                })
            }
        );

        // Test UiAmountToAmount
        let ui_amount_to_amount_ix =
            ui_amount_to_amount(program_id, &mint_pubkey, "42.42").unwrap();
        let message = Message::new(&[ui_amount_to_amount_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "uiAmountToAmount".to_string(),
                info: json!({
                   "mint": mint_pubkey.to_string(),
                   "uiAmount": "42.42",
                })
            }
        );
    }

    #[test]
    fn test_parse_token_v3() {
        test_parse_token(&spl_token::id());
    }

    #[test]
    fn test_parse_token_2022() {
        test_parse_token(&spl_token_2022::id());
    }

    #[test]
    fn test_create_native_mint() {
        let payer = Pubkey::new_unique();
        let create_native_mint_ix = create_native_mint(&spl_token_2022::id(), &payer).unwrap();
        let message = Message::new(&[create_native_mint_ix], None);
        let compiled_instruction = &message.instructions[0];
        assert_eq!(
            parse_token(
                compiled_instruction,
                &AccountKeys::new(&message.account_keys, None)
            )
            .unwrap(),
            ParsedInstructionEnum {
                instruction_type: "createNativeMint".to_string(),
                info: json!({
                   "payer": payer.to_string(),
                   "nativeMint": spl_token_2022::native_mint::id().to_string(),
                   "systemProgram": solana_sdk::system_program::id().to_string(),
                })
            }
        );
    }

    fn test_token_ix_not_enough_keys(program_id: &Pubkey) {
        let keys: Vec<Pubkey> = repeat_with(solana_sdk::pubkey::new_rand).take(10).collect();

        // Test InitializeMint variations
        let initialize_mint_ix =
            initialize_mint(program_id, &keys[0], &keys[1], Some(&keys[2]), 2).unwrap();
        let mut message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..1], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        let initialize_mint_ix = initialize_mint(program_id, &keys[0], &keys[1], None, 2).unwrap();
        let mut message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..1], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeMint2
        let initialize_mint_ix =
            initialize_mint2(program_id, &keys[0], &keys[1], Some(&keys[2]), 2).unwrap();
        let mut message = Message::new(&[initialize_mint_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..0], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeAccount
        let initialize_account_ix =
            initialize_account(program_id, &keys[0], &keys[1], &keys[2]).unwrap();
        let mut message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..3], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeAccount2
        let initialize_account_ix =
            initialize_account2(program_id, &keys[0], &keys[1], &keys[3]).unwrap();
        let mut message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeAccount3
        let initialize_account_ix =
            initialize_account3(program_id, &keys[0], &keys[1], &keys[2]).unwrap();
        let mut message = Message::new(&[initialize_account_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..1], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeMultisig
        let initialize_multisig_ix =
            initialize_multisig(program_id, &keys[0], &[&keys[1], &keys[2], &keys[3]], 2).unwrap();
        let mut message = Message::new(&[initialize_multisig_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..4], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeMultisig2
        let initialize_multisig_ix =
            initialize_multisig2(program_id, &keys[0], &[&keys[1], &keys[2], &keys[3]], 2).unwrap();
        let mut message = Message::new(&[initialize_multisig_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..3], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test Transfer, incl multisig
        #[allow(deprecated)]
        let transfer_ix = transfer(program_id, &keys[1], &keys[2], &keys[0], &[], 42).unwrap();
        let mut message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        #[allow(deprecated)]
        let transfer_ix = transfer(
            program_id,
            &keys[2],
            &keys[3],
            &keys[4],
            &[&keys[0], &keys[1]],
            42,
        )
        .unwrap();
        let mut message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..4], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test Approve, incl multisig
        let approve_ix = approve(program_id, &keys[1], &keys[2], &keys[0], &[], 42).unwrap();
        let mut message = Message::new(&[approve_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        let approve_ix = approve(
            program_id,
            &keys[2],
            &keys[3],
            &keys[4],
            &[&keys[0], &keys[1]],
            42,
        )
        .unwrap();
        let mut message = Message::new(&[approve_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..4], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test Revoke
        let revoke_ix = revoke(program_id, &keys[1], &keys[0], &[]).unwrap();
        let mut message = Message::new(&[revoke_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..1], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test SetAuthority
        let set_authority_ix = set_authority(
            program_id,
            &keys[1],
            Some(&keys[2]),
            AuthorityType::FreezeAccount,
            &keys[0],
            &[],
        )
        .unwrap();
        let mut message = Message::new(&[set_authority_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..1], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test MintTo
        let mint_to_ix = mint_to(program_id, &keys[1], &keys[2], &keys[0], &[], 42).unwrap();
        let mut message = Message::new(&[mint_to_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test Burn
        let burn_ix = burn(program_id, &keys[1], &keys[2], &keys[0], &[], 42).unwrap();
        let mut message = Message::new(&[burn_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test CloseAccount
        let close_account_ix =
            close_account(program_id, &keys[1], &keys[2], &keys[0], &[]).unwrap();
        let mut message = Message::new(&[close_account_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test FreezeAccount
        let freeze_account_ix =
            freeze_account(program_id, &keys[1], &keys[2], &keys[0], &[]).unwrap();
        let mut message = Message::new(&[freeze_account_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test ThawAccount
        let thaw_account_ix = thaw_account(program_id, &keys[1], &keys[2], &keys[0], &[]).unwrap();
        let mut message = Message::new(&[thaw_account_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test TransferChecked, incl multisig
        let transfer_ix = transfer_checked(
            program_id,
            &keys[1],
            &keys[2],
            &keys[3],
            &keys[0],
            &[],
            42,
            2,
        )
        .unwrap();
        let mut message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..3], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        let transfer_ix = transfer_checked(
            program_id,
            &keys[2],
            &keys[3],
            &keys[4],
            &keys[5],
            &[&keys[0], &keys[1]],
            42,
            2,
        )
        .unwrap();
        let mut message = Message::new(&[transfer_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..5], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test ApproveChecked, incl multisig
        let approve_ix = approve_checked(
            program_id,
            &keys[1],
            &keys[2],
            &keys[3],
            &keys[0],
            &[],
            42,
            2,
        )
        .unwrap();
        let mut message = Message::new(&[approve_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..3], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        let approve_ix = approve_checked(
            program_id,
            &keys[2],
            &keys[3],
            &keys[4],
            &keys[5],
            &[&keys[0], &keys[1]],
            42,
            2,
        )
        .unwrap();
        let mut message = Message::new(&[approve_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..5], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 3].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test MintToChecked
        let mint_to_ix =
            mint_to_checked(program_id, &keys[1], &keys[2], &keys[0], &[], 42, 2).unwrap();
        let mut message = Message::new(&[mint_to_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test BurnChecked
        let burn_ix = burn_checked(program_id, &keys[1], &keys[2], &keys[0], &[], 42, 2).unwrap();
        let mut message = Message::new(&[burn_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys[0..2], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test SyncNative
        let sync_native_ix = sync_native(program_id, &keys[0]).unwrap();
        let mut message = Message::new(&[sync_native_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&[], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test InitializeImmutableOwner
        let init_immutable_owner_ix = initialize_immutable_owner(program_id, &keys[0]).unwrap();
        let mut message = Message::new(&[init_immutable_owner_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&[], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test GetAccountDataSize
        let get_account_data_size_ix = get_account_data_size(program_id, &keys[0], &[]).unwrap();
        let mut message = Message::new(&[get_account_data_size_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&[], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test AmountToUiAmount
        let amount_to_ui_amount_ix = amount_to_ui_amount(program_id, &keys[0], 4242).unwrap();
        let mut message = Message::new(&[amount_to_ui_amount_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&[], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());

        // Test UiAmountToAmount
        let ui_amount_to_amount_ix = ui_amount_to_amount(program_id, &keys[0], "42.42").unwrap();
        let mut message = Message::new(&[ui_amount_to_amount_ix], None);
        let compiled_instruction = &mut message.instructions[0];
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&[], None)).is_err());
        compiled_instruction.accounts =
            compiled_instruction.accounts[0..compiled_instruction.accounts.len() - 1].to_vec();
        assert!(parse_token(compiled_instruction, &AccountKeys::new(&keys, None)).is_err());
    }

    #[test]
    fn test_not_enough_keys_token_v3() {
        test_token_ix_not_enough_keys(&spl_token::id());
    }

    #[test]
    fn test_not_enough_keys_token_2022() {
        test_token_ix_not_enough_keys(&spl_token_2022::id());
    }
}
