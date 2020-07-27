use crate::rpc_filter::RpcFilterType;
use solana_account_decoder::UiAccountEncoding;
use solana_sdk::{clock::Epoch, commitment_config::CommitmentConfig};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSignatureStatusConfig {
    pub search_transaction_history: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSendTransactionConfig {
    pub skip_preflight: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSimulateTransactionConfig {
    pub sig_verify: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcLargestAccountsFilter {
    Circulating,
    NonCirculating,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcLargestAccountsConfig {
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
    pub filter: Option<RpcLargestAccountsFilter>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcStakeConfig {
    pub epoch: Option<Epoch>,
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcAccountInfoConfig {
    pub encoding: Option<UiAccountEncoding>,
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcProgramAccountsConfig {
    pub filters: Option<Vec<RpcFilterType>>,
    #[serde(flatten)]
    pub account_config: RpcAccountInfoConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcTokenAccountsFilter {
    Mint(String),
    ProgramId(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcGetConfirmedSignaturesForAddress2Config {
    pub start_after: Option<String>, // Signature as base-58 string
    pub limit: Option<usize>,
}
