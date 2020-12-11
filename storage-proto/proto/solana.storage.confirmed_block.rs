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
    #[prost(message, optional, tag = "1")]
    pub transaction: ::std::option::Option<Transaction>,
    #[prost(message, optional, tag = "2")]
    pub meta: ::std::option::Option<TransactionStatusMeta>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Transaction {
    #[prost(bytes, repeated, tag = "1")]
    pub signatures: ::std::vec::Vec<std::vec::Vec<u8>>,
    #[prost(message, optional, tag = "2")]
    pub message: ::std::option::Option<Message>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Message {
    #[prost(message, optional, tag = "1")]
    pub header: ::std::option::Option<MessageHeader>,
    #[prost(bytes, repeated, tag = "2")]
    pub account_keys: ::std::vec::Vec<std::vec::Vec<u8>>,
    #[prost(bytes, tag = "3")]
    pub recent_blockhash: std::vec::Vec<u8>,
    #[prost(message, repeated, tag = "4")]
    pub instructions: ::std::vec::Vec<CompiledInstruction>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MessageHeader {
    #[prost(uint32, tag = "1")]
    pub num_required_signatures: u32,
    #[prost(uint32, tag = "2")]
    pub num_readonly_signed_accounts: u32,
    #[prost(uint32, tag = "3")]
    pub num_readonly_unsigned_accounts: u32,
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
    #[prost(string, repeated, tag = "6")]
    pub log_messages: ::std::vec::Vec<std::string::String>,
    #[prost(message, repeated, tag = "7")]
    pub pre_token_balances: ::std::vec::Vec<TokenBalance>,
    #[prost(message, repeated, tag = "8")]
    pub post_token_balances: ::std::vec::Vec<TokenBalance>,
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
pub struct TokenBalance {
    #[prost(uint32, tag = "1")]
    pub account_index: u32,
    #[prost(string, tag = "2")]
    pub mint: std::string::String,
    #[prost(message, optional, tag = "3")]
    pub ui_token_amount: ::std::option::Option<UiTokenAmount>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UiTokenAmount {
    #[prost(double, tag = "1")]
    pub ui_amount: f64,
    #[prost(uint32, tag = "2")]
    pub decimals: u32,
    #[prost(string, tag = "3")]
    pub amount: std::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Reward {
    #[prost(string, tag = "1")]
    pub pubkey: std::string::String,
    #[prost(int64, tag = "2")]
    pub lamports: i64,
    #[prost(uint64, tag = "3")]
    pub post_balance: u64,
    #[prost(enumeration = "RewardType", tag = "4")]
    pub reward_type: i32,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Rewards {
    #[prost(message, repeated, tag = "1")]
    pub rewards: ::std::vec::Vec<Reward>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UnixTimestamp {
    #[prost(int64, tag = "1")]
    pub timestamp: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum RewardType {
    Unspecified = 0,
    Fee = 1,
    Rent = 2,
    Staking = 3,
    Voting = 4,
}
