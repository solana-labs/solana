#![allow(deprecated)]
use {
    crate::config::{
        EncodingConfig, RpcBlockConfig, RpcEncodingConfigWrapper, RpcTransactionConfig,
    },
    solana_sdk::{clock::Slot, commitment_config::CommitmentConfig},
    solana_transaction_status::{TransactionDetails, UiTransactionEncoding},
};

#[deprecated(
    since = "1.7.0",
    note = "Please use RpcSignaturesForAddressConfig instead"
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcGetConfirmedSignaturesForAddress2Config {
    pub before: Option<String>, // Signature as base-58 string
    pub until: Option<String>,  // Signature as base-58 string
    pub limit: Option<usize>,
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
}

#[deprecated(since = "1.7.0", note = "Please use RpcBlockConfig instead")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcConfirmedBlockConfig {
    pub encoding: Option<UiTransactionEncoding>,
    pub transaction_details: Option<TransactionDetails>,
    pub rewards: Option<bool>,
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
}

impl EncodingConfig for RpcConfirmedBlockConfig {
    fn new_with_encoding(encoding: &Option<UiTransactionEncoding>) -> Self {
        Self {
            encoding: *encoding,
            ..Self::default()
        }
    }
}

impl RpcConfirmedBlockConfig {
    pub fn rewards_only() -> Self {
        Self {
            transaction_details: Some(TransactionDetails::None),
            ..Self::default()
        }
    }

    pub fn rewards_with_commitment(commitment: Option<CommitmentConfig>) -> Self {
        Self {
            transaction_details: Some(TransactionDetails::None),
            commitment,
            ..Self::default()
        }
    }
}

impl From<RpcConfirmedBlockConfig> for RpcEncodingConfigWrapper<RpcConfirmedBlockConfig> {
    fn from(config: RpcConfirmedBlockConfig) -> Self {
        RpcEncodingConfigWrapper::Current(Some(config))
    }
}

impl From<RpcConfirmedBlockConfig> for RpcBlockConfig {
    fn from(config: RpcConfirmedBlockConfig) -> Self {
        Self {
            encoding: config.encoding,
            transaction_details: config.transaction_details,
            rewards: config.rewards,
            commitment: config.commitment,
            max_supported_transaction_version: None,
        }
    }
}

#[deprecated(since = "1.7.0", note = "Please use RpcTransactionConfig instead")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcConfirmedTransactionConfig {
    pub encoding: Option<UiTransactionEncoding>,
    #[serde(flatten)]
    pub commitment: Option<CommitmentConfig>,
}

impl EncodingConfig for RpcConfirmedTransactionConfig {
    fn new_with_encoding(encoding: &Option<UiTransactionEncoding>) -> Self {
        Self {
            encoding: *encoding,
            ..Self::default()
        }
    }
}

impl From<RpcConfirmedTransactionConfig> for RpcTransactionConfig {
    fn from(config: RpcConfirmedTransactionConfig) -> Self {
        Self {
            encoding: config.encoding,
            commitment: config.commitment,
            max_supported_transaction_version: None,
        }
    }
}

#[deprecated(since = "1.7.0", note = "Please use RpcBlocksConfigWrapper instead")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcConfirmedBlocksConfigWrapper {
    EndSlotOnly(Option<Slot>),
    CommitmentOnly(Option<CommitmentConfig>),
}

impl RpcConfirmedBlocksConfigWrapper {
    pub fn unzip(&self) -> (Option<Slot>, Option<CommitmentConfig>) {
        match &self {
            RpcConfirmedBlocksConfigWrapper::EndSlotOnly(end_slot) => (*end_slot, None),
            RpcConfirmedBlocksConfigWrapper::CommitmentOnly(commitment) => (None, *commitment),
        }
    }
}
