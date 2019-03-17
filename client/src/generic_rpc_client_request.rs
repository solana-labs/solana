use crate::rpc_request::RpcRequest;

pub(crate) trait GenericRpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: Option<serde_json::Value>,
        retries: usize,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>>;
}
