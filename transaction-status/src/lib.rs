#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod parse_accounts;
pub mod parse_instruction;

use crate::{parse_accounts::parse_accounts, parse_instruction::parse};
use serde_json::Value;
use solana_sdk::{
    clock::Slot,
    commitment_config::CommitmentConfig,
    instruction::CompiledInstruction,
    message::MessageHeader,
    transaction::{Result, Transaction, TransactionError},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum RpcInstruction {
    Compiled(RpcCompiledInstruction),
    Parsed(Value),
}

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcCompiledInstruction {
    pub program_id_index: u8,
    pub accounts: Vec<u8>,
    pub data: String,
}

impl From<&CompiledInstruction> for RpcCompiledInstruction {
    fn from(instruction: &CompiledInstruction) -> Self {
        Self {
            program_id_index: instruction.program_id_index,
            accounts: instruction.accounts.clone(),
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionStatusMeta {
    pub err: Option<TransactionError>,
    pub status: Result<()>, // This field is deprecated.  See https://github.com/solana-labs/solana/issues/9302
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
}

impl From<TransactionStatusMeta> for RpcTransactionStatusMeta {
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
pub struct RpcTransaction {
    pub signatures: Vec<String>,
    pub message: RpcMessage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum RpcMessage {
    Parsed(RpcParsedMessage),
    Raw(RpcRawMessage),
}

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcRawMessage {
    pub header: MessageHeader,
    pub account_keys: Vec<String>,
    pub recent_blockhash: String,
    pub instructions: Vec<RpcCompiledInstruction>,
}

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcParsedMessage {
    pub account_keys: Value,
    pub recent_blockhash: String,
    pub instructions: Vec<RpcInstruction>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionWithStatusMeta {
    pub transaction: EncodedTransaction,
    pub meta: Option<RpcTransactionStatusMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransactionEncoding {
    Binary,
    Json,
    JsonParsed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum EncodedTransaction {
    Binary(String),
    Json(RpcTransaction),
}

impl EncodedTransaction {
    pub fn encode(transaction: Transaction, encoding: TransactionEncoding) -> Self {
        match encoding {
            TransactionEncoding::Binary => EncodedTransaction::Binary(
                bs58::encode(bincode::serialize(&transaction).unwrap()).into_string(),
            ),
            _ => {
                let message = if encoding == TransactionEncoding::Json {
                    RpcMessage::Raw(RpcRawMessage {
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
                    RpcMessage::Parsed(RpcParsedMessage {
                        account_keys: parse_accounts(&transaction.message),
                        recent_blockhash: transaction.message.recent_blockhash.to_string(),
                        instructions: transaction
                            .message
                            .instructions
                            .iter()
                            .map(|instruction| {
                                let program_id =
                                    instruction.program_id(&transaction.message.account_keys);
                                if let Some(parsed_instruction) = parse(program_id, instruction) {
                                    RpcInstruction::Parsed(parsed_instruction)
                                } else {
                                    RpcInstruction::Compiled(instruction.into())
                                }
                            })
                            .collect(),
                    })
                };
                EncodedTransaction::Json(RpcTransaction {
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
