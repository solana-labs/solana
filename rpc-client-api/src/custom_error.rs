//! Implementation defined RPC server errors
use {
    crate::response::RpcSimulateTransactionResult,
    jsonrpc_core::{Error, ErrorCode},
    solana_sdk::clock::Slot,
    solana_transaction_status::EncodeError,
    thiserror::Error,
};

// Keep in sync with web3.js/src/errors.ts
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
pub const JSON_RPC_SCAN_ERROR: i64 = -32012;
pub const JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_LEN_MISMATCH: i64 = -32013;
pub const JSON_RPC_SERVER_ERROR_BLOCK_STATUS_NOT_AVAILABLE_YET: i64 = -32014;
pub const JSON_RPC_SERVER_ERROR_UNSUPPORTED_TRANSACTION_VERSION: i64 = -32015;
pub const JSON_RPC_SERVER_ERROR_MIN_CONTEXT_SLOT_NOT_REACHED: i64 = -32016;

#[derive(Error, Debug)]
pub enum RpcCustomError {
    #[error("BlockCleanedUp")]
    BlockCleanedUp {
        slot: Slot,
        first_available_block: Slot,
    },
    #[error("SendTransactionPreflightFailure")]
    SendTransactionPreflightFailure {
        message: String,
        result: RpcSimulateTransactionResult,
    },
    #[error("TransactionSignatureVerificationFailure")]
    TransactionSignatureVerificationFailure,
    #[error("BlockNotAvailable")]
    BlockNotAvailable { slot: Slot },
    #[error("NodeUnhealthy")]
    NodeUnhealthy { num_slots_behind: Option<Slot> },
    #[error("TransactionPrecompileVerificationFailure")]
    TransactionPrecompileVerificationFailure(solana_sdk::transaction::TransactionError),
    #[error("SlotSkipped")]
    SlotSkipped { slot: Slot },
    #[error("NoSnapshot")]
    NoSnapshot,
    #[error("LongTermStorageSlotSkipped")]
    LongTermStorageSlotSkipped { slot: Slot },
    #[error("KeyExcludedFromSecondaryIndex")]
    KeyExcludedFromSecondaryIndex { index_key: String },
    #[error("TransactionHistoryNotAvailable")]
    TransactionHistoryNotAvailable,
    #[error("ScanError")]
    ScanError { message: String },
    #[error("TransactionSignatureLenMismatch")]
    TransactionSignatureLenMismatch,
    #[error("BlockStatusNotAvailableYet")]
    BlockStatusNotAvailableYet { slot: Slot },
    #[error("UnsupportedTransactionVersion")]
    UnsupportedTransactionVersion(u8),
    #[error("MinContextSlotNotReached")]
    MinContextSlotNotReached { context_slot: Slot },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeUnhealthyErrorData {
    pub num_slots_behind: Option<Slot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinContextSlotNotReachedErrorData {
    pub context_slot: Slot,
}

impl From<EncodeError> for RpcCustomError {
    fn from(err: EncodeError) -> Self {
        match err {
            EncodeError::UnsupportedTransactionVersion(version) => {
                Self::UnsupportedTransactionVersion(version)
            }
        }
    }
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
                    "Block {slot} cleaned up, does not exist on node. First available block: {first_available_block}",
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
                message: format!("Block not available for slot {slot}"),
                data: None,
            },
            RpcCustomError::NodeUnhealthy { num_slots_behind } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY),
                message: if let Some(num_slots_behind) = num_slots_behind {
                    format!("Node is behind by {num_slots_behind} slots")
                } else {
                    "Node is unhealthy".to_string()
                },
                data: Some(serde_json::json!(NodeUnhealthyErrorData {
                    num_slots_behind
                })),
            },
            RpcCustomError::TransactionPrecompileVerificationFailure(e) => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_TRANSACTION_PRECOMPILE_VERIFICATION_FAILURE,
                ),
                message: format!("Transaction precompile verification failure {e:?}"),
                data: None,
            },
            RpcCustomError::SlotSkipped { slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_SLOT_SKIPPED),
                message: format!(
                    "Slot {slot} was skipped, or missing due to ledger jump to recent snapshot"
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
                message: format!("Slot {slot} was skipped, or missing in long-term storage"),
                data: None,
            },
            RpcCustomError::KeyExcludedFromSecondaryIndex { index_key } => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_KEY_EXCLUDED_FROM_SECONDARY_INDEX,
                ),
                message: format!(
                    "{index_key} excluded from account secondary indexes; \
                    this RPC method unavailable for key"
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
            RpcCustomError::ScanError { message } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SCAN_ERROR),
                message,
                data: None,
            },
            RpcCustomError::TransactionSignatureLenMismatch => Self {
                code: ErrorCode::ServerError(
                    JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_LEN_MISMATCH,
                ),
                message: "Transaction signature length mismatch".to_string(),
                data: None,
            },
            RpcCustomError::BlockStatusNotAvailableYet { slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_BLOCK_STATUS_NOT_AVAILABLE_YET),
                message: format!("Block status not yet available for slot {slot}"),
                data: None,
            },
            RpcCustomError::UnsupportedTransactionVersion(version) => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_UNSUPPORTED_TRANSACTION_VERSION),
                message: format!(
                    "Transaction version ({version}) is not supported by the requesting client. \
                    Please try the request again with the following configuration parameter: \
                    \"maxSupportedTransactionVersion\": {version}"
                ),
                data: None,
            },
            RpcCustomError::MinContextSlotNotReached { context_slot } => Self {
                code: ErrorCode::ServerError(JSON_RPC_SERVER_ERROR_MIN_CONTEXT_SLOT_NOT_REACHED),
                message: "Minimum context slot has not been reached".to_string(),
                data: Some(serde_json::json!(MinContextSlotNotReachedErrorData {
                    context_slot,
                })),
            },
        }
    }
}
