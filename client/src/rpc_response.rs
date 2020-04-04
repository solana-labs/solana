use crate::{client_error, rpc_request::RpcError};
use solana_sdk::{
    account::Account,
    clock::{Epoch, Slot},
    fee_calculator::{FeeCalculator, FeeRateGovernor},
    pubkey::Pubkey,
    transaction::{Result, TransactionError},
};
use std::{collections::HashMap, net::SocketAddr, str::FromStr};

pub type RpcResult<T> = client_error::Result<Response<T>>;

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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlockhashFeeCalculator {
    pub blockhash: String,
    pub fee_calculator: FeeCalculator,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcFeeCalculator {
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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcSignatureResult {
    pub err: Option<TransactionError>,
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
#[serde(rename_all = "kebab-case")]
pub struct RpcIdentity {
    /// The current node identity pubkey
    pub identity: String,
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
