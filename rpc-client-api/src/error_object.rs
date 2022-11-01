#[derive(Deserialize, Debug)]
pub struct RpcErrorObject {
    pub code: i64,
    pub message: String,
}
