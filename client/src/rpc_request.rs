use jsonrpc_core::Result as JsonResult;
use serde_json::{json, Value};
use solana_sdk::{
    clock::{Epoch, Slot},
    commitment_config::CommitmentConfig,
    hash::Hash,
    transaction::{Result, Transaction},
};
use std::{error, fmt, io, net::SocketAddr};

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

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcConfirmedBlock {
    pub previous_blockhash: Hash,
    pub blockhash: Hash,
    pub parent_slot: Slot,
    pub transactions: Vec<(Transaction, Option<RpcTransactionStatus>)>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcTransactionStatus {
    pub status: Result<()>,
    pub fee: u64,
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

    /// Most recent slot voted on by this vote account (0 if no votes exist)
    pub last_vote: u64,

    /// Current root slot for this vote account (0 if not root slot exists)
    pub root_slot: Slot,
}

#[derive(Debug, PartialEq)]
pub enum RpcRequest {
    ConfirmTransaction,
    DeregisterNode,
    ValidatorExit,
    GetAccountInfo,
    GetBalance,
    GetClusterNodes,
    GetEpochInfo,
    GetEpochSchedule,
    GetGenesisHash,
    GetInflation,
    GetNumBlocksSinceSignatureConfirmation,
    GetProgramAccounts,
    GetRecentBlockhash,
    GetSignatureStatus,
    GetSlot,
    GetSlotLeader,
    GetStorageTurn,
    GetStorageTurnRate,
    GetSlotsPerSegment,
    GetStoragePubkeysForSlot,
    GetTransactionCount,
    GetVersion,
    GetVoteAccounts,
    RegisterNode,
    RequestAirdrop,
    SendTransaction,
    SignVote,
    GetMinimumBalanceForRentExemption,
}

impl RpcRequest {
    pub(crate) fn build_request_json(
        &self,
        id: u64,
        params: Option<Value>,
        commitment_config: Option<CommitmentConfig>,
    ) -> Value {
        let jsonrpc = "2.0";
        let method = match self {
            RpcRequest::ConfirmTransaction => "confirmTransaction",
            RpcRequest::DeregisterNode => "deregisterNode",
            RpcRequest::ValidatorExit => "validatorExit",
            RpcRequest::GetAccountInfo => "getAccountInfo",
            RpcRequest::GetBalance => "getBalance",
            RpcRequest::GetClusterNodes => "getClusterNodes",
            RpcRequest::GetEpochInfo => "getEpochInfo",
            RpcRequest::GetEpochSchedule => "getEpochSchedule",
            RpcRequest::GetGenesisHash => "getGenesisHash",
            RpcRequest::GetInflation => "getInflation",
            RpcRequest::GetNumBlocksSinceSignatureConfirmation => {
                "getNumBlocksSinceSignatureConfirmation"
            }
            RpcRequest::GetProgramAccounts => "getProgramAccounts",
            RpcRequest::GetRecentBlockhash => "getRecentBlockhash",
            RpcRequest::GetSignatureStatus => "getSignatureStatus",
            RpcRequest::GetSlot => "getSlot",
            RpcRequest::GetSlotLeader => "getSlotLeader",
            RpcRequest::GetStorageTurn => "getStorageTurn",
            RpcRequest::GetStorageTurnRate => "getStorageTurnRate",
            RpcRequest::GetSlotsPerSegment => "getSlotsPerSegment",
            RpcRequest::GetStoragePubkeysForSlot => "getStoragePubkeysForSlot",
            RpcRequest::GetTransactionCount => "getTransactionCount",
            RpcRequest::GetVersion => "getVersion",
            RpcRequest::GetVoteAccounts => "getVoteAccounts",
            RpcRequest::RegisterNode => "registerNode",
            RpcRequest::RequestAirdrop => "requestAirdrop",
            RpcRequest::SendTransaction => "sendTransaction",
            RpcRequest::SignVote => "signVote",
            RpcRequest::GetMinimumBalanceForRentExemption => "getMinimumBalanceForRentExemption",
        };
        let mut request = json!({
           "jsonrpc": jsonrpc,
           "id": id,
           "method": method,
        });
        if let Some(param_string) = params {
            if let Some(config) = commitment_config {
                request["params"] = json!([param_string, config]);
            } else {
                request["params"] = json!([param_string]);
            }
        } else if let Some(config) = commitment_config {
            request["params"] = json!([config]);
        }
        request
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RpcError {
    RpcRequestError(String),
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for RpcError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::commitment_config::CommitmentLevel;

    #[test]
    fn test_build_request_json() {
        let test_request = RpcRequest::GetAccountInfo;
        let addr = json!("deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx");
        let request = test_request.build_request_json(1, Some(addr.clone()), None);
        assert_eq!(request["method"], "getAccountInfo");
        assert_eq!(request["params"], json!([addr]));

        let test_request = RpcRequest::GetBalance;
        let request = test_request.build_request_json(1, Some(addr), None);
        assert_eq!(request["method"], "getBalance");

        let test_request = RpcRequest::GetEpochInfo;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "getEpochInfo");

        let test_request = RpcRequest::GetInflation;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "getInflation");

        let test_request = RpcRequest::GetRecentBlockhash;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "getRecentBlockhash");

        let test_request = RpcRequest::GetSlot;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "getSlot");

        let test_request = RpcRequest::GetTransactionCount;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "getTransactionCount");

        let test_request = RpcRequest::RequestAirdrop;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "requestAirdrop");

        let test_request = RpcRequest::SendTransaction;
        let request = test_request.build_request_json(1, None, None);
        assert_eq!(request["method"], "sendTransaction");
    }

    #[test]
    fn test_build_request_json_config_options() {
        let commitment_config = CommitmentConfig {
            commitment: CommitmentLevel::Max,
        };
        let addr = json!("deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx");

        // Test request with CommitmentConfig and no params
        let test_request = RpcRequest::GetRecentBlockhash;
        let request = test_request.build_request_json(1, None, Some(commitment_config.clone()));
        assert_eq!(request["params"], json!([commitment_config.clone()]));

        // Test request with CommitmentConfig and params
        let test_request = RpcRequest::GetBalance;
        let request =
            test_request.build_request_json(1, Some(addr.clone()), Some(commitment_config.clone()));
        assert_eq!(request["params"], json!([addr, commitment_config]));
    }
}
