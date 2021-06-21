#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfirmedBlock {
    #[prost(string, tag = "1")]
    pub previous_blockhash: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub blockhash: ::prost::alloc::string::String,
    #[prost(uint64, tag = "3")]
    pub parent_slot: u64,
    #[prost(message, repeated, tag = "4")]
    pub transactions: ::prost::alloc::vec::Vec<ConfirmedTransaction>,
    #[prost(message, repeated, tag = "5")]
    pub rewards: ::prost::alloc::vec::Vec<Reward>,
    #[prost(message, optional, tag = "6")]
    pub block_time: ::core::option::Option<UnixTimestamp>,
    #[prost(message, optional, tag = "7")]
    pub block_height: ::core::option::Option<BlockHeight>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfirmedTransaction {
    #[prost(message, optional, tag = "1")]
    pub transaction: ::core::option::Option<Transaction>,
    #[prost(message, optional, tag = "2")]
    pub meta: ::core::option::Option<TransactionStatusMeta>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Transaction {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub signatures: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
    #[prost(message, optional, tag = "2")]
    pub message: ::core::option::Option<Message>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Message {
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<MessageHeader>,
    #[prost(bytes = "vec", repeated, tag = "2")]
    pub account_keys: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
    #[prost(bytes = "vec", tag = "3")]
    pub recent_blockhash: ::prost::alloc::vec::Vec<u8>,
    #[prost(message, repeated, tag = "4")]
    pub instructions: ::prost::alloc::vec::Vec<CompiledInstruction>,
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
    pub err: ::core::option::Option<TransactionError>,
    #[prost(uint64, tag = "2")]
    pub fee: u64,
    #[prost(uint64, repeated, tag = "3")]
    pub pre_balances: ::prost::alloc::vec::Vec<u64>,
    #[prost(uint64, repeated, tag = "4")]
    pub post_balances: ::prost::alloc::vec::Vec<u64>,
    #[prost(message, repeated, tag = "5")]
    pub inner_instructions: ::prost::alloc::vec::Vec<InnerInstructions>,
    #[prost(string, repeated, tag = "6")]
    pub log_messages: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    #[prost(message, repeated, tag = "7")]
    pub pre_token_balances: ::prost::alloc::vec::Vec<TokenBalance>,
    #[prost(message, repeated, tag = "8")]
    pub post_token_balances: ::prost::alloc::vec::Vec<TokenBalance>,
    #[prost(message, repeated, tag = "9")]
    pub rewards: ::prost::alloc::vec::Vec<Reward>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransactionError {
    #[prost(bytes = "vec", tag = "1")]
    pub err: ::prost::alloc::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct InnerInstructions {
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(message, repeated, tag = "2")]
    pub instructions: ::prost::alloc::vec::Vec<CompiledInstruction>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CompiledInstruction {
    #[prost(uint32, tag = "1")]
    pub program_id_index: u32,
    #[prost(bytes = "vec", tag = "2")]
    pub accounts: ::prost::alloc::vec::Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TokenBalance {
    #[prost(uint32, tag = "1")]
    pub account_index: u32,
    #[prost(string, tag = "2")]
    pub mint: ::prost::alloc::string::String,
    #[prost(message, optional, tag = "3")]
    pub ui_token_amount: ::core::option::Option<UiTokenAmount>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UiTokenAmount {
    #[prost(double, tag = "1")]
    pub ui_amount: f64,
    #[prost(uint32, tag = "2")]
    pub decimals: u32,
    #[prost(string, tag = "3")]
    pub amount: ::prost::alloc::string::String,
    #[prost(string, tag = "4")]
    pub ui_amount_string: ::prost::alloc::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Reward {
    #[prost(string, tag = "1")]
    pub pubkey: ::prost::alloc::string::String,
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
    pub rewards: ::prost::alloc::vec::Vec<Reward>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UnixTimestamp {
    #[prost(int64, tag = "1")]
    pub timestamp: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockHeight {
    #[prost(uint64, tag = "1")]
    pub block_height: u64,
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
