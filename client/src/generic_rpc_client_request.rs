use crate::{client_error::Result, rpc_request::RpcRequest};

pub(crate) trait GenericRpcClientRequest {
    fn send(&self, request: RpcRequest, params: serde_json::Value) -> Result<serde_json::Value>;
}
