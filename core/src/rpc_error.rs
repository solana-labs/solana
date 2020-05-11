use jsonrpc_core::{Error, ErrorCode};
use solana_sdk::clock::Slot;

const JSON_RPC_SERVER_ERROR_0: i64 = -32000;
const JSON_RPC_SERVER_ERROR_1: i64 = -32001;

pub enum RpcCustomError {
    NonexistentClusterRoot {
        cluster_root: Slot,
        node_root: Slot,
    },
    BlockCleanedUp {
        slot: Slot,
        first_available_block: Slot,
    },
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
        }
    }
}
