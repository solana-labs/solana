#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod parse_accounts;
pub mod parse_instruction;
pub mod parse_token;

use crate::{
    parse_accounts::{parse_accounts, ParsedAccount},
    parse_instruction::{parse, ParsedInstruction},
};
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    commitment_config::CommitmentConfig,
    instruction::CompiledInstruction,
    message::MessageHeader,
    pubkey::Pubkey,
    transaction::{Result, Transaction, TransactionError},
};

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
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusMeta {
    pub status: Result<()>,
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
}

impl Default for TransactionStatusMeta {
    fn default() -> Self {
        Self {
            status: Ok(()),
            fee: 0,
            pre_balances: vec![],
            post_balances: vec![],
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
}

impl From<TransactionStatusMeta> for UiTransactionStatusMeta {
    fn from(meta: TransactionStatusMeta) -> Self {
        Self {
            err: meta.status.clone().err(),
            status: meta.status,
            fee: meta.fee,
            pre_balances: meta.pre_balances,
            post_balances: meta.post_balances,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatus {
    pub slot: Slot,
    pub confirmations: Option<usize>, // None = rooted
    pub status: Result<()>,
    pub err: Option<TransactionError>,
}

impl TransactionStatus {
    pub fn satisfies_commitment(&self, commitment_config: CommitmentConfig) -> bool {
        (commitment_config == CommitmentConfig::default() && self.confirmations.is_none())
            || commitment_config == CommitmentConfig::recent()
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Reward {
    pub pubkey: String,
    pub lamports: i64,
}

pub type Rewards = Vec<Reward>;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub transactions: Vec<TransactionWithStatusMeta>,
    pub rewards: Rewards,
    pub block_time: Option<UnixTimestamp>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedTransaction {
    pub slot: Slot,
    #[serde(flatten)]
    pub transaction: TransactionWithStatusMeta,
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionWithStatusMeta {
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiTransactionEncoding {
    Binary,
    Json,
    JsonParsed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum EncodedTransaction {
    Binary(String),
    Json(UiTransaction),
}

impl EncodedTransaction {
    pub fn encode(transaction: Transaction, encoding: UiTransactionEncoding) -> Self {
        match encoding {
            UiTransactionEncoding::Binary => EncodedTransaction::Binary(
                bs58::encode(bincode::serialize(&transaction).unwrap()).into_string(),
            ),
            _ => {
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
                                let program_id =
                                    instruction.program_id(&transaction.message.account_keys);
                                if let Ok(parsed_instruction) = parse(
                                    program_id,
                                    instruction,
                                    &transaction.message.account_keys,
                                ) {
                                    UiInstruction::Parsed(UiParsedInstruction::Parsed(
                                        parsed_instruction,
                                    ))
                                } else {
                                    UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(
                                        UiPartiallyDecodedInstruction::from(
                                            instruction,
                                            &transaction.message.account_keys,
                                        ),
                                    ))
                                }
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
        match self {
            EncodedTransaction::Json(_) => None,
            EncodedTransaction::Binary(blob) => bs58::decode(blob)
                .into_vec()
                .ok()
                .and_then(|bytes| bincode::deserialize(&bytes).ok()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_satisfies_commitment() {
        let status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status: Ok(()),
            err: None,
        };

        assert!(status.satisfies_commitment(CommitmentConfig::default()));
        assert!(status.satisfies_commitment(CommitmentConfig::recent()));

        let status = TransactionStatus {
            slot: 0,
            confirmations: Some(10),
            status: Ok(()),
            err: None,
        };

        assert!(!status.satisfies_commitment(CommitmentConfig::default()));
        assert!(status.satisfies_commitment(CommitmentConfig::recent()));
    }
}
