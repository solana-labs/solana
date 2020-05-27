use serde_json::{json, Value};
use std::fmt;
use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum RpcRequest {
    DeregisterNode,
    ValidatorExit,
    GetAccountInfo,
    GetBalance,
    GetBlockTime,
    GetClusterNodes,
    GetConfirmedBlock,
    GetConfirmedBlocks,
    GetConfirmedSignaturesForAddress,
    GetConfirmedTransaction,
    GetEpochInfo,
    GetEpochSchedule,
    GetFeeCalculatorForBlockhash,
    GetFeeRateGovernor,
    GetFees,
    GetGenesisHash,
    GetIdentity,
    GetInflationGovernor,
    GetInflationRate,
    GetLargestAccounts,
    GetLeaderSchedule,
    GetMinimumBalanceForRentExemption,
    GetProgramAccounts,
    GetRecentBlockhash,
    GetSignatureStatuses,
    GetSlot,
    GetSlotLeader,
    GetStorageTurn,
    GetStorageTurnRate,
    GetSlotsPerSegment,
    GetStoragePubkeysForSlot,
    GetSupply,
    GetTotalSupply,
    GetTransactionCount,
    GetVersion,
    GetVoteAccounts,
    MinimumLedgerSlot,
    RegisterNode,
    RequestAirdrop,
    SendTransaction,
    SimulateTransaction,
    SignVote,
}

impl fmt::Display for RpcRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let method = match self {
            RpcRequest::DeregisterNode => "deregisterNode",
            RpcRequest::ValidatorExit => "validatorExit",
            RpcRequest::GetAccountInfo => "getAccountInfo",
            RpcRequest::GetBalance => "getBalance",
            RpcRequest::GetBlockTime => "getBlockTime",
            RpcRequest::GetClusterNodes => "getClusterNodes",
            RpcRequest::GetConfirmedBlock => "getConfirmedBlock",
            RpcRequest::GetConfirmedBlocks => "getConfirmedBlocks",
            RpcRequest::GetConfirmedSignaturesForAddress => "getConfirmedSignaturesForAddress",
            RpcRequest::GetConfirmedTransaction => "getConfirmedTransaction",
            RpcRequest::GetEpochInfo => "getEpochInfo",
            RpcRequest::GetEpochSchedule => "getEpochSchedule",
            RpcRequest::GetFeeCalculatorForBlockhash => "getFeeCalculatorForBlockhash",
            RpcRequest::GetFeeRateGovernor => "getFeeRateGovernor",
            RpcRequest::GetFees => "getFees",
            RpcRequest::GetGenesisHash => "getGenesisHash",
            RpcRequest::GetIdentity => "getIdentity",
            RpcRequest::GetInflationGovernor => "getInflationGovernor",
            RpcRequest::GetInflationRate => "getInflationRate",
            RpcRequest::GetLargestAccounts => "getLargestAccounts",
            RpcRequest::GetLeaderSchedule => "getLeaderSchedule",
            RpcRequest::GetMinimumBalanceForRentExemption => "getMinimumBalanceForRentExemption",
            RpcRequest::GetProgramAccounts => "getProgramAccounts",
            RpcRequest::GetRecentBlockhash => "getRecentBlockhash",
            RpcRequest::GetSignatureStatuses => "getSignatureStatuses",
            RpcRequest::GetSlot => "getSlot",
            RpcRequest::GetSlotLeader => "getSlotLeader",
            RpcRequest::GetStorageTurn => "getStorageTurn",
            RpcRequest::GetStorageTurnRate => "getStorageTurnRate",
            RpcRequest::GetSlotsPerSegment => "getSlotsPerSegment",
            RpcRequest::GetStoragePubkeysForSlot => "getStoragePubkeysForSlot",
            RpcRequest::GetSupply => "getSupply",
            RpcRequest::GetTotalSupply => "getTotalSupply",
            RpcRequest::GetTransactionCount => "getTransactionCount",
            RpcRequest::GetVersion => "getVersion",
            RpcRequest::GetVoteAccounts => "getVoteAccounts",
            RpcRequest::MinimumLedgerSlot => "minimumLedgerSlot",
            RpcRequest::RegisterNode => "registerNode",
            RpcRequest::RequestAirdrop => "requestAirdrop",
            RpcRequest::SendTransaction => "sendTransaction",
            RpcRequest::SimulateTransaction => "simulateTransaction",
            RpcRequest::SignVote => "signVote",
        };

        write!(f, "{}", method)
    }
}

pub const NUM_LARGEST_ACCOUNTS: usize = 20;
pub const MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS: usize = 256;
pub const MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS_SLOT_RANGE: u64 = 10_000;

impl RpcRequest {
    pub(crate) fn build_request_json(self, id: u64, params: Value) -> Value {
        let jsonrpc = "2.0";
        json!({
           "jsonrpc": jsonrpc,
           "id": id,
           "method": format!("{}", self),
           "params": params,
        })
    }
}

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("rpc request error: {0}")]
    RpcRequestError(String),
    #[error("parse error: expected {0}")]
    ParseError(String), /* "expected" */
    // Anything in a `ForUser` needs to die.  The caller should be
    // deciding what to tell their user
    #[error("{0}")]
    ForUser(String), /* "direct-to-user message" */
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};

    #[test]
    fn test_build_request_json() {
        let test_request = RpcRequest::GetAccountInfo;
        let addr = json!("deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx");
        let request = test_request.build_request_json(1, json!([addr.clone()]));
        assert_eq!(request["method"], "getAccountInfo");
        assert_eq!(request["params"], json!([addr]));

        let test_request = RpcRequest::GetBalance;
        let request = test_request.build_request_json(1, json!([addr.clone()]));
        assert_eq!(request["method"], "getBalance");

        let test_request = RpcRequest::GetEpochInfo;
        let request = test_request.build_request_json(1, Value::Null);
        assert_eq!(request["method"], "getEpochInfo");

        let test_request = RpcRequest::GetRecentBlockhash;
        let request = test_request.build_request_json(1, Value::Null);
        assert_eq!(request["method"], "getRecentBlockhash");

        let test_request = RpcRequest::GetFeeCalculatorForBlockhash;
        let request = test_request.build_request_json(1, json!([addr]));
        assert_eq!(request["method"], "getFeeCalculatorForBlockhash");

        let test_request = RpcRequest::GetFeeRateGovernor;
        let request = test_request.build_request_json(1, Value::Null);
        assert_eq!(request["method"], "getFeeRateGovernor");

        let test_request = RpcRequest::GetSlot;
        let request = test_request.build_request_json(1, Value::Null);
        assert_eq!(request["method"], "getSlot");

        let test_request = RpcRequest::GetTransactionCount;
        let request = test_request.build_request_json(1, Value::Null);
        assert_eq!(request["method"], "getTransactionCount");

        let test_request = RpcRequest::RequestAirdrop;
        let request = test_request.build_request_json(1, Value::Null);
        assert_eq!(request["method"], "requestAirdrop");

        let test_request = RpcRequest::SendTransaction;
        let request = test_request.build_request_json(1, Value::Null);
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
        let request = test_request.build_request_json(1, json!([commitment_config.clone()]));
        assert_eq!(request["params"], json!([commitment_config.clone()]));

        // Test request with CommitmentConfig and params
        let test_request = RpcRequest::GetBalance;
        let request =
            test_request.build_request_json(1, json!([addr.clone(), commitment_config.clone()]));
        assert_eq!(request["params"], json!([addr, commitment_config]));
    }
}
