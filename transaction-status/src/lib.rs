#![allow(clippy::arithmetic_side_effects)]

pub use {
    crate::extract_memos::extract_and_fmt_memos,
    solana_sdk::reward_type::RewardType,
    solana_transaction_status_client_types::{
        option_serializer, ConfirmedTransactionStatusWithSignature, EncodeError,
        EncodedConfirmedBlock, EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction,
        EncodedTransactionWithStatusMeta, InnerInstruction, InnerInstructions, Reward, Rewards,
        TransactionBinaryEncoding, TransactionConfirmationStatus, TransactionDetails,
        TransactionStatus, TransactionStatusMeta, TransactionTokenBalance, UiAccountsList,
        UiAddressTableLookup, UiCompiledInstruction, UiConfirmedBlock, UiInnerInstructions,
        UiInstruction, UiLoadedAddresses, UiMessage, UiParsedInstruction, UiParsedMessage,
        UiPartiallyDecodedInstruction, UiRawMessage, UiReturnDataEncoding, UiTransaction,
        UiTransactionEncoding, UiTransactionReturnData, UiTransactionStatusMeta,
        UiTransactionTokenBalance,
    },
};
use {
    crate::{
        option_serializer::OptionSerializer,
        parse_accounts::{parse_legacy_message_accounts, parse_v0_message_accounts},
        parse_instruction::parse,
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        hash::Hash,
        instruction::CompiledInstruction,
        message::{
            v0::{self, LoadedAddresses, LoadedMessage},
            AccountKeys, Message, VersionedMessage,
        },
        pubkey::Pubkey,
        reserved_account_keys::ReservedAccountKeys,
        signature::Signature,
        transaction::{Transaction, TransactionError, TransactionVersion, VersionedTransaction},
    },
    std::collections::HashSet,
    thiserror::Error,
};

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod extract_memos;
pub mod parse_accounts;
pub mod parse_address_lookup_table;
pub mod parse_associated_token;
pub mod parse_bpf_loader;
pub mod parse_instruction;
pub mod parse_stake;
pub mod parse_system;
pub mod parse_token;
pub mod parse_vote;
pub mod token_balances;

pub struct BlockEncodingOptions {
    pub transaction_details: TransactionDetails,
    pub show_rewards: bool,
    pub max_supported_transaction_version: Option<u8>,
}

/// Represents types that can be encoded into one of several encoding formats
pub trait Encodable {
    type Encoded;
    fn encode(&self, encoding: UiTransactionEncoding) -> Self::Encoded;
}

/// Represents types that can be encoded into one of several encoding formats
pub trait EncodableWithMeta {
    type Encoded;
    fn encode_with_meta(
        &self,
        encoding: UiTransactionEncoding,
        meta: &TransactionStatusMeta,
    ) -> Self::Encoded;
    fn json_encode(&self) -> Self::Encoded;
}

trait JsonAccounts {
    type Encoded;
    fn build_json_accounts(&self) -> Self::Encoded;
}

fn make_ui_partially_decoded_instruction(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
    stack_height: Option<u32>,
) -> UiPartiallyDecodedInstruction {
    UiPartiallyDecodedInstruction {
        program_id: account_keys[instruction.program_id_index as usize].to_string(),
        accounts: instruction
            .accounts
            .iter()
            .map(|&i| account_keys[i as usize].to_string())
            .collect(),
        data: bs58::encode(instruction.data.clone()).into_string(),
        stack_height,
    }
}

pub fn parse_ui_instruction(
    instruction: &CompiledInstruction,
    account_keys: &AccountKeys,
    stack_height: Option<u32>,
) -> UiInstruction {
    let program_id = &account_keys[instruction.program_id_index as usize];
    if let Ok(parsed_instruction) = parse(program_id, instruction, account_keys, stack_height) {
        UiInstruction::Parsed(UiParsedInstruction::Parsed(parsed_instruction))
    } else {
        UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(
            make_ui_partially_decoded_instruction(instruction, account_keys, stack_height),
        ))
    }
}

/// Maps a list of inner instructions from `solana_sdk` into a list of this
/// crate's representation of inner instructions (with instruction indices).
pub fn map_inner_instructions(
    inner_instructions: solana_sdk::inner_instruction::InnerInstructionsList,
) -> impl Iterator<Item = InnerInstructions> {
    inner_instructions
        .into_iter()
        .enumerate()
        .map(|(index, instructions)| InnerInstructions {
            index: index as u8,
            instructions: instructions
                .into_iter()
                .map(|info| InnerInstruction {
                    stack_height: Some(u32::from(info.stack_height)),
                    instruction: info.instruction,
                })
                .collect(),
        })
        .filter(|i| !i.instructions.is_empty())
}

pub fn parse_ui_inner_instructions(
    inner_instructions: InnerInstructions,
    account_keys: &AccountKeys,
) -> UiInnerInstructions {
    UiInnerInstructions {
        index: inner_instructions.index,
        instructions: inner_instructions
            .instructions
            .iter()
            .map(
                |InnerInstruction {
                     instruction: ix,
                     stack_height,
                 }| { parse_ui_instruction(ix, account_keys, *stack_height) },
            )
            .collect(),
    }
}

fn build_simple_ui_transaction_status_meta(
    meta: TransactionStatusMeta,
    show_rewards: bool,
) -> UiTransactionStatusMeta {
    UiTransactionStatusMeta {
        err: meta.status.clone().err(),
        status: meta.status,
        fee: meta.fee,
        pre_balances: meta.pre_balances,
        post_balances: meta.post_balances,
        inner_instructions: OptionSerializer::Skip,
        log_messages: OptionSerializer::Skip,
        pre_token_balances: meta
            .pre_token_balances
            .map(|balance| balance.into_iter().map(Into::into).collect())
            .into(),
        post_token_balances: meta
            .post_token_balances
            .map(|balance| balance.into_iter().map(Into::into).collect())
            .into(),
        rewards: if show_rewards {
            meta.rewards.into()
        } else {
            OptionSerializer::Skip
        },
        loaded_addresses: OptionSerializer::Skip,
        return_data: OptionSerializer::Skip,
        compute_units_consumed: OptionSerializer::Skip,
    }
}

fn parse_ui_transaction_status_meta(
    meta: TransactionStatusMeta,
    static_keys: &[Pubkey],
    show_rewards: bool,
) -> UiTransactionStatusMeta {
    let account_keys = AccountKeys::new(static_keys, Some(&meta.loaded_addresses));
    UiTransactionStatusMeta {
        err: meta.status.clone().err(),
        status: meta.status,
        fee: meta.fee,
        pre_balances: meta.pre_balances,
        post_balances: meta.post_balances,
        inner_instructions: meta
            .inner_instructions
            .map(|ixs| {
                ixs.into_iter()
                    .map(|ix| parse_ui_inner_instructions(ix, &account_keys))
                    .collect()
            })
            .into(),
        log_messages: meta.log_messages.into(),
        pre_token_balances: meta
            .pre_token_balances
            .map(|balance| balance.into_iter().map(Into::into).collect())
            .into(),
        post_token_balances: meta
            .post_token_balances
            .map(|balance| balance.into_iter().map(Into::into).collect())
            .into(),
        rewards: if show_rewards { meta.rewards } else { None }.into(),
        loaded_addresses: OptionSerializer::Skip,
        return_data: OptionSerializer::or_skip(
            meta.return_data.map(|return_data| return_data.into()),
        ),
        compute_units_consumed: OptionSerializer::or_skip(meta.compute_units_consumed),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardsAndNumPartitions {
    pub rewards: Rewards,
    pub num_partitions: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ConvertBlockError {
    #[error("transactions missing after converted, before: {0}, after: {1}")]
    TransactionsMissing(usize, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub transactions: Vec<TransactionWithStatusMeta>,
    pub rewards: Rewards,
    pub num_partitions: Option<u64>,
    pub block_time: Option<UnixTimestamp>,
    pub block_height: Option<u64>,
}

// Confirmed block with type guarantees that transaction metadata
// is always present. Used for uploading to BigTable.
#[derive(Clone, Debug, PartialEq)]
pub struct VersionedConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub transactions: Vec<VersionedTransactionWithStatusMeta>,
    pub rewards: Rewards,
    pub num_partitions: Option<u64>,
    pub block_time: Option<UnixTimestamp>,
    pub block_height: Option<u64>,
}

impl From<VersionedConfirmedBlock> for ConfirmedBlock {
    fn from(block: VersionedConfirmedBlock) -> Self {
        Self {
            previous_blockhash: block.previous_blockhash,
            blockhash: block.blockhash,
            parent_slot: block.parent_slot,
            transactions: block
                .transactions
                .into_iter()
                .map(TransactionWithStatusMeta::Complete)
                .collect(),
            rewards: block.rewards,
            num_partitions: block.num_partitions,
            block_time: block.block_time,
            block_height: block.block_height,
        }
    }
}

impl TryFrom<ConfirmedBlock> for VersionedConfirmedBlock {
    type Error = ConvertBlockError;

    fn try_from(block: ConfirmedBlock) -> Result<Self, Self::Error> {
        let expected_transaction_count = block.transactions.len();

        let txs: Vec<_> = block
            .transactions
            .into_iter()
            .filter_map(|tx| match tx {
                TransactionWithStatusMeta::MissingMetadata(_) => None,
                TransactionWithStatusMeta::Complete(tx) => Some(tx),
            })
            .collect();

        if txs.len() != expected_transaction_count {
            return Err(ConvertBlockError::TransactionsMissing(
                expected_transaction_count,
                txs.len(),
            ));
        }

        Ok(Self {
            previous_blockhash: block.previous_blockhash,
            blockhash: block.blockhash,
            parent_slot: block.parent_slot,
            transactions: txs,
            rewards: block.rewards,
            num_partitions: block.num_partitions,
            block_time: block.block_time,
            block_height: block.block_height,
        })
    }
}

impl ConfirmedBlock {
    pub fn encode_with_options(
        self,
        encoding: UiTransactionEncoding,
        options: BlockEncodingOptions,
    ) -> Result<UiConfirmedBlock, EncodeError> {
        let (transactions, signatures) = match options.transaction_details {
            TransactionDetails::Full => (
                Some(
                    self.transactions
                        .into_iter()
                        .map(|tx_with_meta| {
                            tx_with_meta.encode(
                                encoding,
                                options.max_supported_transaction_version,
                                options.show_rewards,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None,
            ),
            TransactionDetails::Signatures => (
                None,
                Some(
                    self.transactions
                        .into_iter()
                        .map(|tx_with_meta| tx_with_meta.transaction_signature().to_string())
                        .collect(),
                ),
            ),
            TransactionDetails::None => (None, None),
            TransactionDetails::Accounts => (
                Some(
                    self.transactions
                        .into_iter()
                        .map(|tx_with_meta| {
                            tx_with_meta.build_json_accounts(
                                options.max_supported_transaction_version,
                                options.show_rewards,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None,
            ),
        };
        Ok(UiConfirmedBlock {
            previous_blockhash: self.previous_blockhash,
            blockhash: self.blockhash,
            parent_slot: self.parent_slot,
            transactions,
            signatures,
            rewards: if options.show_rewards {
                Some(self.rewards)
            } else {
                None
            },
            num_reward_partitions: self.num_partitions,
            block_time: self.block_time,
            block_height: self.block_height,
        })
    }
}

// Confirmed block with type guarantees that transaction metadata is always
// present, as well as a list of the entry data needed to cryptographically
// verify the block. Used for uploading to BigTable.
pub struct VersionedConfirmedBlockWithEntries {
    pub block: VersionedConfirmedBlock,
    pub entries: Vec<EntrySummary>,
}

// Data needed to reconstruct an Entry, given an ordered list of transactions in
// a block. Used for uploading to BigTable.
pub struct EntrySummary {
    pub num_hashes: u64,
    pub hash: Hash,
    pub num_transactions: u64,
    pub starting_transaction_index: usize,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum TransactionWithStatusMeta {
    // Very old transactions may be missing metadata
    MissingMetadata(Transaction),
    // Versioned stored transaction always have metadata
    Complete(VersionedTransactionWithStatusMeta),
}

#[derive(Clone, Debug, PartialEq)]
pub struct VersionedTransactionWithStatusMeta {
    pub transaction: VersionedTransaction,
    pub meta: TransactionStatusMeta,
}

impl TransactionWithStatusMeta {
    pub fn get_status_meta(&self) -> Option<TransactionStatusMeta> {
        match self {
            Self::MissingMetadata(_) => None,
            Self::Complete(tx_with_meta) => Some(tx_with_meta.meta.clone()),
        }
    }

    pub fn get_transaction(&self) -> VersionedTransaction {
        match self {
            Self::MissingMetadata(transaction) => VersionedTransaction::from(transaction.clone()),
            Self::Complete(tx_with_meta) => tx_with_meta.transaction.clone(),
        }
    }

    pub fn transaction_signature(&self) -> &Signature {
        match self {
            Self::MissingMetadata(transaction) => &transaction.signatures[0],
            Self::Complete(VersionedTransactionWithStatusMeta { transaction, .. }) => {
                &transaction.signatures[0]
            }
        }
    }

    pub fn encode(
        self,
        encoding: UiTransactionEncoding,
        max_supported_transaction_version: Option<u8>,
        show_rewards: bool,
    ) -> Result<EncodedTransactionWithStatusMeta, EncodeError> {
        match self {
            Self::MissingMetadata(ref transaction) => Ok(EncodedTransactionWithStatusMeta {
                version: None,
                transaction: transaction.encode(encoding),
                meta: None,
            }),
            Self::Complete(tx_with_meta) => {
                tx_with_meta.encode(encoding, max_supported_transaction_version, show_rewards)
            }
        }
    }

    pub fn account_keys(&self) -> AccountKeys {
        match self {
            Self::MissingMetadata(tx) => AccountKeys::new(&tx.message.account_keys, None),
            Self::Complete(tx_with_meta) => tx_with_meta.account_keys(),
        }
    }

    fn build_json_accounts(
        self,
        max_supported_transaction_version: Option<u8>,
        show_rewards: bool,
    ) -> Result<EncodedTransactionWithStatusMeta, EncodeError> {
        match self {
            Self::MissingMetadata(ref transaction) => Ok(EncodedTransactionWithStatusMeta {
                version: None,
                transaction: transaction.build_json_accounts(),
                meta: None,
            }),
            Self::Complete(tx_with_meta) => {
                tx_with_meta.build_json_accounts(max_supported_transaction_version, show_rewards)
            }
        }
    }
}

impl VersionedTransactionWithStatusMeta {
    fn validate_version(
        &self,
        max_supported_transaction_version: Option<u8>,
    ) -> Result<Option<TransactionVersion>, EncodeError> {
        match (
            max_supported_transaction_version,
            self.transaction.version(),
        ) {
            // Set to none because old clients can't handle this field
            (None, TransactionVersion::LEGACY) => Ok(None),
            (None, TransactionVersion::Number(version)) => {
                Err(EncodeError::UnsupportedTransactionVersion(version))
            }
            (Some(_), TransactionVersion::LEGACY) => Ok(Some(TransactionVersion::LEGACY)),
            (Some(max_version), TransactionVersion::Number(version)) => {
                if version <= max_version {
                    Ok(Some(TransactionVersion::Number(version)))
                } else {
                    Err(EncodeError::UnsupportedTransactionVersion(version))
                }
            }
        }
    }

    pub fn encode(
        self,
        encoding: UiTransactionEncoding,
        max_supported_transaction_version: Option<u8>,
        show_rewards: bool,
    ) -> Result<EncodedTransactionWithStatusMeta, EncodeError> {
        let version = self.validate_version(max_supported_transaction_version)?;

        Ok(EncodedTransactionWithStatusMeta {
            transaction: self.transaction.encode_with_meta(encoding, &self.meta),
            meta: Some(match encoding {
                UiTransactionEncoding::JsonParsed => parse_ui_transaction_status_meta(
                    self.meta,
                    self.transaction.message.static_account_keys(),
                    show_rewards,
                ),
                _ => {
                    let mut meta = UiTransactionStatusMeta::from(self.meta);
                    if !show_rewards {
                        meta.rewards = OptionSerializer::None;
                    }
                    meta
                }
            }),
            version,
        })
    }

    pub fn account_keys(&self) -> AccountKeys {
        AccountKeys::new(
            self.transaction.message.static_account_keys(),
            Some(&self.meta.loaded_addresses),
        )
    }

    fn build_json_accounts(
        self,
        max_supported_transaction_version: Option<u8>,
        show_rewards: bool,
    ) -> Result<EncodedTransactionWithStatusMeta, EncodeError> {
        let version = self.validate_version(max_supported_transaction_version)?;
        let reserved_account_keys = ReservedAccountKeys::new_all_activated();

        let account_keys = match &self.transaction.message {
            VersionedMessage::Legacy(message) => parse_legacy_message_accounts(message),
            VersionedMessage::V0(message) => {
                let loaded_message = LoadedMessage::new_borrowed(
                    message,
                    &self.meta.loaded_addresses,
                    &reserved_account_keys.active,
                );
                parse_v0_message_accounts(&loaded_message)
            }
        };

        Ok(EncodedTransactionWithStatusMeta {
            transaction: EncodedTransaction::Accounts(UiAccountsList {
                signatures: self
                    .transaction
                    .signatures
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                account_keys,
            }),
            meta: Some(build_simple_ui_transaction_status_meta(
                self.meta,
                show_rewards,
            )),
            version,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedTransactionWithStatusMeta {
    pub slot: Slot,
    pub tx_with_meta: TransactionWithStatusMeta,
    pub block_time: Option<UnixTimestamp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionedConfirmedTransactionWithStatusMeta {
    pub slot: Slot,
    pub tx_with_meta: VersionedTransactionWithStatusMeta,
    pub block_time: Option<UnixTimestamp>,
}

impl ConfirmedTransactionWithStatusMeta {
    pub fn encode(
        self,
        encoding: UiTransactionEncoding,
        max_supported_transaction_version: Option<u8>,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta, EncodeError> {
        Ok(EncodedConfirmedTransactionWithStatusMeta {
            slot: self.slot,
            transaction: self.tx_with_meta.encode(
                encoding,
                max_supported_transaction_version,
                true,
            )?,
            block_time: self.block_time,
        })
    }

    pub fn get_transaction(&self) -> VersionedTransaction {
        self.tx_with_meta.get_transaction()
    }
}

impl EncodableWithMeta for VersionedTransaction {
    type Encoded = EncodedTransaction;
    fn encode_with_meta(
        &self,
        encoding: UiTransactionEncoding,
        meta: &TransactionStatusMeta,
    ) -> Self::Encoded {
        match encoding {
            UiTransactionEncoding::Binary => EncodedTransaction::LegacyBinary(
                bs58::encode(bincode::serialize(self).unwrap()).into_string(),
            ),
            UiTransactionEncoding::Base58 => EncodedTransaction::Binary(
                bs58::encode(bincode::serialize(self).unwrap()).into_string(),
                TransactionBinaryEncoding::Base58,
            ),
            UiTransactionEncoding::Base64 => EncodedTransaction::Binary(
                BASE64_STANDARD.encode(bincode::serialize(self).unwrap()),
                TransactionBinaryEncoding::Base64,
            ),
            UiTransactionEncoding::Json => self.json_encode(),
            UiTransactionEncoding::JsonParsed => EncodedTransaction::Json(UiTransaction {
                signatures: self.signatures.iter().map(ToString::to_string).collect(),
                message: match &self.message {
                    VersionedMessage::Legacy(message) => {
                        message.encode(UiTransactionEncoding::JsonParsed)
                    }
                    VersionedMessage::V0(message) => {
                        message.encode_with_meta(UiTransactionEncoding::JsonParsed, meta)
                    }
                },
            }),
        }
    }
    fn json_encode(&self) -> Self::Encoded {
        EncodedTransaction::Json(UiTransaction {
            signatures: self.signatures.iter().map(ToString::to_string).collect(),
            message: match &self.message {
                VersionedMessage::Legacy(message) => message.encode(UiTransactionEncoding::Json),
                VersionedMessage::V0(message) => message.json_encode(),
            },
        })
    }
}

impl Encodable for VersionedTransaction {
    type Encoded = EncodedTransaction;
    fn encode(&self, encoding: UiTransactionEncoding) -> Self::Encoded {
        match encoding {
            UiTransactionEncoding::Binary => EncodedTransaction::LegacyBinary(
                bs58::encode(bincode::serialize(self).unwrap()).into_string(),
            ),
            UiTransactionEncoding::Base58 => EncodedTransaction::Binary(
                bs58::encode(bincode::serialize(self).unwrap()).into_string(),
                TransactionBinaryEncoding::Base58,
            ),
            UiTransactionEncoding::Base64 => EncodedTransaction::Binary(
                BASE64_STANDARD.encode(bincode::serialize(self).unwrap()),
                TransactionBinaryEncoding::Base64,
            ),
            UiTransactionEncoding::Json | UiTransactionEncoding::JsonParsed => {
                EncodedTransaction::Json(UiTransaction {
                    signatures: self.signatures.iter().map(ToString::to_string).collect(),
                    message: match &self.message {
                        VersionedMessage::Legacy(message) => {
                            message.encode(UiTransactionEncoding::JsonParsed)
                        }
                        VersionedMessage::V0(message) => {
                            message.encode(UiTransactionEncoding::JsonParsed)
                        }
                    },
                })
            }
        }
    }
}

impl Encodable for Transaction {
    type Encoded = EncodedTransaction;
    fn encode(&self, encoding: UiTransactionEncoding) -> Self::Encoded {
        match encoding {
            UiTransactionEncoding::Binary => EncodedTransaction::LegacyBinary(
                bs58::encode(bincode::serialize(self).unwrap()).into_string(),
            ),
            UiTransactionEncoding::Base58 => EncodedTransaction::Binary(
                bs58::encode(bincode::serialize(self).unwrap()).into_string(),
                TransactionBinaryEncoding::Base58,
            ),
            UiTransactionEncoding::Base64 => EncodedTransaction::Binary(
                BASE64_STANDARD.encode(bincode::serialize(self).unwrap()),
                TransactionBinaryEncoding::Base64,
            ),
            UiTransactionEncoding::Json | UiTransactionEncoding::JsonParsed => {
                EncodedTransaction::Json(UiTransaction {
                    signatures: self.signatures.iter().map(ToString::to_string).collect(),
                    message: self.message.encode(encoding),
                })
            }
        }
    }
}

impl JsonAccounts for Transaction {
    type Encoded = EncodedTransaction;
    fn build_json_accounts(&self) -> Self::Encoded {
        EncodedTransaction::Accounts(UiAccountsList {
            signatures: self.signatures.iter().map(ToString::to_string).collect(),
            account_keys: parse_legacy_message_accounts(&self.message),
        })
    }
}

impl Encodable for Message {
    type Encoded = UiMessage;
    fn encode(&self, encoding: UiTransactionEncoding) -> Self::Encoded {
        if encoding == UiTransactionEncoding::JsonParsed {
            let account_keys = AccountKeys::new(&self.account_keys, None);
            UiMessage::Parsed(UiParsedMessage {
                account_keys: parse_legacy_message_accounts(self),
                recent_blockhash: self.recent_blockhash.to_string(),
                instructions: self
                    .instructions
                    .iter()
                    .map(|instruction| parse_ui_instruction(instruction, &account_keys, None))
                    .collect(),
                address_table_lookups: None,
            })
        } else {
            UiMessage::Raw(UiRawMessage {
                header: self.header,
                account_keys: self.account_keys.iter().map(ToString::to_string).collect(),
                recent_blockhash: self.recent_blockhash.to_string(),
                instructions: self
                    .instructions
                    .iter()
                    .map(|ix| UiCompiledInstruction::from(ix, None))
                    .collect(),
                address_table_lookups: None,
            })
        }
    }
}

impl Encodable for v0::Message {
    type Encoded = UiMessage;
    fn encode(&self, encoding: UiTransactionEncoding) -> Self::Encoded {
        if encoding == UiTransactionEncoding::JsonParsed {
            let account_keys = AccountKeys::new(&self.account_keys, None);
            let loaded_addresses = LoadedAddresses::default();
            let loaded_message =
                LoadedMessage::new_borrowed(self, &loaded_addresses, &HashSet::new());
            UiMessage::Parsed(UiParsedMessage {
                account_keys: parse_v0_message_accounts(&loaded_message),
                recent_blockhash: self.recent_blockhash.to_string(),
                instructions: self
                    .instructions
                    .iter()
                    .map(|instruction| parse_ui_instruction(instruction, &account_keys, None))
                    .collect(),
                address_table_lookups: None,
            })
        } else {
            UiMessage::Raw(UiRawMessage {
                header: self.header,
                account_keys: self.account_keys.iter().map(ToString::to_string).collect(),
                recent_blockhash: self.recent_blockhash.to_string(),
                instructions: self
                    .instructions
                    .iter()
                    .map(|ix| UiCompiledInstruction::from(ix, None))
                    .collect(),
                address_table_lookups: None,
            })
        }
    }
}

impl EncodableWithMeta for v0::Message {
    type Encoded = UiMessage;
    fn encode_with_meta(
        &self,
        encoding: UiTransactionEncoding,
        meta: &TransactionStatusMeta,
    ) -> Self::Encoded {
        if encoding == UiTransactionEncoding::JsonParsed {
            let reserved_account_keys = ReservedAccountKeys::new_all_activated();
            let account_keys = AccountKeys::new(&self.account_keys, Some(&meta.loaded_addresses));
            let loaded_message = LoadedMessage::new_borrowed(
                self,
                &meta.loaded_addresses,
                &reserved_account_keys.active,
            );
            UiMessage::Parsed(UiParsedMessage {
                account_keys: parse_v0_message_accounts(&loaded_message),
                recent_blockhash: self.recent_blockhash.to_string(),
                instructions: self
                    .instructions
                    .iter()
                    .map(|instruction| parse_ui_instruction(instruction, &account_keys, None))
                    .collect(),
                address_table_lookups: Some(
                    self.address_table_lookups.iter().map(Into::into).collect(),
                ),
            })
        } else {
            self.json_encode()
        }
    }
    fn json_encode(&self) -> Self::Encoded {
        UiMessage::Raw(UiRawMessage {
            header: self.header,
            account_keys: self.account_keys.iter().map(ToString::to_string).collect(),
            recent_blockhash: self.recent_blockhash.to_string(),
            instructions: self
                .instructions
                .iter()
                .map(|ix| UiCompiledInstruction::from(ix, None))
                .collect(),
            address_table_lookups: Some(
                self.address_table_lookups.iter().map(Into::into).collect(),
            ),
        })
    }
}

// A serialized `Vec<TransactionByAddrInfo>` is stored in the `tx-by-addr` table.  The row keys are
// the one's compliment of the slot so that rows may be listed in reverse order
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionByAddrInfo {
    pub signature: Signature,          // The transaction signature
    pub err: Option<TransactionError>, // None if the transaction executed successfully
    pub index: u32,                    // Where the transaction is located in the block
    pub memo: Option<String>,          // Transaction memo
    pub block_time: Option<UnixTimestamp>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ui_transaction_status_meta_ctors_serialization() {
        let meta = TransactionStatusMeta {
            status: Ok(()),
            fee: 1234,
            pre_balances: vec![1, 2, 3],
            post_balances: vec![4, 5, 6],
            inner_instructions: None,
            log_messages: None,
            pre_token_balances: None,
            post_token_balances: None,
            rewards: None,
            loaded_addresses: LoadedAddresses {
                writable: vec![],
                readonly: vec![],
            },
            return_data: None,
            compute_units_consumed: None,
        };
        let expected_json_output_value: serde_json::Value = serde_json::from_str(
            "{\
            \"err\":null,\
            \"status\":{\"Ok\":null},\
            \"fee\":1234,\
            \"preBalances\":[1,2,3],\
            \"postBalances\":[4,5,6],\
            \"innerInstructions\":null,\
            \"logMessages\":null,\
            \"preTokenBalances\":null,\
            \"postTokenBalances\":null,\
            \"rewards\":null,\
            \"loadedAddresses\":{\
                \"readonly\": [],\
                \"writable\": []\
            }\
        }",
        )
        .unwrap();
        let ui_meta_from: UiTransactionStatusMeta = meta.clone().into();
        assert_eq!(
            serde_json::to_value(ui_meta_from).unwrap(),
            expected_json_output_value
        );

        let expected_json_output_value: serde_json::Value = serde_json::from_str(
            "{\
            \"err\":null,\
            \"status\":{\"Ok\":null},\
            \"fee\":1234,\
            \"preBalances\":[1,2,3],\
            \"postBalances\":[4,5,6],\
            \"innerInstructions\":null,\
            \"logMessages\":null,\
            \"preTokenBalances\":null,\
            \"postTokenBalances\":null,\
            \"rewards\":null\
        }",
        )
        .unwrap();
        let ui_meta_parse_with_rewards = parse_ui_transaction_status_meta(meta.clone(), &[], true);
        assert_eq!(
            serde_json::to_value(ui_meta_parse_with_rewards).unwrap(),
            expected_json_output_value
        );

        let ui_meta_parse_no_rewards = parse_ui_transaction_status_meta(meta, &[], false);
        assert_eq!(
            serde_json::to_value(ui_meta_parse_no_rewards).unwrap(),
            expected_json_output_value
        );
    }
}
