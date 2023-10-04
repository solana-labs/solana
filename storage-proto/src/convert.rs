use {
    crate::{StoredExtendedRewards, StoredTransactionStatusMeta},
    solana_account_decoder::parse_token::{real_number_string_trimmed, UiTokenAmount},
    solana_sdk::{
        hash::Hash,
        instruction::{CompiledInstruction, InstructionError},
        message::{
            legacy::Message as LegacyMessage,
            v0::{self, LoadedAddresses, MessageAddressTableLookup},
            MessageHeader, VersionedMessage,
        },
        pubkey::Pubkey,
        signature::Signature,
        transaction::{Transaction, TransactionError, VersionedTransaction},
        transaction_context::TransactionReturnData,
    },
    solana_transaction_status::{
        ConfirmedBlock, InnerInstruction, InnerInstructions, Reward, RewardType,
        TransactionByAddrInfo, TransactionStatusMeta, TransactionTokenBalance,
        TransactionWithStatusMeta, VersionedConfirmedBlock, VersionedTransactionWithStatusMeta,
    },
    std::{
        convert::{TryFrom, TryInto},
        str::FromStr,
    },
};

#[allow(clippy::derive_partial_eq_without_eq)]
pub mod generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/solana.storage.confirmed_block.rs"
    ));
}

#[allow(clippy::derive_partial_eq_without_eq)]
pub mod tx_by_addr {
    include!(concat!(
        env!("OUT_DIR"),
        "/solana.storage.transaction_by_addr.rs"
    ));
}

impl From<Vec<Reward>> for generated::Rewards {
    fn from(rewards: Vec<Reward>) -> Self {
        Self {
            rewards: rewards.into_iter().map(|r| r.into()).collect(),
        }
    }
}

impl From<generated::Rewards> for Vec<Reward> {
    fn from(rewards: generated::Rewards) -> Self {
        rewards.rewards.into_iter().map(|r| r.into()).collect()
    }
}

impl From<StoredExtendedRewards> for generated::Rewards {
    fn from(rewards: StoredExtendedRewards) -> Self {
        Self {
            rewards: rewards
                .into_iter()
                .map(|r| {
                    let r: Reward = r.into();
                    r.into()
                })
                .collect(),
        }
    }
}

impl From<generated::Rewards> for StoredExtendedRewards {
    fn from(rewards: generated::Rewards) -> Self {
        rewards
            .rewards
            .into_iter()
            .map(|r| {
                let r: Reward = r.into();
                r.into()
            })
            .collect()
    }
}

impl From<Reward> for generated::Reward {
    fn from(reward: Reward) -> Self {
        Self {
            pubkey: reward.pubkey,
            lamports: reward.lamports,
            post_balance: reward.post_balance,
            reward_type: match reward.reward_type {
                None => generated::RewardType::Unspecified,
                Some(RewardType::Fee) => generated::RewardType::Fee,
                Some(RewardType::Rent) => generated::RewardType::Rent,
                Some(RewardType::Staking) => generated::RewardType::Staking,
                Some(RewardType::Voting) => generated::RewardType::Voting,
            } as i32,
            commission: reward.commission.map(|c| c.to_string()).unwrap_or_default(),
        }
    }
}

impl From<generated::Reward> for Reward {
    fn from(reward: generated::Reward) -> Self {
        Self {
            pubkey: reward.pubkey,
            lamports: reward.lamports,
            post_balance: reward.post_balance,
            reward_type: match reward.reward_type {
                0 => None,
                1 => Some(RewardType::Fee),
                2 => Some(RewardType::Rent),
                3 => Some(RewardType::Staking),
                4 => Some(RewardType::Voting),
                _ => None,
            },
            commission: reward.commission.parse::<u8>().ok(),
        }
    }
}

impl From<VersionedConfirmedBlock> for generated::ConfirmedBlock {
    fn from(confirmed_block: VersionedConfirmedBlock) -> Self {
        let VersionedConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
            block_height,
        } = confirmed_block;

        Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions.into_iter().map(|tx| tx.into()).collect(),
            rewards: rewards.into_iter().map(|r| r.into()).collect(),
            block_time: block_time.map(|timestamp| generated::UnixTimestamp { timestamp }),
            block_height: block_height.map(|block_height| generated::BlockHeight { block_height }),
        }
    }
}

impl TryFrom<generated::ConfirmedBlock> for ConfirmedBlock {
    type Error = bincode::Error;
    fn try_from(
        confirmed_block: generated::ConfirmedBlock,
    ) -> std::result::Result<Self, Self::Error> {
        let generated::ConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
            block_height,
        } = confirmed_block;

        Ok(Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions
                .into_iter()
                .map(|tx| tx.try_into())
                .collect::<std::result::Result<Vec<_>, Self::Error>>()?,
            rewards: rewards.into_iter().map(|r| r.into()).collect(),
            block_time: block_time.map(|generated::UnixTimestamp { timestamp }| timestamp),
            block_height: block_height.map(|generated::BlockHeight { block_height }| block_height),
        })
    }
}

impl From<TransactionWithStatusMeta> for generated::ConfirmedTransaction {
    fn from(tx_with_meta: TransactionWithStatusMeta) -> Self {
        match tx_with_meta {
            TransactionWithStatusMeta::MissingMetadata(transaction) => Self {
                transaction: Some(generated::Transaction::from(transaction)),
                meta: None,
            },
            TransactionWithStatusMeta::Complete(tx_with_meta) => Self::from(tx_with_meta),
        }
    }
}

impl From<VersionedTransactionWithStatusMeta> for generated::ConfirmedTransaction {
    fn from(value: VersionedTransactionWithStatusMeta) -> Self {
        Self {
            transaction: Some(value.transaction.into()),
            meta: Some(value.meta.into()),
        }
    }
}

impl TryFrom<generated::ConfirmedTransaction> for TransactionWithStatusMeta {
    type Error = bincode::Error;
    fn try_from(value: generated::ConfirmedTransaction) -> std::result::Result<Self, Self::Error> {
        let meta = value.meta.map(|meta| meta.try_into()).transpose()?;
        let transaction = value.transaction.expect("transaction is required").into();
        Ok(match meta {
            Some(meta) => Self::Complete(VersionedTransactionWithStatusMeta { transaction, meta }),
            None => Self::MissingMetadata(
                transaction
                    .into_legacy_transaction()
                    .expect("meta is required for versioned transactions"),
            ),
        })
    }
}

impl From<Transaction> for generated::Transaction {
    fn from(value: Transaction) -> Self {
        Self {
            signatures: value
                .signatures
                .into_iter()
                .map(|signature| <Signature as AsRef<[u8]>>::as_ref(&signature).into())
                .collect(),
            message: Some(value.message.into()),
        }
    }
}

impl From<VersionedTransaction> for generated::Transaction {
    fn from(value: VersionedTransaction) -> Self {
        Self {
            signatures: value
                .signatures
                .into_iter()
                .map(|signature| <Signature as AsRef<[u8]>>::as_ref(&signature).into())
                .collect(),
            message: Some(value.message.into()),
        }
    }
}

impl From<generated::Transaction> for VersionedTransaction {
    fn from(value: generated::Transaction) -> Self {
        Self {
            signatures: value
                .signatures
                .into_iter()
                .map(Signature::try_from)
                .collect::<Result<_, _>>()
                .unwrap(),
            message: value.message.expect("message is required").into(),
        }
    }
}

impl From<LegacyMessage> for generated::Message {
    fn from(message: LegacyMessage) -> Self {
        Self {
            header: Some(message.header.into()),
            account_keys: message
                .account_keys
                .iter()
                .map(|key| <Pubkey as AsRef<[u8]>>::as_ref(key).into())
                .collect(),
            recent_blockhash: message.recent_blockhash.to_bytes().into(),
            instructions: message
                .instructions
                .into_iter()
                .map(|ix| ix.into())
                .collect(),
            versioned: false,
            address_table_lookups: vec![],
        }
    }
}

impl From<VersionedMessage> for generated::Message {
    fn from(message: VersionedMessage) -> Self {
        match message {
            VersionedMessage::Legacy(message) => Self::from(message),
            VersionedMessage::V0(message) => Self {
                header: Some(message.header.into()),
                account_keys: message
                    .account_keys
                    .iter()
                    .map(|key| <Pubkey as AsRef<[u8]>>::as_ref(key).into())
                    .collect(),
                recent_blockhash: message.recent_blockhash.to_bytes().into(),
                instructions: message
                    .instructions
                    .into_iter()
                    .map(|ix| ix.into())
                    .collect(),
                versioned: true,
                address_table_lookups: message
                    .address_table_lookups
                    .into_iter()
                    .map(|lookup| lookup.into())
                    .collect(),
            },
        }
    }
}

impl From<generated::Message> for VersionedMessage {
    fn from(value: generated::Message) -> Self {
        let header = value.header.expect("header is required").into();
        let account_keys = value
            .account_keys
            .into_iter()
            .map(|key| Pubkey::try_from(key).unwrap())
            .collect();
        let recent_blockhash = Hash::new(&value.recent_blockhash);
        let instructions = value.instructions.into_iter().map(|ix| ix.into()).collect();
        let address_table_lookups = value
            .address_table_lookups
            .into_iter()
            .map(|lookup| lookup.into())
            .collect();

        if !value.versioned {
            Self::Legacy(LegacyMessage {
                header,
                account_keys,
                recent_blockhash,
                instructions,
            })
        } else {
            Self::V0(v0::Message {
                header,
                account_keys,
                recent_blockhash,
                instructions,
                address_table_lookups,
            })
        }
    }
}

impl From<MessageHeader> for generated::MessageHeader {
    fn from(value: MessageHeader) -> Self {
        Self {
            num_required_signatures: value.num_required_signatures as u32,
            num_readonly_signed_accounts: value.num_readonly_signed_accounts as u32,
            num_readonly_unsigned_accounts: value.num_readonly_unsigned_accounts as u32,
        }
    }
}

impl From<generated::MessageHeader> for MessageHeader {
    fn from(value: generated::MessageHeader) -> Self {
        Self {
            num_required_signatures: value.num_required_signatures as u8,
            num_readonly_signed_accounts: value.num_readonly_signed_accounts as u8,
            num_readonly_unsigned_accounts: value.num_readonly_unsigned_accounts as u8,
        }
    }
}

impl From<TransactionStatusMeta> for generated::TransactionStatusMeta {
    fn from(value: TransactionStatusMeta) -> Self {
        let TransactionStatusMeta {
            status,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            log_messages,
            pre_token_balances,
            post_token_balances,
            rewards,
            loaded_addresses,
            return_data,
            compute_units_consumed,
        } = value;
        let err = match status {
            Ok(()) => None,
            Err(err) => Some(generated::TransactionError {
                err: bincode::serialize(&err).expect("transaction error to serialize to bytes"),
            }),
        };
        let inner_instructions_none = inner_instructions.is_none();
        let inner_instructions = inner_instructions
            .unwrap_or_default()
            .into_iter()
            .map(|ii| ii.into())
            .collect();
        let log_messages_none = log_messages.is_none();
        let log_messages = log_messages.unwrap_or_default();
        let pre_token_balances = pre_token_balances
            .unwrap_or_default()
            .into_iter()
            .map(|balance| balance.into())
            .collect();
        let post_token_balances = post_token_balances
            .unwrap_or_default()
            .into_iter()
            .map(|balance| balance.into())
            .collect();
        let rewards = rewards
            .unwrap_or_default()
            .into_iter()
            .map(|reward| reward.into())
            .collect();
        let loaded_writable_addresses = loaded_addresses
            .writable
            .into_iter()
            .map(|key| <Pubkey as AsRef<[u8]>>::as_ref(&key).into())
            .collect();
        let loaded_readonly_addresses = loaded_addresses
            .readonly
            .into_iter()
            .map(|key| <Pubkey as AsRef<[u8]>>::as_ref(&key).into())
            .collect();
        let return_data_none = return_data.is_none();
        let return_data = return_data.map(|return_data| return_data.into());

        Self {
            err,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            inner_instructions_none,
            log_messages,
            log_messages_none,
            pre_token_balances,
            post_token_balances,
            rewards,
            loaded_writable_addresses,
            loaded_readonly_addresses,
            return_data,
            return_data_none,
            compute_units_consumed,
        }
    }
}

impl From<StoredTransactionStatusMeta> for generated::TransactionStatusMeta {
    fn from(meta: StoredTransactionStatusMeta) -> Self {
        let meta: TransactionStatusMeta = meta.into();
        meta.into()
    }
}

impl TryFrom<generated::TransactionStatusMeta> for TransactionStatusMeta {
    type Error = bincode::Error;

    fn try_from(value: generated::TransactionStatusMeta) -> std::result::Result<Self, Self::Error> {
        let generated::TransactionStatusMeta {
            err,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            inner_instructions_none,
            log_messages,
            log_messages_none,
            pre_token_balances,
            post_token_balances,
            rewards,
            loaded_writable_addresses,
            loaded_readonly_addresses,
            return_data,
            return_data_none,
            compute_units_consumed,
        } = value;
        let status = match &err {
            None => Ok(()),
            Some(tx_error) => Err(bincode::deserialize(&tx_error.err)?),
        };
        let inner_instructions = if inner_instructions_none {
            None
        } else {
            Some(
                inner_instructions
                    .into_iter()
                    .map(|inner| inner.into())
                    .collect(),
            )
        };
        let log_messages = if log_messages_none {
            None
        } else {
            Some(log_messages)
        };
        let pre_token_balances = Some(
            pre_token_balances
                .into_iter()
                .map(|balance| balance.into())
                .collect(),
        );
        let post_token_balances = Some(
            post_token_balances
                .into_iter()
                .map(|balance| balance.into())
                .collect(),
        );
        let rewards = Some(rewards.into_iter().map(|reward| reward.into()).collect());
        let loaded_addresses = LoadedAddresses {
            writable: loaded_writable_addresses
                .into_iter()
                .map(Pubkey::try_from)
                .collect::<Result<_, _>>()
                .map_err(|err| {
                    let err = format!("Invalid writable address: {err:?}");
                    Self::Error::new(bincode::ErrorKind::Custom(err))
                })?,
            readonly: loaded_readonly_addresses
                .into_iter()
                .map(Pubkey::try_from)
                .collect::<Result<_, _>>()
                .map_err(|err| {
                    let err = format!("Invalid readonly address: {err:?}");
                    Self::Error::new(bincode::ErrorKind::Custom(err))
                })?,
        };
        let return_data = if return_data_none {
            None
        } else {
            return_data.map(|return_data| return_data.into())
        };
        Ok(Self {
            status,
            fee,
            pre_balances,
            post_balances,
            inner_instructions,
            log_messages,
            pre_token_balances,
            post_token_balances,
            rewards,
            loaded_addresses,
            return_data,
            compute_units_consumed,
        })
    }
}

impl From<InnerInstructions> for generated::InnerInstructions {
    fn from(value: InnerInstructions) -> Self {
        Self {
            index: value.index as u32,
            instructions: value.instructions.into_iter().map(|i| i.into()).collect(),
        }
    }
}

impl From<generated::InnerInstructions> for InnerInstructions {
    fn from(value: generated::InnerInstructions) -> Self {
        Self {
            index: value.index as u8,
            instructions: value.instructions.into_iter().map(|i| i.into()).collect(),
        }
    }
}

impl From<TransactionTokenBalance> for generated::TokenBalance {
    fn from(value: TransactionTokenBalance) -> Self {
        Self {
            account_index: value.account_index as u32,
            mint: value.mint,
            ui_token_amount: Some(generated::UiTokenAmount {
                ui_amount: value.ui_token_amount.ui_amount.unwrap_or_default(),
                decimals: value.ui_token_amount.decimals as u32,
                amount: value.ui_token_amount.amount,
                ui_amount_string: value.ui_token_amount.ui_amount_string,
            }),
            owner: value.owner,
            program_id: value.program_id,
        }
    }
}

impl From<generated::TokenBalance> for TransactionTokenBalance {
    fn from(value: generated::TokenBalance) -> Self {
        let ui_token_amount = value.ui_token_amount.unwrap_or_default();
        Self {
            account_index: value.account_index as u8,
            mint: value.mint,
            ui_token_amount: UiTokenAmount {
                ui_amount: if (ui_token_amount.ui_amount - f64::default()).abs() > f64::EPSILON {
                    Some(ui_token_amount.ui_amount)
                } else {
                    None
                },
                decimals: ui_token_amount.decimals as u8,
                amount: ui_token_amount.amount.clone(),
                ui_amount_string: if !ui_token_amount.ui_amount_string.is_empty() {
                    ui_token_amount.ui_amount_string
                } else {
                    real_number_string_trimmed(
                        u64::from_str(&ui_token_amount.amount).unwrap_or_default(),
                        ui_token_amount.decimals as u8,
                    )
                },
            },
            owner: value.owner,
            program_id: value.program_id,
        }
    }
}

impl From<MessageAddressTableLookup> for generated::MessageAddressTableLookup {
    fn from(lookup: MessageAddressTableLookup) -> Self {
        Self {
            account_key: <Pubkey as AsRef<[u8]>>::as_ref(&lookup.account_key).into(),
            writable_indexes: lookup.writable_indexes,
            readonly_indexes: lookup.readonly_indexes,
        }
    }
}

impl From<generated::MessageAddressTableLookup> for MessageAddressTableLookup {
    fn from(value: generated::MessageAddressTableLookup) -> Self {
        Self {
            account_key: Pubkey::try_from(value.account_key).unwrap(),
            writable_indexes: value.writable_indexes,
            readonly_indexes: value.readonly_indexes,
        }
    }
}

impl From<TransactionReturnData> for generated::ReturnData {
    fn from(value: TransactionReturnData) -> Self {
        Self {
            program_id: <Pubkey as AsRef<[u8]>>::as_ref(&value.program_id).into(),
            data: value.data,
        }
    }
}

impl From<generated::ReturnData> for TransactionReturnData {
    fn from(value: generated::ReturnData) -> Self {
        Self {
            program_id: Pubkey::try_from(value.program_id).unwrap(),
            data: value.data,
        }
    }
}

impl From<CompiledInstruction> for generated::CompiledInstruction {
    fn from(value: CompiledInstruction) -> Self {
        Self {
            program_id_index: value.program_id_index as u32,
            accounts: value.accounts,
            data: value.data,
        }
    }
}

impl From<generated::CompiledInstruction> for CompiledInstruction {
    fn from(value: generated::CompiledInstruction) -> Self {
        Self {
            program_id_index: value.program_id_index as u8,
            accounts: value.accounts,
            data: value.data,
        }
    }
}

impl From<InnerInstruction> for generated::InnerInstruction {
    fn from(value: InnerInstruction) -> Self {
        Self {
            program_id_index: value.instruction.program_id_index as u32,
            accounts: value.instruction.accounts,
            data: value.instruction.data,
            stack_height: value.stack_height,
        }
    }
}

impl From<generated::InnerInstruction> for InnerInstruction {
    fn from(value: generated::InnerInstruction) -> Self {
        Self {
            instruction: CompiledInstruction {
                program_id_index: value.program_id_index as u8,
                accounts: value.accounts,
                data: value.data,
            },
            stack_height: value.stack_height,
        }
    }
}

impl TryFrom<tx_by_addr::TransactionError> for TransactionError {
    type Error = &'static str;

    fn try_from(transaction_error: tx_by_addr::TransactionError) -> Result<Self, Self::Error> {
        if transaction_error.transaction_error == 8 {
            if let Some(instruction_error) = transaction_error.instruction_error {
                if let Some(custom) = instruction_error.custom {
                    return Ok(TransactionError::InstructionError(
                        instruction_error.index as u8,
                        InstructionError::Custom(custom.custom),
                    ));
                }

                let ie = match instruction_error.error {
                    0 => InstructionError::GenericError,
                    1 => InstructionError::InvalidArgument,
                    2 => InstructionError::InvalidInstructionData,
                    3 => InstructionError::InvalidAccountData,
                    4 => InstructionError::AccountDataTooSmall,
                    5 => InstructionError::InsufficientFunds,
                    6 => InstructionError::IncorrectProgramId,
                    7 => InstructionError::MissingRequiredSignature,
                    8 => InstructionError::AccountAlreadyInitialized,
                    9 => InstructionError::UninitializedAccount,
                    10 => InstructionError::UnbalancedInstruction,
                    11 => InstructionError::ModifiedProgramId,
                    12 => InstructionError::ExternalAccountLamportSpend,
                    13 => InstructionError::ExternalAccountDataModified,
                    14 => InstructionError::ReadonlyLamportChange,
                    15 => InstructionError::ReadonlyDataModified,
                    16 => InstructionError::DuplicateAccountIndex,
                    17 => InstructionError::ExecutableModified,
                    18 => InstructionError::RentEpochModified,
                    19 => InstructionError::NotEnoughAccountKeys,
                    20 => InstructionError::AccountDataSizeChanged,
                    21 => InstructionError::AccountNotExecutable,
                    22 => InstructionError::AccountBorrowFailed,
                    23 => InstructionError::AccountBorrowOutstanding,
                    24 => InstructionError::DuplicateAccountOutOfSync,
                    26 => InstructionError::InvalidError,
                    27 => InstructionError::ExecutableDataModified,
                    28 => InstructionError::ExecutableLamportChange,
                    29 => InstructionError::ExecutableAccountNotRentExempt,
                    30 => InstructionError::UnsupportedProgramId,
                    31 => InstructionError::CallDepth,
                    32 => InstructionError::MissingAccount,
                    33 => InstructionError::ReentrancyNotAllowed,
                    34 => InstructionError::MaxSeedLengthExceeded,
                    35 => InstructionError::InvalidSeeds,
                    36 => InstructionError::InvalidRealloc,
                    37 => InstructionError::ComputationalBudgetExceeded,
                    38 => InstructionError::PrivilegeEscalation,
                    39 => InstructionError::ProgramEnvironmentSetupFailure,
                    40 => InstructionError::ProgramFailedToComplete,
                    41 => InstructionError::ProgramFailedToCompile,
                    42 => InstructionError::Immutable,
                    43 => InstructionError::IncorrectAuthority,
                    44 => InstructionError::BorshIoError(String::new()),
                    45 => InstructionError::AccountNotRentExempt,
                    46 => InstructionError::InvalidAccountOwner,
                    47 => InstructionError::ArithmeticOverflow,
                    48 => InstructionError::UnsupportedSysvar,
                    49 => InstructionError::IllegalOwner,
                    50 => InstructionError::MaxAccountsDataAllocationsExceeded,
                    51 => InstructionError::MaxAccountsExceeded,
                    52 => InstructionError::MaxInstructionTraceLengthExceeded,
                    53 => InstructionError::BuiltinProgramsMustConsumeComputeUnits,
                    _ => return Err("Invalid InstructionError"),
                };

                return Ok(TransactionError::InstructionError(
                    instruction_error.index as u8,
                    ie,
                ));
            }
        }

        if let Some(transaction_details) = transaction_error.transaction_details {
            match transaction_error.transaction_error {
                30 => {
                    return Ok(TransactionError::DuplicateInstruction(
                        transaction_details.index as u8,
                    ));
                }
                31 => {
                    return Ok(TransactionError::InsufficientFundsForRent {
                        account_index: transaction_details.index as u8,
                    });
                }

                35 => {
                    return Ok(TransactionError::ProgramExecutionTemporarilyRestricted {
                        account_index: transaction_details.index as u8,
                    });
                }
                _ => {}
            }
        }

        Ok(match transaction_error.transaction_error {
            0 => TransactionError::AccountInUse,
            1 => TransactionError::AccountLoadedTwice,
            2 => TransactionError::AccountNotFound,
            3 => TransactionError::ProgramAccountNotFound,
            4 => TransactionError::InsufficientFundsForFee,
            5 => TransactionError::InvalidAccountForFee,
            6 => TransactionError::AlreadyProcessed,
            7 => TransactionError::BlockhashNotFound,
            9 => TransactionError::CallChainTooDeep,
            10 => TransactionError::MissingSignatureForFee,
            11 => TransactionError::InvalidAccountIndex,
            12 => TransactionError::SignatureFailure,
            13 => TransactionError::InvalidProgramForExecution,
            14 => TransactionError::SanitizeFailure,
            15 => TransactionError::ClusterMaintenance,
            16 => TransactionError::AccountBorrowOutstanding,
            17 => TransactionError::WouldExceedMaxBlockCostLimit,
            18 => TransactionError::UnsupportedVersion,
            19 => TransactionError::InvalidWritableAccount,
            20 => TransactionError::WouldExceedMaxAccountCostLimit,
            21 => TransactionError::WouldExceedAccountDataBlockLimit,
            22 => TransactionError::TooManyAccountLocks,
            23 => TransactionError::AddressLookupTableNotFound,
            24 => TransactionError::InvalidAddressLookupTableOwner,
            25 => TransactionError::InvalidAddressLookupTableData,
            26 => TransactionError::InvalidAddressLookupTableIndex,
            27 => TransactionError::InvalidRentPayingAccount,
            28 => TransactionError::WouldExceedMaxVoteCostLimit,
            29 => TransactionError::WouldExceedAccountDataTotalLimit,
            32 => TransactionError::MaxLoadedAccountsDataSizeExceeded,
            33 => TransactionError::InvalidLoadedAccountsDataSizeLimit,
            34 => TransactionError::ResanitizationNeeded,
            36 => TransactionError::UnbalancedTransaction,
            _ => return Err("Invalid TransactionError"),
        })
    }
}

impl From<TransactionError> for tx_by_addr::TransactionError {
    fn from(transaction_error: TransactionError) -> Self {
        Self {
            transaction_error: match transaction_error {
                TransactionError::AccountInUse => tx_by_addr::TransactionErrorType::AccountInUse,
                TransactionError::AccountLoadedTwice => {
                    tx_by_addr::TransactionErrorType::AccountLoadedTwice
                }
                TransactionError::AccountNotFound => {
                    tx_by_addr::TransactionErrorType::AccountNotFound
                }
                TransactionError::ProgramAccountNotFound => {
                    tx_by_addr::TransactionErrorType::ProgramAccountNotFound
                }
                TransactionError::InsufficientFundsForFee => {
                    tx_by_addr::TransactionErrorType::InsufficientFundsForFee
                }
                TransactionError::InvalidAccountForFee => {
                    tx_by_addr::TransactionErrorType::InvalidAccountForFee
                }
                TransactionError::AlreadyProcessed => {
                    tx_by_addr::TransactionErrorType::AlreadyProcessed
                }
                TransactionError::BlockhashNotFound => {
                    tx_by_addr::TransactionErrorType::BlockhashNotFound
                }
                TransactionError::CallChainTooDeep => {
                    tx_by_addr::TransactionErrorType::CallChainTooDeep
                }
                TransactionError::MissingSignatureForFee => {
                    tx_by_addr::TransactionErrorType::MissingSignatureForFee
                }
                TransactionError::InvalidAccountIndex => {
                    tx_by_addr::TransactionErrorType::InvalidAccountIndex
                }
                TransactionError::SignatureFailure => {
                    tx_by_addr::TransactionErrorType::SignatureFailure
                }
                TransactionError::InvalidProgramForExecution => {
                    tx_by_addr::TransactionErrorType::InvalidProgramForExecution
                }
                TransactionError::SanitizeFailure => {
                    tx_by_addr::TransactionErrorType::SanitizeFailure
                }
                TransactionError::ClusterMaintenance => {
                    tx_by_addr::TransactionErrorType::ClusterMaintenance
                }
                TransactionError::InstructionError(_, _) => {
                    tx_by_addr::TransactionErrorType::InstructionError
                }
                TransactionError::AccountBorrowOutstanding => {
                    tx_by_addr::TransactionErrorType::AccountBorrowOutstandingTx
                }
                TransactionError::WouldExceedMaxBlockCostLimit => {
                    tx_by_addr::TransactionErrorType::WouldExceedMaxBlockCostLimit
                }
                TransactionError::UnsupportedVersion => {
                    tx_by_addr::TransactionErrorType::UnsupportedVersion
                }
                TransactionError::InvalidWritableAccount => {
                    tx_by_addr::TransactionErrorType::InvalidWritableAccount
                }
                TransactionError::WouldExceedMaxAccountCostLimit => {
                    tx_by_addr::TransactionErrorType::WouldExceedMaxAccountCostLimit
                }
                TransactionError::WouldExceedAccountDataBlockLimit => {
                    tx_by_addr::TransactionErrorType::WouldExceedAccountDataBlockLimit
                }
                TransactionError::TooManyAccountLocks => {
                    tx_by_addr::TransactionErrorType::TooManyAccountLocks
                }
                TransactionError::AddressLookupTableNotFound => {
                    tx_by_addr::TransactionErrorType::AddressLookupTableNotFound
                }
                TransactionError::InvalidAddressLookupTableOwner => {
                    tx_by_addr::TransactionErrorType::InvalidAddressLookupTableOwner
                }
                TransactionError::InvalidAddressLookupTableData => {
                    tx_by_addr::TransactionErrorType::InvalidAddressLookupTableData
                }
                TransactionError::InvalidAddressLookupTableIndex => {
                    tx_by_addr::TransactionErrorType::InvalidAddressLookupTableIndex
                }
                TransactionError::InvalidRentPayingAccount => {
                    tx_by_addr::TransactionErrorType::InvalidRentPayingAccount
                }
                TransactionError::WouldExceedMaxVoteCostLimit => {
                    tx_by_addr::TransactionErrorType::WouldExceedMaxVoteCostLimit
                }
                TransactionError::WouldExceedAccountDataTotalLimit => {
                    tx_by_addr::TransactionErrorType::WouldExceedAccountDataTotalLimit
                }
                TransactionError::DuplicateInstruction(_) => {
                    tx_by_addr::TransactionErrorType::DuplicateInstruction
                }
                TransactionError::InsufficientFundsForRent { .. } => {
                    tx_by_addr::TransactionErrorType::InsufficientFundsForRent
                }
                TransactionError::MaxLoadedAccountsDataSizeExceeded => {
                    tx_by_addr::TransactionErrorType::MaxLoadedAccountsDataSizeExceeded
                }
                TransactionError::InvalidLoadedAccountsDataSizeLimit => {
                    tx_by_addr::TransactionErrorType::InvalidLoadedAccountsDataSizeLimit
                }
                TransactionError::ResanitizationNeeded => {
                    tx_by_addr::TransactionErrorType::ResanitizationNeeded
                }
                TransactionError::ProgramExecutionTemporarilyRestricted { .. } => {
                    tx_by_addr::TransactionErrorType::ProgramExecutionTemporarilyRestricted
                }
                TransactionError::UnbalancedTransaction => {
                    tx_by_addr::TransactionErrorType::UnbalancedTransaction
                }
            } as i32,
            instruction_error: match transaction_error {
                TransactionError::InstructionError(index, ref instruction_error) => {
                    Some(tx_by_addr::InstructionError {
                        index: index as u32,
                        error: match instruction_error {
                            InstructionError::GenericError => {
                                tx_by_addr::InstructionErrorType::GenericError
                            }
                            InstructionError::InvalidArgument => {
                                tx_by_addr::InstructionErrorType::InvalidArgument
                            }
                            InstructionError::InvalidInstructionData => {
                                tx_by_addr::InstructionErrorType::InvalidInstructionData
                            }
                            InstructionError::InvalidAccountData => {
                                tx_by_addr::InstructionErrorType::InvalidAccountData
                            }
                            InstructionError::AccountDataTooSmall => {
                                tx_by_addr::InstructionErrorType::AccountDataTooSmall
                            }
                            InstructionError::InsufficientFunds => {
                                tx_by_addr::InstructionErrorType::InsufficientFunds
                            }
                            InstructionError::IncorrectProgramId => {
                                tx_by_addr::InstructionErrorType::IncorrectProgramId
                            }
                            InstructionError::MissingRequiredSignature => {
                                tx_by_addr::InstructionErrorType::MissingRequiredSignature
                            }
                            InstructionError::AccountAlreadyInitialized => {
                                tx_by_addr::InstructionErrorType::AccountAlreadyInitialized
                            }
                            InstructionError::UninitializedAccount => {
                                tx_by_addr::InstructionErrorType::UninitializedAccount
                            }
                            InstructionError::UnbalancedInstruction => {
                                tx_by_addr::InstructionErrorType::UnbalancedInstruction
                            }
                            InstructionError::ModifiedProgramId => {
                                tx_by_addr::InstructionErrorType::ModifiedProgramId
                            }
                            InstructionError::ExternalAccountLamportSpend => {
                                tx_by_addr::InstructionErrorType::ExternalAccountLamportSpend
                            }
                            InstructionError::ExternalAccountDataModified => {
                                tx_by_addr::InstructionErrorType::ExternalAccountDataModified
                            }
                            InstructionError::ReadonlyLamportChange => {
                                tx_by_addr::InstructionErrorType::ReadonlyLamportChange
                            }
                            InstructionError::ReadonlyDataModified => {
                                tx_by_addr::InstructionErrorType::ReadonlyDataModified
                            }
                            InstructionError::DuplicateAccountIndex => {
                                tx_by_addr::InstructionErrorType::DuplicateAccountIndex
                            }
                            InstructionError::ExecutableModified => {
                                tx_by_addr::InstructionErrorType::ExecutableModified
                            }
                            InstructionError::RentEpochModified => {
                                tx_by_addr::InstructionErrorType::RentEpochModified
                            }
                            InstructionError::NotEnoughAccountKeys => {
                                tx_by_addr::InstructionErrorType::NotEnoughAccountKeys
                            }
                            InstructionError::AccountDataSizeChanged => {
                                tx_by_addr::InstructionErrorType::AccountDataSizeChanged
                            }
                            InstructionError::AccountNotExecutable => {
                                tx_by_addr::InstructionErrorType::AccountNotExecutable
                            }
                            InstructionError::AccountBorrowFailed => {
                                tx_by_addr::InstructionErrorType::AccountBorrowFailed
                            }
                            InstructionError::AccountBorrowOutstanding => {
                                tx_by_addr::InstructionErrorType::AccountBorrowOutstanding
                            }
                            InstructionError::DuplicateAccountOutOfSync => {
                                tx_by_addr::InstructionErrorType::DuplicateAccountOutOfSync
                            }
                            InstructionError::Custom(_) => tx_by_addr::InstructionErrorType::Custom,
                            InstructionError::InvalidError => {
                                tx_by_addr::InstructionErrorType::InvalidError
                            }
                            InstructionError::ExecutableDataModified => {
                                tx_by_addr::InstructionErrorType::ExecutableDataModified
                            }
                            InstructionError::ExecutableLamportChange => {
                                tx_by_addr::InstructionErrorType::ExecutableLamportChange
                            }
                            InstructionError::ExecutableAccountNotRentExempt => {
                                tx_by_addr::InstructionErrorType::ExecutableAccountNotRentExempt
                            }
                            InstructionError::UnsupportedProgramId => {
                                tx_by_addr::InstructionErrorType::UnsupportedProgramId
                            }
                            InstructionError::CallDepth => {
                                tx_by_addr::InstructionErrorType::CallDepth
                            }
                            InstructionError::MissingAccount => {
                                tx_by_addr::InstructionErrorType::MissingAccount
                            }
                            InstructionError::ReentrancyNotAllowed => {
                                tx_by_addr::InstructionErrorType::ReentrancyNotAllowed
                            }
                            InstructionError::MaxSeedLengthExceeded => {
                                tx_by_addr::InstructionErrorType::MaxSeedLengthExceeded
                            }
                            InstructionError::InvalidSeeds => {
                                tx_by_addr::InstructionErrorType::InvalidSeeds
                            }
                            InstructionError::InvalidRealloc => {
                                tx_by_addr::InstructionErrorType::InvalidRealloc
                            }
                            InstructionError::ComputationalBudgetExceeded => {
                                tx_by_addr::InstructionErrorType::ComputationalBudgetExceeded
                            }
                            InstructionError::PrivilegeEscalation => {
                                tx_by_addr::InstructionErrorType::PrivilegeEscalation
                            }
                            InstructionError::ProgramEnvironmentSetupFailure => {
                                tx_by_addr::InstructionErrorType::ProgramEnvironmentSetupFailure
                            }
                            InstructionError::ProgramFailedToComplete => {
                                tx_by_addr::InstructionErrorType::ProgramFailedToComplete
                            }
                            InstructionError::ProgramFailedToCompile => {
                                tx_by_addr::InstructionErrorType::ProgramFailedToCompile
                            }
                            InstructionError::Immutable => {
                                tx_by_addr::InstructionErrorType::Immutable
                            }
                            InstructionError::IncorrectAuthority => {
                                tx_by_addr::InstructionErrorType::IncorrectAuthority
                            }
                            InstructionError::BorshIoError(_) => {
                                tx_by_addr::InstructionErrorType::BorshIoError
                            }
                            InstructionError::AccountNotRentExempt => {
                                tx_by_addr::InstructionErrorType::AccountNotRentExempt
                            }
                            InstructionError::InvalidAccountOwner => {
                                tx_by_addr::InstructionErrorType::InvalidAccountOwner
                            }
                            InstructionError::ArithmeticOverflow => {
                                tx_by_addr::InstructionErrorType::ArithmeticOverflow
                            }
                            InstructionError::UnsupportedSysvar => {
                                tx_by_addr::InstructionErrorType::UnsupportedSysvar
                            }
                            InstructionError::IllegalOwner => {
                                tx_by_addr::InstructionErrorType::IllegalOwner
                            }
                            InstructionError::MaxAccountsDataAllocationsExceeded => {
                                tx_by_addr::InstructionErrorType::MaxAccountsDataAllocationsExceeded
                            }
                            InstructionError::MaxAccountsExceeded => {
                                tx_by_addr::InstructionErrorType::MaxAccountsExceeded
                            }
                            InstructionError::MaxInstructionTraceLengthExceeded => {
                                tx_by_addr::InstructionErrorType::MaxInstructionTraceLengthExceeded
                            }
                            InstructionError::BuiltinProgramsMustConsumeComputeUnits => {
                                tx_by_addr::InstructionErrorType::BuiltinProgramsMustConsumeComputeUnits
                            }
                        } as i32,
                        custom: match instruction_error {
                            InstructionError::Custom(custom) => {
                                Some(tx_by_addr::CustomError { custom: *custom })
                            }
                            _ => None,
                        },
                    })
                }
                _ => None,
            },
            transaction_details: match transaction_error {
                TransactionError::DuplicateInstruction(index) => {
                    Some(tx_by_addr::TransactionDetails {
                        index: index as u32,
                    })
                }
                TransactionError::InsufficientFundsForRent { account_index } => {
                    Some(tx_by_addr::TransactionDetails {
                        index: account_index as u32,
                    })
                }
                TransactionError::ProgramExecutionTemporarilyRestricted { account_index } => {
                    Some(tx_by_addr::TransactionDetails {
                        index: account_index as u32,
                    })
                }

                _ => None,
            },
        }
    }
}

impl From<TransactionByAddrInfo> for tx_by_addr::TransactionByAddrInfo {
    fn from(by_addr: TransactionByAddrInfo) -> Self {
        let TransactionByAddrInfo {
            signature,
            err,
            index,
            memo,
            block_time,
        } = by_addr;

        Self {
            signature: <Signature as AsRef<[u8]>>::as_ref(&signature).into(),
            err: err.map(|e| e.into()),
            index,
            memo: memo.map(|memo| tx_by_addr::Memo { memo }),
            block_time: block_time.map(|timestamp| tx_by_addr::UnixTimestamp { timestamp }),
        }
    }
}

impl TryFrom<tx_by_addr::TransactionByAddrInfo> for TransactionByAddrInfo {
    type Error = &'static str;

    fn try_from(
        transaction_by_addr: tx_by_addr::TransactionByAddrInfo,
    ) -> Result<Self, Self::Error> {
        let err = transaction_by_addr
            .err
            .map(|err| err.try_into())
            .transpose()?;

        Ok(Self {
            signature: Signature::try_from(transaction_by_addr.signature)
                .map_err(|_| "Invalid Signature")?,
            err,
            index: transaction_by_addr.index,
            memo: transaction_by_addr
                .memo
                .map(|tx_by_addr::Memo { memo }| memo),
            block_time: transaction_by_addr
                .block_time
                .map(|tx_by_addr::UnixTimestamp { timestamp }| timestamp),
        })
    }
}

impl TryFrom<tx_by_addr::TransactionByAddr> for Vec<TransactionByAddrInfo> {
    type Error = &'static str;

    fn try_from(collection: tx_by_addr::TransactionByAddr) -> Result<Self, Self::Error> {
        collection
            .tx_by_addrs
            .into_iter()
            .map(|tx_by_addr| tx_by_addr.try_into())
            .collect::<Result<Vec<TransactionByAddrInfo>, Self::Error>>()
    }
}

#[cfg(test)]
mod test {
    use {super::*, enum_iterator::all};

    #[test]
    fn test_reward_type_encode() {
        let mut reward = Reward {
            pubkey: "invalid".to_string(),
            lamports: 123,
            post_balance: 321,
            reward_type: None,
            commission: None,
        };
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Fee);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Rent);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Voting);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());

        reward.reward_type = Some(RewardType::Staking);
        let gen_reward: generated::Reward = reward.clone().into();
        assert_eq!(reward, gen_reward.into());
    }

    #[test]
    fn test_transaction_by_addr_encode() {
        let info = TransactionByAddrInfo {
            signature: bs58::decode("Nfo6rgemG1KLbk1xuNwfrQTsdxaGfLuWURHNRy9LYnDrubG7LFQZaA5obPNas9LQ6DdorJqxh2LxA3PsnWdkSrL")
                .into_vec()
                .map(Signature::try_from)
                .unwrap()
                .unwrap(),
            err: None,
            index: 5,
            memo: Some("string".to_string()),
            block_time: Some(1610674861)
        };

        let tx_by_addr_transaction_info: tx_by_addr::TransactionByAddrInfo = info.clone().into();
        assert_eq!(info, tx_by_addr_transaction_info.try_into().unwrap());
    }

    #[test]
    fn test_transaction_error_encode() {
        let transaction_error = TransactionError::AccountBorrowOutstanding;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::AccountInUse;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::AccountLoadedTwice;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::AccountNotFound;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::AlreadyProcessed;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::BlockhashNotFound;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::CallChainTooDeep;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::ClusterMaintenance;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InsufficientFundsForFee;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InvalidAccountForFee;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InvalidAccountIndex;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InvalidProgramForExecution;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::MissingSignatureForFee;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::ProgramAccountNotFound;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::SanitizeFailure;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::SignatureFailure;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::WouldExceedMaxBlockCostLimit;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::WouldExceedMaxVoteCostLimit;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::WouldExceedMaxAccountCostLimit;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::UnsupportedVersion;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::AccountAlreadyInitialized);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::AccountBorrowFailed);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::AccountBorrowOutstanding);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::AccountDataSizeChanged);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::AccountDataTooSmall);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::AccountNotExecutable);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InstructionError(10, InstructionError::CallDepth);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ComputationalBudgetExceeded);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::DuplicateAccountIndex);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::DuplicateAccountOutOfSync);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InstructionError(
            10,
            InstructionError::ExecutableAccountNotRentExempt,
        );
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ExecutableDataModified);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ExecutableLamportChange);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ExecutableModified);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ExternalAccountDataModified);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ExternalAccountLamportSpend);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::GenericError);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InstructionError(10, InstructionError::Immutable);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::IncorrectAuthority);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::IncorrectProgramId);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InsufficientFunds);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InvalidAccountData);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InvalidArgument);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InvalidError);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InvalidInstructionData);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InvalidRealloc);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::InvalidSeeds);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::MaxSeedLengthExceeded);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::MissingAccount);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::MissingRequiredSignature);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ModifiedProgramId);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::NotEnoughAccountKeys);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::PrivilegeEscalation);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InstructionError(
            10,
            InstructionError::ProgramEnvironmentSetupFailure,
        );
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ProgramFailedToCompile);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ProgramFailedToComplete);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ReadonlyDataModified);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ReadonlyLamportChange);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::ReentrancyNotAllowed);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::RentEpochModified);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::UnbalancedInstruction);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::UninitializedAccount);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::UnsupportedProgramId);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error =
            TransactionError::InstructionError(10, InstructionError::Custom(10));
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::DuplicateInstruction(10);
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::InsufficientFundsForRent { account_index: 10 };
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );

        let transaction_error = TransactionError::UnbalancedTransaction;
        let tx_by_addr_transaction_error: tx_by_addr::TransactionError =
            transaction_error.clone().into();
        assert_eq!(
            transaction_error,
            tx_by_addr_transaction_error.try_into().unwrap()
        );
    }

    #[test]
    fn test_error_enums() {
        let ix_index = 1;
        let custom_error = 42;
        for error in all::<tx_by_addr::TransactionErrorType>() {
            match error {
                tx_by_addr::TransactionErrorType::DuplicateInstruction
                | tx_by_addr::TransactionErrorType::InsufficientFundsForRent
                | tx_by_addr::TransactionErrorType::ProgramExecutionTemporarilyRestricted => {
                    let tx_by_addr_error = tx_by_addr::TransactionError {
                        transaction_error: error as i32,
                        instruction_error: None,
                        transaction_details: Some(tx_by_addr::TransactionDetails {
                            index: ix_index,
                        }),
                    };
                    let transaction_error: TransactionError = tx_by_addr_error
                        .clone()
                        .try_into()
                        .unwrap_or_else(|_| panic!("{error:?} conversion implemented?"));
                    assert_eq!(tx_by_addr_error, transaction_error.into());
                }
                tx_by_addr::TransactionErrorType::InstructionError => {
                    for ix_error in all::<tx_by_addr::InstructionErrorType>() {
                        if ix_error != tx_by_addr::InstructionErrorType::Custom {
                            let tx_by_addr_error = tx_by_addr::TransactionError {
                                transaction_error: error as i32,
                                instruction_error: Some(tx_by_addr::InstructionError {
                                    index: ix_index,
                                    error: ix_error as i32,
                                    custom: None,
                                }),
                                transaction_details: None,
                            };
                            let transaction_error: TransactionError = tx_by_addr_error
                                .clone()
                                .try_into()
                                .unwrap_or_else(|_| panic!("{ix_error:?} conversion implemented?"));
                            assert_eq!(tx_by_addr_error, transaction_error.into());
                        } else {
                            let tx_by_addr_error = tx_by_addr::TransactionError {
                                transaction_error: error as i32,
                                instruction_error: Some(tx_by_addr::InstructionError {
                                    index: ix_index,
                                    error: ix_error as i32,
                                    custom: Some(tx_by_addr::CustomError {
                                        custom: custom_error,
                                    }),
                                }),
                                transaction_details: None,
                            };
                            let transaction_error: TransactionError =
                                tx_by_addr_error.clone().try_into().unwrap();
                            assert_eq!(tx_by_addr_error, transaction_error.into());
                        }
                    }
                }
                _ => {
                    let tx_by_addr_error = tx_by_addr::TransactionError {
                        transaction_error: error as i32,
                        instruction_error: None,
                        transaction_details: None,
                    };
                    let transaction_error: TransactionError = tx_by_addr_error
                        .clone()
                        .try_into()
                        .unwrap_or_else(|_| panic!("{error:?} conversion implemented?"));
                    assert_eq!(tx_by_addr_error, transaction_error.into());
                }
            }
        }
    }
}
