#![allow(clippy::integer_arithmetic)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod extract_memos;
pub mod parse_accounts;
pub mod parse_associated_token;
pub mod parse_bpf_loader;
pub mod parse_instruction;
pub mod parse_stake;
pub mod parse_system;
pub mod parse_token;
pub mod parse_vote;
pub mod token_balances;

pub use crate::extract_memos::extract_and_fmt_memos;
use crate::{
    parse_accounts::{parse_accounts, ParsedAccount},
    parse_instruction::{parse, ParsedInstruction},
};
use solana_account_decoder::parse_token::UiTokenAmount;
pub use solana_runtime::bank::RewardType;
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    commitment_config::CommitmentConfig,
    deserialize_utils::default_on_eof,
    instruction::CompiledInstruction,
    message::{Message, MessageHeader},
    pubkey::Pubkey,
    sanitize::Sanitize,
    signature::Signature,
    transaction::{Result, Transaction, TransactionError},
};
use std::fmt;
/// A duplicate representation of an Instruction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiInstruction {
    Compiled(UiCompiledInstruction),
    Parsed(UiParsedInstruction),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiParsedInstruction {
    Parsed(ParsedInstruction),
    PartiallyDecoded(UiPartiallyDecodedInstruction),
}

impl UiInstruction {
    fn parse(instruction: &CompiledInstruction, message: &Message) -> Self {
        let program_id = instruction.program_id(&message.account_keys);
        if let Ok(parsed_instruction) = parse(program_id, instruction, &message.account_keys) {
            UiInstruction::Parsed(UiParsedInstruction::Parsed(parsed_instruction))
        } else {
            UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(
                UiPartiallyDecodedInstruction::from(instruction, &message.account_keys),
            ))
        }
    }
}

/// A duplicate representation of a CompiledInstruction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiCompiledInstruction {
    pub program_id_index: u8,
    pub accounts: Vec<u8>,
    pub data: String,
}

impl From<&CompiledInstruction> for UiCompiledInstruction {
    fn from(instruction: &CompiledInstruction) -> Self {
        Self {
            program_id_index: instruction.program_id_index,
            accounts: instruction.accounts.clone(),
            data: bs58::encode(instruction.data.clone()).into_string(),
        }
    }
}

/// A partially decoded CompiledInstruction that includes explicit account addresses
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPartiallyDecodedInstruction {
    pub program_id: String,
    pub accounts: Vec<String>,
    pub data: String,
}

impl UiPartiallyDecodedInstruction {
    fn from(instruction: &CompiledInstruction, account_keys: &[Pubkey]) -> Self {
        Self {
            program_id: account_keys[instruction.program_id_index as usize].to_string(),
            accounts: instruction
                .accounts
                .iter()
                .map(|&i| account_keys[i as usize].to_string())
                .collect(),
            data: bs58::encode(instruction.data.clone()).into_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InnerInstructions {
    /// Transaction instruction index
    pub index: u8,
    /// List of inner instructions
    pub instructions: Vec<CompiledInstruction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiInnerInstructions {
    /// Transaction instruction index
    pub index: u8,
    /// List of inner instructions
    pub instructions: Vec<UiInstruction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransactionTokenBalance {
    pub account_index: u8,
    pub mint: String,
    pub ui_token_amount: UiTokenAmount,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTransactionTokenBalance {
    pub account_index: u8,
    pub mint: String,
    pub ui_token_amount: UiTokenAmount,
}

impl From<TransactionTokenBalance> for UiTransactionTokenBalance {
    fn from(token_balance: TransactionTokenBalance) -> Self {
        Self {
            account_index: token_balance.account_index,
            mint: token_balance.mint,
            ui_token_amount: token_balance.ui_token_amount,
        }
    }
}

impl UiInnerInstructions {
    fn parse(inner_instructions: InnerInstructions, message: &Message) -> Self {
        Self {
            index: inner_instructions.index,
            instructions: inner_instructions
                .instructions
                .iter()
                .map(|ix| UiInstruction::parse(ix, message))
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
                .map(|ix| UiInstruction::Compiled(ix.into()))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusMeta {
    pub status: Result<()>,
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
    #[serde(deserialize_with = "default_on_eof")]
    pub inner_instructions: Option<Vec<InnerInstructions>>,
    #[serde(deserialize_with = "default_on_eof")]
    pub log_messages: Option<Vec<String>>,
    #[serde(deserialize_with = "default_on_eof")]
    pub pre_token_balances: Option<Vec<TransactionTokenBalance>>,
    #[serde(deserialize_with = "default_on_eof")]
    pub post_token_balances: Option<Vec<TransactionTokenBalance>>,
    #[serde(deserialize_with = "default_on_eof")]
    pub rewards: Option<Rewards>,
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
        }
    }
}

/// A duplicate representation of TransactionStatusMeta with `err` field
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTransactionStatusMeta {
    pub err: Option<TransactionError>,
    pub status: Result<()>, // This field is deprecated.  See https://github.com/solana-labs/solana/issues/9302
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
    pub inner_instructions: Option<Vec<UiInnerInstructions>>,
    pub log_messages: Option<Vec<String>>,
    pub pre_token_balances: Option<Vec<UiTransactionTokenBalance>>,
    pub post_token_balances: Option<Vec<UiTransactionTokenBalance>>,
    pub rewards: Option<Rewards>,
}

impl UiTransactionStatusMeta {
    fn parse(meta: TransactionStatusMeta, message: &Message) -> Self {
        Self {
            err: meta.status.clone().err(),
            status: meta.status,
            fee: meta.fee,
            pre_balances: meta.pre_balances,
            post_balances: meta.post_balances,
            inner_instructions: meta.inner_instructions.map(|ixs| {
                ixs.into_iter()
                    .map(|ix| UiInnerInstructions::parse(ix, message))
                    .collect()
            }),
            log_messages: meta.log_messages,
            pre_token_balances: meta
                .pre_token_balances
                .map(|balance| balance.into_iter().map(|balance| balance.into()).collect()),
            post_token_balances: meta
                .post_token_balances
                .map(|balance| balance.into_iter().map(|balance| balance.into()).collect()),
            rewards: meta.rewards,
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
                .map(|ixs| ixs.into_iter().map(|ix| ix.into()).collect()),
            log_messages: meta.log_messages,
            pre_token_balances: meta
                .pre_token_balances
                .map(|balance| balance.into_iter().map(|balance| balance.into()).collect()),
            post_token_balances: meta
                .post_token_balances
                .map(|balance| balance.into_iter().map(|balance| balance.into()).collect()),
            rewards: meta.rewards,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionConfirmationStatus {
    Processed,
    Confirmed,
    Finalized,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatus {
    pub slot: Slot,
    pub confirmations: Option<usize>, // None = rooted
    pub status: Result<()>,           // legacy field
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTransactionStatusWithSignature {
    pub signature: Signature,
    pub slot: Slot,
    pub err: Option<TransactionError>,
    pub memo: Option<String>,
    pub block_time: Option<UnixTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reward {
    pub pubkey: String,
    pub lamports: i64,
    pub post_balance: u64, // Account balance in lamports after `lamports` was applied
    pub reward_type: Option<RewardType>,
    pub commission: Option<u8>, // Vote account commission when the reward was credited, only present for voting and staking rewards
}

pub type Rewards = Vec<Reward>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub transactions: Vec<TransactionWithStatusMeta>,
    pub rewards: Rewards,
    pub block_time: Option<UnixTimestamp>,
    pub block_height: Option<u64>,
}

impl ConfirmedBlock {
    pub fn encode(self, encoding: UiTransactionEncoding) -> EncodedConfirmedBlock {
        EncodedConfirmedBlock {
            previous_blockhash: self.previous_blockhash,
            blockhash: self.blockhash,
            parent_slot: self.parent_slot,
            transactions: self
                .transactions
                .into_iter()
                .map(|tx| tx.encode(encoding))
                .collect(),
            rewards: self.rewards,
            block_time: self.block_time,
            block_height: self.block_height,
        }
    }

    pub fn configure(
        self,
        encoding: UiTransactionEncoding,
        transaction_details: TransactionDetails,
        show_rewards: bool,
    ) -> UiConfirmedBlock {
        let (transactions, signatures) = match transaction_details {
            TransactionDetails::Full => (
                Some(
                    self.transactions
                        .into_iter()
                        .map(|tx| tx.encode(encoding))
                        .collect(),
                ),
                None,
            ),
            TransactionDetails::Signatures => (
                None,
                Some(
                    self.transactions
                        .into_iter()
                        .map(|tx| tx.transaction.signatures[0].to_string())
                        .collect(),
                ),
            ),
            TransactionDetails::None => (None, None),
        };
        UiConfirmedBlock {
            previous_blockhash: self.previous_blockhash,
            blockhash: self.blockhash,
            parent_slot: self.parent_slot,
            transactions,
            signatures,
            rewards: if show_rewards {
                Some(self.rewards)
            } else {
                None
            },
            block_time: self.block_time,
            block_height: self.block_height,
        }
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
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

impl From<EncodedConfirmedBlock> for UiConfirmedBlock {
    fn from(block: EncodedConfirmedBlock) -> Self {
        Self {
            previous_blockhash: block.previous_blockhash,
            blockhash: block.blockhash,
            parent_slot: block.parent_slot,
            transactions: Some(block.transactions),
            signatures: None,
            rewards: Some(block.rewards),
            block_time: block.block_time,
            block_height: block.block_height,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionDetails {
    Full,
    Signatures,
    None,
}

impl Default for TransactionDetails {
    fn default() -> Self {
        Self::Full
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTransaction {
    pub slot: Slot,
    #[serde(flatten)]
    pub transaction: TransactionWithStatusMeta,
    pub block_time: Option<UnixTimestamp>,
}

impl ConfirmedTransaction {
    pub fn encode(self, encoding: UiTransactionEncoding) -> EncodedConfirmedTransaction {
        EncodedConfirmedTransaction {
            slot: self.slot,
            transaction: self.transaction.encode(encoding),
            block_time: self.block_time,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedConfirmedTransaction {
    pub slot: Slot,
    #[serde(flatten)]
    pub transaction: EncodedTransactionWithStatusMeta,
    pub block_time: Option<UnixTimestamp>,
}

/// A duplicate representation of a Transaction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTransaction {
    pub signatures: Vec<String>,
    pub message: UiMessage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiMessage {
    Parsed(UiParsedMessage),
    Raw(UiRawMessage),
}

/// A duplicate representation of a Message, in raw format, for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiRawMessage {
    pub header: MessageHeader,
    pub account_keys: Vec<String>,
    pub recent_blockhash: String,
    pub instructions: Vec<UiCompiledInstruction>,
}

/// A duplicate representation of a Message, in parsed format, for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiParsedMessage {
    pub account_keys: Vec<ParsedAccount>,
    pub recent_blockhash: String,
    pub instructions: Vec<UiInstruction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionWithStatusMeta {
    pub transaction: Transaction,
    pub meta: Option<TransactionStatusMeta>,
}

impl TransactionWithStatusMeta {
    fn encode(self, encoding: UiTransactionEncoding) -> EncodedTransactionWithStatusMeta {
        let message = self.transaction.message();
        let meta = self.meta.map(|meta| meta.encode(encoding, message));
        EncodedTransactionWithStatusMeta {
            transaction: EncodedTransaction::encode(self.transaction, encoding),
            meta,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedTransactionWithStatusMeta {
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
}

impl TransactionStatusMeta {
    fn encode(self, encoding: UiTransactionEncoding, message: &Message) -> UiTransactionStatusMeta {
        match encoding {
            UiTransactionEncoding::JsonParsed => UiTransactionStatusMeta::parse(self, message),
            _ => self.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiTransactionEncoding {
    Binary, // Legacy. Retained for RPC backwards compatibility
    Base64,
    Base58,
    Json,
    JsonParsed,
}

impl fmt::Display for UiTransactionEncoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let v = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        let s = v.as_str().ok_or(fmt::Error)?;
        write!(f, "{}", s)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum EncodedTransaction {
    LegacyBinary(String), // Old way of expressing base-58, retained for RPC backwards compatibility
    Binary(String, UiTransactionEncoding),
    Json(UiTransaction),
}

impl EncodedTransaction {
    pub fn encode(transaction: Transaction, encoding: UiTransactionEncoding) -> Self {
        match encoding {
            UiTransactionEncoding::Binary => EncodedTransaction::LegacyBinary(
                bs58::encode(bincode::serialize(&transaction).unwrap()).into_string(),
            ),
            UiTransactionEncoding::Base58 => EncodedTransaction::Binary(
                bs58::encode(bincode::serialize(&transaction).unwrap()).into_string(),
                encoding,
            ),
            UiTransactionEncoding::Base64 => EncodedTransaction::Binary(
                base64::encode(bincode::serialize(&transaction).unwrap()),
                encoding,
            ),
            UiTransactionEncoding::Json | UiTransactionEncoding::JsonParsed => {
                let message = if encoding == UiTransactionEncoding::Json {
                    UiMessage::Raw(UiRawMessage {
                        header: transaction.message.header,
                        account_keys: transaction
                            .message
                            .account_keys
                            .iter()
                            .map(|pubkey| pubkey.to_string())
                            .collect(),
                        recent_blockhash: transaction.message.recent_blockhash.to_string(),
                        instructions: transaction
                            .message
                            .instructions
                            .iter()
                            .map(|instruction| instruction.into())
                            .collect(),
                    })
                } else {
                    UiMessage::Parsed(UiParsedMessage {
                        account_keys: parse_accounts(&transaction.message),
                        recent_blockhash: transaction.message.recent_blockhash.to_string(),
                        instructions: transaction
                            .message
                            .instructions
                            .iter()
                            .map(|instruction| {
                                UiInstruction::parse(instruction, &transaction.message)
                            })
                            .collect(),
                    })
                };
                EncodedTransaction::Json(UiTransaction {
                    signatures: transaction
                        .signatures
                        .iter()
                        .map(|sig| sig.to_string())
                        .collect(),
                    message,
                })
            }
        }
    }
    pub fn decode(&self) -> Option<Transaction> {
        let transaction: Option<Transaction> = match self {
            EncodedTransaction::Json(_) => None,
            EncodedTransaction::LegacyBinary(blob) => bs58::decode(blob)
                .into_vec()
                .ok()
                .and_then(|bytes| bincode::deserialize(&bytes).ok()),
            EncodedTransaction::Binary(blob, encoding) => match *encoding {
                UiTransactionEncoding::Base58 => bs58::decode(blob)
                    .into_vec()
                    .ok()
                    .and_then(|bytes| bincode::deserialize(&bytes).ok()),
                UiTransactionEncoding::Base64 => base64::decode(blob)
                    .ok()
                    .and_then(|bytes| bincode::deserialize(&bytes).ok()),
                UiTransactionEncoding::Binary
                | UiTransactionEncoding::Json
                | UiTransactionEncoding::JsonParsed => None,
            },
        };
        transaction.filter(|transaction| transaction.sanitize().is_ok())
    }
}

// A serialized `Vec<TransactionByAddrInfo>` is stored in the `tx-by-addr` table.  The row keys are
// the one's compliment of the slot so that rows may be listed in reverse order
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    fn test_decode_invalid_transaction() {
        // This transaction will not pass sanitization
        let unsanitary_transaction = EncodedTransaction::Binary(
            "ju9xZWuDBX4pRxX2oZkTjxU5jB4SSTgEGhX8bQ8PURNzyzqKMPPpNvWihx8zUe\
             FfrbVNoAaEsNKZvGzAnTDy5bhNT9kt6KFCTBixpvrLCzg4M5UdFUQYrn1gdgjX\
             pLHxcaShD81xBNaFDgnA2nkkdHnKtZt4hVSfKAmw3VRZbjrZ7L2fKZBx21CwsG\
             hD6onjM2M3qZW5C8J6d1pj41MxKmZgPBSha3MyKkNLkAGFASK"
                .to_string(),
            UiTransactionEncoding::Base58,
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
}
