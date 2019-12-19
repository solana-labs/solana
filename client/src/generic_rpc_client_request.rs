use crate::{client_error::ClientError, rpc_request::RpcRequest};

pub(crate) trait GenericRpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: serde_json::Value,
        retries: usize,
    ) -> Result<serde_json::Value, ClientError>;
}
