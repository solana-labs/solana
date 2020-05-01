use jsonrpc_core::{Error, ErrorCode, Value};
use solana_sdk::clock::Slot;

const JSON_RPC_SERVER_ERROR_0: i64 = -32000;
const JSON_RPC_SERVER_ERROR_1: i64 = -32001;

pub enum RpcCustomError {
    NonexistentClusterRoot { cluster_root: Slot, node_root: Slot },
    TransactionError(Value),
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
            RpcCustomError::TransactionError(data) => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_1),
                message: "on-chain transaction error".to_string(),
                data: Some(data),
            },
        }
    }
}
