use crate::rpc_request::RpcError;
use bincode::serialize;
use jsonrpc_core::Result as JsonResult;
use solana_sdk::{
    account::Account,
    clock::{Epoch, Slot},
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    message::MessageHeader,
    pubkey::Pubkey,
    transaction::{Result, Transaction},
};
use std::{collections::HashMap, io, net::SocketAddr, str::FromStr};

pub type RpcResponseIn<T> = JsonResult<Response<T>>;
pub type RpcResponse<T> = io::Result<Response<T>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcResponseContext {
    pub slot: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response<T> {
    pub context: RpcResponseContext,
    pub value: T,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlockCommitment<T> {
    pub commitment: Option<T>,
    pub total_stake: u64,
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionWithStatusMeta {
    pub transaction: RpcEncodedTransaction,
    pub meta: Option<RpcTransactionStatus>,
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
pub struct RpcTransactionStatus {
    pub status: Result<()>,
    pub fee: u64,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlockhashFeeCalculator {
    pub blockhash: String,
    pub fee_calculator: FeeCalculator,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcFeeRateGovernor {
    pub fee_rate_governor: FeeRateGovernor,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcKeyedAccount {
    pub pubkey: String,
    pub account: RpcAccount,
}

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcAccount {
    pub lamports: u64,
    pub data: String,
    pub owner: String,
    pub executable: bool,
    pub rent_epoch: Epoch,
}

impl RpcAccount {
    pub fn encode(account: Account) -> Self {
        RpcAccount {
            lamports: account.lamports,
            data: bs58::encode(account.data.clone()).into_string(),
            owner: account.owner.to_string(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }

    pub fn decode(&self) -> std::result::Result<Account, RpcError> {
        Ok(Account {
            lamports: self.lamports,
            data: bs58::decode(self.data.clone()).into_vec().map_err(|_| {
                RpcError::RpcRequestError("Could not parse encoded account data".to_string())
            })?,
            owner: Pubkey::from_str(&self.owner).map_err(|_| {
                RpcError::RpcRequestError("Could not parse encoded account owner".to_string())
            })?,
            executable: self.executable,
            rent_epoch: self.rent_epoch,
            ..Account::default()
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RpcContactInfo {
    /// Pubkey of the node as a base-58 string
    pub pubkey: String,
    /// Gossip port
    pub gossip: Option<SocketAddr>,
    /// Tpu port
    pub tpu: Option<SocketAddr>,
    /// JSON RPC port
    pub rpc: Option<SocketAddr>,
}

/// Map of leader base58 identity pubkeys to the slot indices relative to the first epoch slot
pub type RpcLeaderSchedule = HashMap<String, Vec<usize>>;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcEpochInfo {
    /// The current epoch
    pub epoch: Epoch,

    /// The current slot, relative to the start of the current epoch
    pub slot_index: u64,

    /// The number of slots in this epoch
    pub slots_in_epoch: u64,

    /// The absolute current slot
    pub absolute_slot: Slot,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct RpcVersionInfo {
    /// The current version of solana-core
    pub solana_core: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcVoteAccountStatus {
    pub current: Vec<RpcVoteAccountInfo>,
    pub delinquent: Vec<RpcVoteAccountInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcVoteAccountInfo {
    /// Vote account pubkey as base-58 encoded string
    pub vote_pubkey: String,

    /// The pubkey of the node that votes using this account
    pub node_pubkey: String,

    /// The current stake, in lamports, delegated to this vote account
    pub activated_stake: u64,

    /// An 8-bit integer used as a fraction (commission/MAX_U8) for rewards payout
    pub commission: u8,

    /// Whether this account is staked for the current epoch
    pub epoch_vote_account: bool,

    /// History of how many credits earned by the end of each epoch
    ///   each tuple is (Epoch, credits, prev_credits)
    pub epoch_credits: Vec<(Epoch, u64, u64)>,

    /// Most recent slot voted on by this vote account (0 if no votes exist)
    pub last_vote: u64,

    /// Current root slot for this vote account (0 if not root slot exists)
    pub root_slot: Slot,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcSignatureConfirmation {
    pub confirmations: usize,
    pub status: Result<()>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcStorageTurn {
    pub blockhash: String,
    pub slot: Slot,
}
