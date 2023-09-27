#![allow(clippy::arithmetic_side_effects)]

pub use {crate::extract_memos::extract_and_fmt_memos, solana_sdk::reward_type::RewardType};
use {
    crate::{
        option_serializer::OptionSerializer,
        parse_accounts::{parse_legacy_message_accounts, parse_v0_message_accounts, ParsedAccount},
        parse_instruction::{parse, ParsedInstruction},
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    solana_account_decoder::parse_token::UiTokenAmount,
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        commitment_config::CommitmentConfig,
        instruction::CompiledInstruction,
        message::{
            v0::{self, LoadedAddresses, LoadedMessage, MessageAddressTableLookup},
            AccountKeys, Message, MessageHeader, VersionedMessage,
        },
        pubkey::Pubkey,
        signature::Signature,
        transaction::{
            Result as TransactionResult, Transaction, TransactionError, TransactionVersion,
            VersionedTransaction,
        },
        transaction_context::TransactionReturnData,
    },
    std::fmt,
    thiserror::Error,
};

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod extract_memos;
pub mod option_serializer;
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

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum EncodeError {
    #[error("Encoding does not support transaction version {0}")]
    UnsupportedTransactionVersion(u8),
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransactionBinaryEncoding {
    Base58,
    Base64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiTransactionEncoding {
    Binary, // Legacy. Retained for RPC backwards compatibility
    Base64,
    Base58,
    Json,
    JsonParsed,
}

impl UiTransactionEncoding {
    pub fn into_binary_encoding(&self) -> Option<TransactionBinaryEncoding> {
        match self {
            Self::Binary | Self::Base58 => Some(TransactionBinaryEncoding::Base58),
            Self::Base64 => Some(TransactionBinaryEncoding::Base64),
            _ => None,
        }
    }
}

impl fmt::Display for UiTransactionEncoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let v = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        let s = v.as_str().ok_or(fmt::Error)?;
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionDetails {
    Full,
    Signatures,
    None,
    Accounts,
}

impl Default for TransactionDetails {
    fn default() -> Self {
        Self::Full
    }
}

/// A duplicate representation of an Instruction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiInstruction {
    Compiled(UiCompiledInstruction),
    Parsed(UiParsedInstruction),
}

impl UiInstruction {
    fn parse(
        instruction: &CompiledInstruction,
        account_keys: &AccountKeys,
        stack_height: Option<u32>,
    ) -> Self {
        let program_id = &account_keys[instruction.program_id_index as usize];
        if let Ok(parsed_instruction) = parse(program_id, instruction, account_keys, stack_height) {
            UiInstruction::Parsed(UiParsedInstruction::Parsed(parsed_instruction))
        } else {
            UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(
                UiPartiallyDecodedInstruction::from(instruction, account_keys, stack_height),
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiParsedInstruction {
    Parsed(ParsedInstruction),
    PartiallyDecoded(UiPartiallyDecodedInstruction),
}

/// A duplicate representation of a CompiledInstruction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiCompiledInstruction {
    pub program_id_index: u8,
    pub accounts: Vec<u8>,
    pub data: String,
    pub stack_height: Option<u32>,
}

impl UiCompiledInstruction {
    fn from(instruction: &CompiledInstruction, stack_height: Option<u32>) -> Self {
        Self {
            program_id_index: instruction.program_id_index,
            accounts: instruction.accounts.clone(),
            data: bs58::encode(&instruction.data).into_string(),
            stack_height,
        }
    }
}

/// A partially decoded CompiledInstruction that includes explicit account addresses
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPartiallyDecodedInstruction {
    pub program_id: String,
    pub accounts: Vec<String>,
    pub data: String,
    pub stack_height: Option<u32>,
}

impl UiPartiallyDecodedInstruction {
    fn from(
        instruction: &CompiledInstruction,
        account_keys: &AccountKeys,
        stack_height: Option<u32>,
    ) -> Self {
        Self {
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnerInstructions {
    /// Transaction instruction index
    pub index: u8,
    /// List of inner instructions
    pub instructions: Vec<InnerInstruction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnerInstruction {
    /// Compiled instruction
    pub instruction: CompiledInstruction,
    /// Invocation stack height of the instruction,
    pub stack_height: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiInnerInstructions {
    /// Transaction instruction index
    pub index: u8,
    /// List of inner instructions
    pub instructions: Vec<UiInstruction>,
}

impl UiInnerInstructions {
    fn parse(inner_instructions: InnerInstructions, account_keys: &AccountKeys) -> Self {
        Self {
            index: inner_instructions.index,
            instructions: inner_instructions
                .instructions
                .iter()
                .map(
                    |InnerInstruction {
                         instruction: ix,
                         stack_height,
                     }| {
                        UiInstruction::parse(ix, account_keys, *stack_height)
                    },
                )
                .collect(),
        }
    }
}

impl From<InnerInstructions> for UiInnerInstructions {
    fn from(inner_instructions: InnerInstructions) -> Self {
        Self {
            index: inner_instructions.index,
            instructions: inner_instructions
                .instructions
                .iter()
                .map(
                    |InnerInstruction {
                         instruction: ix,
                         stack_height,
                     }| {
                        UiInstruction::Compiled(UiCompiledInstruction::from(ix, *stack_height))
                    },
                )
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransactionTokenBalance {
    pub account_index: u8,
    pub mint: String,
    pub ui_token_amount: UiTokenAmount,
    pub owner: String,
    pub program_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTransactionTokenBalance {
    pub account_index: u8,
    pub mint: String,
    pub ui_token_amount: UiTokenAmount,
    #[serde(
        default = "OptionSerializer::skip",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub owner: OptionSerializer<String>,
    #[serde(
        default = "OptionSerializer::skip",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub program_id: OptionSerializer<String>,
}

impl From<TransactionTokenBalance> for UiTransactionTokenBalance {
    fn from(token_balance: TransactionTokenBalance) -> Self {
        Self {
            account_index: token_balance.account_index,
            mint: token_balance.mint,
            ui_token_amount: token_balance.ui_token_amount,
            owner: if !token_balance.owner.is_empty() {
                OptionSerializer::Some(token_balance.owner)
            } else {
                OptionSerializer::Skip
            },
            program_id: if !token_balance.program_id.is_empty() {
                OptionSerializer::Some(token_balance.program_id)
            } else {
                OptionSerializer::Skip
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransactionStatusMeta {
    pub status: TransactionResult<()>,
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
    pub inner_instructions: Option<Vec<InnerInstructions>>,
    pub log_messages: Option<Vec<String>>,
    pub pre_token_balances: Option<Vec<TransactionTokenBalance>>,
    pub post_token_balances: Option<Vec<TransactionTokenBalance>>,
    pub rewards: Option<Rewards>,
    pub loaded_addresses: LoadedAddresses,
    pub return_data: Option<TransactionReturnData>,
    pub compute_units_consumed: Option<u64>,
}

impl Default for TransactionStatusMeta {
    fn default() -> Self {
        Self {
            status: Ok(()),
            fee: 0,
            pre_balances: vec![],
            post_balances: vec![],
            inner_instructions: None,
            log_messages: None,
            pre_token_balances: None,
            post_token_balances: None,
            rewards: None,
            loaded_addresses: LoadedAddresses::default(),
            return_data: None,
            compute_units_consumed: None,
        }
    }
}

/// A duplicate representation of TransactionStatusMeta with `err` field
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTransactionStatusMeta {
    pub err: Option<TransactionError>,
    pub status: TransactionResult<()>, // This field is deprecated.  See https://github.com/solana-labs/solana/issues/9302
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
    #[serde(
        default = "OptionSerializer::none",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub inner_instructions: OptionSerializer<Vec<UiInnerInstructions>>,
    #[serde(
        default = "OptionSerializer::none",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub log_messages: OptionSerializer<Vec<String>>,
    #[serde(
        default = "OptionSerializer::none",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub pre_token_balances: OptionSerializer<Vec<UiTransactionTokenBalance>>,
    #[serde(
        default = "OptionSerializer::none",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub post_token_balances: OptionSerializer<Vec<UiTransactionTokenBalance>>,
    #[serde(
        default = "OptionSerializer::none",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub rewards: OptionSerializer<Rewards>,
    #[serde(
        default = "OptionSerializer::skip",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub loaded_addresses: OptionSerializer<UiLoadedAddresses>,
    #[serde(
        default = "OptionSerializer::skip",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub return_data: OptionSerializer<UiTransactionReturnData>,
    #[serde(
        default = "OptionSerializer::skip",
        skip_serializing_if = "OptionSerializer::should_skip"
    )]
    pub compute_units_consumed: OptionSerializer<u64>,
}

/// A duplicate representation of LoadedAddresses
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiLoadedAddresses {
    pub writable: Vec<String>,
    pub readonly: Vec<String>,
}

impl From<&LoadedAddresses> for UiLoadedAddresses {
    fn from(loaded_addresses: &LoadedAddresses) -> Self {
        Self {
            writable: loaded_addresses
                .writable
                .iter()
                .map(ToString::to_string)
                .collect(),
            readonly: loaded_addresses
                .readonly
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }
}

impl UiTransactionStatusMeta {
    fn parse(meta: TransactionStatusMeta, static_keys: &[Pubkey], show_rewards: bool) -> Self {
        let account_keys = AccountKeys::new(static_keys, Some(&meta.loaded_addresses));
        Self {
            err: meta.status.clone().err(),
            status: meta.status,
            fee: meta.fee,
            pre_balances: meta.pre_balances,
            post_balances: meta.post_balances,
            inner_instructions: meta
                .inner_instructions
                .map(|ixs| {
                    ixs.into_iter()
                        .map(|ix| UiInnerInstructions::parse(ix, &account_keys))
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

    fn build_simple(meta: TransactionStatusMeta, show_rewards: bool) -> Self {
        Self {
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
}

impl From<TransactionStatusMeta> for UiTransactionStatusMeta {
    fn from(meta: TransactionStatusMeta) -> Self {
        Self {
            err: meta.status.clone().err(),
            status: meta.status,
            fee: meta.fee,
            pre_balances: meta.pre_balances,
            post_balances: meta.post_balances,
            inner_instructions: meta
                .inner_instructions
                .map(|ixs| ixs.into_iter().map(Into::into).collect())
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
            rewards: meta.rewards.into(),
            loaded_addresses: Some(UiLoadedAddresses::from(&meta.loaded_addresses)).into(),
            return_data: OptionSerializer::or_skip(
                meta.return_data.map(|return_data| return_data.into()),
            ),
            compute_units_consumed: OptionSerializer::or_skip(meta.compute_units_consumed),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionConfirmationStatus {
    Processed,
    Confirmed,
    Finalized,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatus {
    pub slot: Slot,
    pub confirmations: Option<usize>,  // None = rooted
    pub status: TransactionResult<()>, // legacy field
    pub err: Option<TransactionError>,
    pub confirmation_status: Option<TransactionConfirmationStatus>,
}

impl TransactionStatus {
    pub fn satisfies_commitment(&self, commitment_config: CommitmentConfig) -> bool {
        if commitment_config.is_finalized() {
            self.confirmations.is_none()
        } else if commitment_config.is_confirmed() {
            if let Some(status) = &self.confirmation_status {
                *status != TransactionConfirmationStatus::Processed
            } else {
                // These fallback cases handle TransactionStatus RPC responses from older software
                self.confirmations.is_some() && self.confirmations.unwrap() > 1
                    || self.confirmations.is_none()
            }
        } else {
            true
        }
    }

    // Returns `confirmation_status`, or if is_none, determines the status from confirmations.
    // Facilitates querying nodes on older software
    pub fn confirmation_status(&self) -> TransactionConfirmationStatus {
        match &self.confirmation_status {
            Some(status) => status.clone(),
            None => {
                if self.confirmations.is_none() {
                    TransactionConfirmationStatus::Finalized
                } else if self.confirmations.unwrap() > 0 {
                    TransactionConfirmationStatus::Confirmed
                } else {
                    TransactionConfirmationStatus::Processed
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedTransactionStatusWithSignature {
    pub signature: Signature,
    pub slot: Slot,
    pub err: Option<TransactionError>,
    pub memo: Option<String>,
    pub block_time: Option<UnixTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reward {
    pub pubkey: String,
    pub lamports: i64,
    pub post_balance: u64, // Account balance in lamports after `lamports` was applied
    pub reward_type: Option<RewardType>,
    pub commission: Option<u8>, // Vote account commission when the reward was credited, only present for voting and staking rewards
}

pub type Rewards = Vec<Reward>;

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
            block_time: self.block_time,
            block_height: self.block_height,
        })
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub transactions: Vec<EncodedTransactionWithStatusMeta>,
    pub rewards: Rewards,
    pub block_time: Option<UnixTimestamp>,
    pub block_height: Option<u64>,
}

impl From<UiConfirmedBlock> for EncodedConfirmedBlock {
    fn from(block: UiConfirmedBlock) -> Self {
        Self {
            previous_blockhash: block.previous_blockhash,
            blockhash: block.blockhash,
            parent_slot: block.parent_slot,
            transactions: block.transactions.unwrap_or_default(),
            rewards: block.rewards.unwrap_or_default(),
            block_time: block.block_time,
            block_height: block.block_height,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UiConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<EncodedTransactionWithStatusMeta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewards: Option<Rewards>,
    pub block_time: Option<UnixTimestamp>,
    pub block_height: Option<u64>,
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
                UiTransactionEncoding::JsonParsed => UiTransactionStatusMeta::parse(
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

        let account_keys = match &self.transaction.message {
            VersionedMessage::Legacy(message) => parse_legacy_message_accounts(message),
            VersionedMessage::V0(message) => {
                let loaded_message =
                    LoadedMessage::new_borrowed(message, &self.meta.loaded_addresses);
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
            meta: Some(UiTransactionStatusMeta::build_simple(
                self.meta,
                show_rewards,
            )),
            version,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedTransactionWithStatusMeta {
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<TransactionVersion>,
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedConfirmedTransactionWithStatusMeta {
    pub slot: Slot,
    #[serde(flatten)]
    pub transaction: EncodedTransactionWithStatusMeta,
    pub block_time: Option<UnixTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum EncodedTransaction {
    LegacyBinary(String), // Old way of expressing base-58, retained for RPC backwards compatibility
    Binary(String, TransactionBinaryEncoding),
    Json(UiTransaction),
    Accounts(UiAccountsList),
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

impl EncodedTransaction {
    pub fn decode(&self) -> Option<VersionedTransaction> {
        let (blob, encoding) = match self {
            Self::Json(_) | Self::Accounts(_) => return None,
            Self::LegacyBinary(blob) => (blob, TransactionBinaryEncoding::Base58),
            Self::Binary(blob, encoding) => (blob, *encoding),
        };

        let transaction: Option<VersionedTransaction> = match encoding {
            TransactionBinaryEncoding::Base58 => bs58::decode(blob)
                .into_vec()
                .ok()
                .and_then(|bytes| bincode::deserialize(&bytes).ok()),
            TransactionBinaryEncoding::Base64 => BASE64_STANDARD
                .decode(blob)
                .ok()
                .and_then(|bytes| bincode::deserialize(&bytes).ok()),
        };

        transaction.filter(|transaction| transaction.sanitize().is_ok())
    }
}

/// A duplicate representation of a Transaction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTransaction {
    pub signatures: Vec<String>,
    pub message: UiMessage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiMessage {
    Parsed(UiParsedMessage),
    Raw(UiRawMessage),
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
                    .map(|instruction| UiInstruction::parse(instruction, &account_keys, None))
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
            let account_keys = AccountKeys::new(&self.account_keys, Some(&meta.loaded_addresses));
            let loaded_message = LoadedMessage::new_borrowed(self, &meta.loaded_addresses);
            UiMessage::Parsed(UiParsedMessage {
                account_keys: parse_v0_message_accounts(&loaded_message),
                recent_blockhash: self.recent_blockhash.to_string(),
                instructions: self
                    .instructions
                    .iter()
                    .map(|instruction| UiInstruction::parse(instruction, &account_keys, None))
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

/// A duplicate representation of a Message, in raw format, for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiRawMessage {
    pub header: MessageHeader,
    pub account_keys: Vec<String>,
    pub recent_blockhash: String,
    pub instructions: Vec<UiCompiledInstruction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_table_lookups: Option<Vec<UiAddressTableLookup>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiAccountsList {
    pub signatures: Vec<String>,
    pub account_keys: Vec<ParsedAccount>,
}

/// A duplicate representation of a MessageAddressTableLookup, in raw format, for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiAddressTableLookup {
    pub account_key: String,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

impl From<&MessageAddressTableLookup> for UiAddressTableLookup {
    fn from(lookup: &MessageAddressTableLookup) -> Self {
        Self {
            account_key: lookup.account_key.to_string(),
            writable_indexes: lookup.writable_indexes.clone(),
            readonly_indexes: lookup.readonly_indexes.clone(),
        }
    }
}

/// A duplicate representation of a Message, in parsed format, for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiParsedMessage {
    pub account_keys: Vec<ParsedAccount>,
    pub recent_blockhash: String,
    pub instructions: Vec<UiInstruction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_table_lookups: Option<Vec<UiAddressTableLookup>>,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransactionReturnData {
    pub program_id: String,
    pub data: (String, UiReturnDataEncoding),
}

impl Default for UiTransactionReturnData {
    fn default() -> Self {
        Self {
            program_id: String::default(),
            data: (String::default(), UiReturnDataEncoding::Base64),
        }
    }
}

impl From<TransactionReturnData> for UiTransactionReturnData {
    fn from(return_data: TransactionReturnData) -> Self {
        Self {
            program_id: return_data.program_id.to_string(),
            data: (
                BASE64_STANDARD.encode(return_data.data),
                UiReturnDataEncoding::Base64,
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiReturnDataEncoding {
    Base64,
}

#[cfg(test)]
mod test {
    use {super::*, serde_json::json};

    #[test]
    fn test_decode_invalid_transaction() {
        // This transaction will not pass sanitization
        let unsanitary_transaction = EncodedTransaction::Binary(
            "ju9xZWuDBX4pRxX2oZkTjxU5jB4SSTgEGhX8bQ8PURNzyzqKMPPpNvWihx8zUe\
             FfrbVNoAaEsNKZvGzAnTDy5bhNT9kt6KFCTBixpvrLCzg4M5UdFUQYrn1gdgjX\
             pLHxcaShD81xBNaFDgnA2nkkdHnKtZt4hVSfKAmw3VRZbjrZ7L2fKZBx21CwsG\
             hD6onjM2M3qZW5C8J6d1pj41MxKmZgPBSha3MyKkNLkAGFASK"
                .to_string(),
            TransactionBinaryEncoding::Base58,
        );
        assert!(unsanitary_transaction.decode().is_none());
    }

    #[test]
    fn test_satisfies_commitment() {
        let status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status: Ok(()),
            err: None,
            confirmation_status: Some(TransactionConfirmationStatus::Finalized),
        };

        assert!(status.satisfies_commitment(CommitmentConfig::finalized()));
        assert!(status.satisfies_commitment(CommitmentConfig::confirmed()));
        assert!(status.satisfies_commitment(CommitmentConfig::processed()));

        let status = TransactionStatus {
            slot: 0,
            confirmations: Some(10),
            status: Ok(()),
            err: None,
            confirmation_status: Some(TransactionConfirmationStatus::Confirmed),
        };

        assert!(!status.satisfies_commitment(CommitmentConfig::finalized()));
        assert!(status.satisfies_commitment(CommitmentConfig::confirmed()));
        assert!(status.satisfies_commitment(CommitmentConfig::processed()));

        let status = TransactionStatus {
            slot: 0,
            confirmations: Some(1),
            status: Ok(()),
            err: None,
            confirmation_status: Some(TransactionConfirmationStatus::Processed),
        };

        assert!(!status.satisfies_commitment(CommitmentConfig::finalized()));
        assert!(!status.satisfies_commitment(CommitmentConfig::confirmed()));
        assert!(status.satisfies_commitment(CommitmentConfig::processed()));

        let status = TransactionStatus {
            slot: 0,
            confirmations: Some(0),
            status: Ok(()),
            err: None,
            confirmation_status: None,
        };

        assert!(!status.satisfies_commitment(CommitmentConfig::finalized()));
        assert!(!status.satisfies_commitment(CommitmentConfig::confirmed()));
        assert!(status.satisfies_commitment(CommitmentConfig::processed()));

        // Test single_gossip fallback cases
        let status = TransactionStatus {
            slot: 0,
            confirmations: Some(1),
            status: Ok(()),
            err: None,
            confirmation_status: None,
        };
        assert!(!status.satisfies_commitment(CommitmentConfig::confirmed()));

        let status = TransactionStatus {
            slot: 0,
            confirmations: Some(2),
            status: Ok(()),
            err: None,
            confirmation_status: None,
        };
        assert!(status.satisfies_commitment(CommitmentConfig::confirmed()));

        let status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status: Ok(()),
            err: None,
            confirmation_status: None,
        };
        assert!(status.satisfies_commitment(CommitmentConfig::confirmed()));
    }

    #[test]
    fn test_serde_empty_fields() {
        fn test_serde<'de, T: serde::Serialize + serde::Deserialize<'de>>(
            json_input: &'de str,
            expected_json_output: &str,
        ) {
            let typed_meta: T = serde_json::from_str(json_input).unwrap();
            let reserialized_value = json!(typed_meta);

            let expected_json_output_value: serde_json::Value =
                serde_json::from_str(expected_json_output).unwrap();
            assert_eq!(reserialized_value, expected_json_output_value);
        }

        let json_input = "{\
            \"err\":null,\
            \"status\":{\"Ok\":null},\
            \"fee\":1234,\
            \"preBalances\":[1,2,3],\
            \"postBalances\":[4,5,6]\
        }";
        let expected_json_output = "{\
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
        }";
        test_serde::<UiTransactionStatusMeta>(json_input, expected_json_output);

        let json_input = "{\
            \"accountIndex\":5,\
            \"mint\":\"DXM2yVSouSg1twmQgHLKoSReqXhtUroehWxrTgPmmfWi\",\
            \"uiTokenAmount\": {
                \"amount\": \"1\",\
                \"decimals\": 0,\
                \"uiAmount\": 1.0,\
                \"uiAmountString\": \"1\"\
            }\
        }";
        let expected_json_output = "{\
            \"accountIndex\":5,\
            \"mint\":\"DXM2yVSouSg1twmQgHLKoSReqXhtUroehWxrTgPmmfWi\",\
            \"uiTokenAmount\": {
                \"amount\": \"1\",\
                \"decimals\": 0,\
                \"uiAmount\": 1.0,\
                \"uiAmountString\": \"1\"\
            }\
        }";
        test_serde::<UiTransactionTokenBalance>(json_input, expected_json_output);
    }

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
        let ui_meta_parse_with_rewards = UiTransactionStatusMeta::parse(meta.clone(), &[], true);
        assert_eq!(
            serde_json::to_value(ui_meta_parse_with_rewards).unwrap(),
            expected_json_output_value
        );

        let ui_meta_parse_no_rewards = UiTransactionStatusMeta::parse(meta, &[], false);
        assert_eq!(
            serde_json::to_value(ui_meta_parse_no_rewards).unwrap(),
            expected_json_output_value
        );
    }
}
