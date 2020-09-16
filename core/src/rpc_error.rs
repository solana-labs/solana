use jsonrpc_core::{Error, ErrorCode};
use solana_client::rpc_response::RpcSimulateTransactionResult;
use solana_sdk::clock::Slot;

const JSON_RPC_SERVER_ERROR_0: i64 = -32000;
const JSON_RPC_SERVER_ERROR_1: i64 = -32001;
const JSON_RPC_SERVER_ERROR_2: i64 = -32002;
const JSON_RPC_SERVER_ERROR_3: i64 = -32003;
const JSON_RPC_SERVER_ERROR_4: i64 = -32004;
const JSON_RPC_SERVER_ERROR_5: i64 = -32005;

pub enum RpcCustomError {
    NonexistentClusterRoot {
        cluster_root: Slot,
        node_root: Slot,
    },
    BlockCleanedUp {
        slot: Slot,
        first_available_block: Slot,
    },
    SendTransactionPreflightFailure {
        message: String,
        result: RpcSimulateTransactionResult,
    },
    TransactionSignatureVerificationFailure,
    BlockNotAvailable {
        slot: Slot,
    },
    RpcNodeUnhealthy,
}

impl From<RpcCustomError> for Error {
    fn from(e: RpcCustomError) -> Self {
        match e {
            RpcCustomError::NonexistentClusterRoot {
                cluster_root,
                node_root,
            } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_0),
                message: format!(
                    "Cluster largest_confirmed_root {} does not exist on node. Node root: {}",
                    cluster_root, node_root,
                ),
                data: None,
            },
            RpcCustomError::BlockCleanedUp {
                slot,
                first_available_block,
            } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_1),
                message: format!(
                    "Block {} cleaned up, does not exist on node. First available block: {}",
                    slot, first_available_block,
                ),
                data: None,
            },
            RpcCustomError::SendTransactionPreflightFailure { message, result } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_2),
                message,
                data: Some(serde_json::json!(result)),
            },
            RpcCustomError::TransactionSignatureVerificationFailure => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_3),
                message: "Transaction signature verification failure".to_string(),
                data: None,
            },
            RpcCustomError::BlockNotAvailable { slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_4),
                message: format!("Block not available for slot {}", slot,),
                data: None,
            },
            RpcCustomError::RpcNodeUnhealthy => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_5),
                message: "RPC node is unhealthy".to_string(),
                data: None,
            },
        }
    }
}
