#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfirmedBlock {
    #[prost(string, tag = "1")]
    pub previous_blockhash: std::string::String,
    #[prost(string, tag = "2")]
    pub blockhash: std::string::String,
    #[prost(uint64, tag = "3")]
    pub parent_slot: u64,
    #[prost(message, repeated, tag = "4")]
    pub transactions: ::std::vec::Vec<ConfirmedTransaction>,
    #[prost(message, repeated, tag = "5")]
    pub rewards: ::std::vec::Vec<Reward>,
    #[prost(message, optional, tag = "6")]
    pub block_time: ::std::option::Option<UnixTimestamp>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfirmedTransaction {
    #[prost(bytes, tag = "1")]
    pub transaction: std::vec::Vec<u8>,
    #[prost(message, optional, tag = "2")]
    pub meta: ::std::option::Option<TransactionStatusMeta>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransactionStatusMeta {
    #[prost(message, optional, tag = "1")]
    pub err: ::std::option::Option<TransactionError>,
    #[prost(uint64, tag = "2")]
    pub fee: u64,
    #[prost(uint64, repeated, tag = "3")]
    pub pre_balances: ::std::vec::Vec<u64>,
    #[prost(uint64, repeated, tag = "4")]
    pub post_balances: ::std::vec::Vec<u64>,
    #[prost(message, repeated, tag = "5")]
    pub inner_instructions: ::std::vec::Vec<InnerInstructions>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransactionError {
    #[prost(bytes, tag = "1")]
    pub err: std::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct InnerInstructions {
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(message, repeated, tag = "2")]
    pub instructions: ::std::vec::Vec<CompiledInstruction>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompiledInstruction {
    #[prost(uint32, tag = "1")]
    pub program_id_index: u32,
    #[prost(bytes, tag = "2")]
    pub accounts: std::vec::Vec<u8>,
    #[prost(bytes, tag = "3")]
    pub data: std::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Reward {
    #[prost(string, tag = "1")]
    pub pubkey: std::string::String,
    #[prost(int64, tag = "2")]
    pub lamports: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UnixTimestamp {
    #[prost(int64, tag = "1")]
    pub timestamp: i64,
}
