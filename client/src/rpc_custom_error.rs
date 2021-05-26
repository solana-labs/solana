//! Implementation defined RPC server errors

use {
    crate::rpc_response::RpcSimulateTransactionResult,
    jsonrpc_core::{Error, ErrorCode},
    solana_sdk::clock::Slot,
};

pub const JSON_RPC_SERVER_ERROR_BLOCK_CLEANED_UP: i64 = -32001;
pub const JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE: i64 = -32002;
pub const JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE: i64 = -32003;
pub const JSON_RPC_SERVER_ERROR_BLOCK_NOT_AVAILABLE: i64 = -32004;
pub const JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY: i64 = -32005;
pub const JSON_RPC_SERVER_ERROR_TRANSACTION_PRECOMPILE_VERIFICATION_FAILURE: i64 = -32006;
pub const JSON_RPC_SERVER_ERROR_SLOT_SKIPPED: i64 = -32007;
pub const JSON_RPC_SERVER_ERROR_NO_SNAPSHOT: i64 = -32008;
pub const JSON_RPC_SERVER_ERROR_LONG_TERM_STORAGE_SLOT_SKIPPED: i64 = -32009;
pub const JSON_RPC_SERVER_ERROR_KEY_EXCLUDED_FROM_SECONDARY_INDEX: i64 = -32010;
pub const JSON_RPC_SERVER_ERROR_TRANSACTION_HISTORY_NOT_AVAILABLE: i64 = -32011;

pub enum RpcCustomError {
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
    NodeUnhealthy {
        num_slots_behind: Option<Slot>,
    },
    TransactionPrecompileVerificationFailure(solana_sdk::transaction::TransactionError),
    SlotSkipped {
        slot: Slot,
    },
    NoSnapshot,
    LongTermStorageSlotSkipped {
        slot: Slot,
    },
    KeyExcludedFromSecondaryIndex {
        index_key: String,
    },
    TransactionHistoryNotAvailable,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeUnhealthyErrorData {
    pub num_slots_behind: Option<Slot>,
}

impl From<RpcCustomError> for Error {
    fn from(e: RpcCustomError) -> Self {
        match e {
            RpcCustomError::BlockCleanedUp {
                slot,
                first_available_block,
            } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_BLOCK_CLEANED_UP),
                message: format!(
                    "Block {} cleaned up, does not exist on node. First available block: {}",
                    slot, first_available_block,
                ),
                data: None,
            },
            RpcCustomError::SendTransactionPreflightFailure { message, result } => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE,
                ),
                message,
                data: Some(serde_json::json!(result)),
            },
            RpcCustomError::TransactionSignatureVerificationFailure => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE,
                ),
                message: "Transaction signature verification failure".to_string(),
                data: None,
            },
            RpcCustomError::BlockNotAvailable { slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_BLOCK_NOT_AVAILABLE),
                message: format!("Block not available for slot {}", slot),
                data: None,
            },
            RpcCustomError::NodeUnhealthy { num_slots_behind } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY),
                message: if let Some(num_slots_behind) = num_slots_behind {
                    format!("Node is behind by {} slots", num_slots_behind)
                } else {
                    "Node is unhealthy".to_string()
                },
                data: Some(serde_json::json!(NodeUnhealthyErrorData {
                    num_slots_behind
                })),
            },
            RpcCustomError::TransactionPrecompileVerificationFailure(e) => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE,
                ),
                message: format!("Transaction precompile verification failure {:?}", e),
                data: None,
            },
            RpcCustomError::SlotSkipped { slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_SLOT_SKIPPED),
                message: format!(
                    "Slot {} was skipped, or missing due to ledger jump to recent snapshot",
                    slot
                ),
                data: None,
            },
            RpcCustomError::NoSnapshot => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_NO_SNAPSHOT),
                message: "No snapshot".to_string(),
                data: None,
            },
            RpcCustomError::LongTermStorageSlotSkipped { slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_LONG_TERM_STORAGE_SLOT_SKIPPED),
                message: format!("Slot {} was skipped, or missing in long-term storage", slot),
                data: None,
            },
            RpcCustomError::KeyExcludedFromSecondaryIndex { index_key } => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_KEY_EXCLUDED_FROM_SECONDARY_INDEX,
                ),
                message: format!(
                    "{} excluded from account secondary indexes; \
                    this RPC method unavailable for key",
                    index_key
                ),
                data: None,
            },
            RpcCustomError::TransactionHistoryNotAvailable => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_TRANSACTION_HISTORY_NOT_AVAILABLE,
                ),
                message: "Transaction history is not available from this node".to_string(),
                data: None,
            },
        }
    }
}
