#[macro_use]
extern crate serde_derive;

use bincode::serialize;
use solana_sdk::{
    clock::Slot,
    message::MessageHeader,
    transaction::{Result, Transaction},
};

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcCompiledInstruction {
    pub program_id_index: u8,
    pub accounts: Vec<u8>,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionStatusMeta {
    pub status: Result<()>,
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionStatus {
    pub slot: Slot,
    pub status: Result<()>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcReward {
    pub pubkey: String,
    pub lamports: i64,
}

pub type RpcRewards = Vec<RpcReward>;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcConfirmedBlock {
    pub previous_blockhash: String,
    pub blockhash: String,
    pub parent_slot: Slot,
    pub transactions: Vec<RpcTransactionWithStatusMeta>,
    pub rewards: RpcRewards,
}

/// A duplicate representation of a Transaction for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransaction {
    pub signatures: Vec<String>,
    pub message: RpcMessage,
}

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcMessage {
    pub header: MessageHeader,
    pub account_keys: Vec<String>,
    pub recent_blockhash: String,
    pub instructions: Vec<RpcCompiledInstruction>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionWithStatusMeta {
    pub transaction: RpcEncodedTransaction,
    pub meta: Option<RpcTransactionStatusMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RpcTransactionEncoding {
    Binary,
    Json,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum RpcEncodedTransaction {
    Binary(String),
    Json(RpcTransaction),
}

impl RpcEncodedTransaction {
    pub fn encode(transaction: Transaction, encoding: RpcTransactionEncoding) -> Self {
        if encoding == RpcTransactionEncoding::Json {
            RpcEncodedTransaction::Json(RpcTransaction {
                signatures: transaction
                    .signatures
                    .iter()
                    .map(|sig| sig.to_string())
                    .collect(),
                message: RpcMessage {
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
                        .map(|instruction| RpcCompiledInstruction {
                            program_id_index: instruction.program_id_index,
                            accounts: instruction.accounts.clone(),
                            data: bs58::encode(instruction.data.clone()).into_string(),
                        })
                        .collect(),
                },
            })
        } else {
            RpcEncodedTransaction::Binary(
                bs58::encode(serialize(&transaction).unwrap()).into_string(),
            )
        }
    }
}
