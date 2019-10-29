use crate::client_error::ClientError;
use crate::rpc_request::{RpcCommitmentConfig, RpcRequest};

pub(crate) trait GenericRpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: Option<serde_json::Value>,
        retries: usize,
        commitment_config: Option<RpcCommitmentConfig>,
    ) -> Result<serde_json::Value, ClientError>;
}
