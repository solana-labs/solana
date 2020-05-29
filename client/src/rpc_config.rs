use solana_sdk::{clock::Epoch, commitment_config::CommitmentConfig};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSignatureStatusConfig {
    pub search_transaction_history: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
pub struct RpcInflationConfig {
    pub epoch: Option<Epoch>,
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
}
