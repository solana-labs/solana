use crate::{client_error::ClientError, rpc_request::RpcRequest};
use solana_sdk::commitment_config::CommitmentConfig;

pub(crate) trait GenericRpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: Option<serde_json::Value>,
        retries: usize,
        commitment_config: Option<CommitmentConfig>,
    ) -> Result<serde_json::Value, ClientError>;
}
