use jsonrpc_core::{Error, ErrorCode};
use solana_sdk::clock::Slot;

const JSON_RPC_SERVER_ERROR_1: i64 = -32001;
const JSON_RPC_SERVER_ERROR_2: i64 = -32002;

pub enum RpcCustomError {
    BlockCleanedUp {
        slot: Slot,
        first_available_block: Slot,
    },
    SendTransactionPreflightFailure {
        message: String,
    },
}

impl From<RpcCustomError> for Error {
    fn from(e: RpcCustomError) -> Self {
        match e {
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
            RpcCustomError::SendTransactionPreflightFailure { message } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_2),
                message,
                data: None,
            },
        }
    }
}
