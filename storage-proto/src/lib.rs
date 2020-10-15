use serde::{Deserialize, Serialize};
use solana_sdk::deserialize_utils::default_on_eof;
use solana_transaction_status::{Reward, RewardType};

pub mod convert;

pub type StoredExtendedRewards = Vec<StoredExtendedReward>;

#[derive(Serialize, Deserialize)]
pub struct StoredExtendedReward {
    pubkey: String,
    lamports: i64,
    #[serde(deserialize_with = "default_on_eof")]
    post_balance: u64,
    #[serde(deserialize_with = "default_on_eof")]
    reward_type: Option<RewardType>,
}

impl From<StoredExtendedReward> for Reward {
    fn from(value: StoredExtendedReward) -> Self {
        let StoredExtendedReward {
            pubkey,
            lamports,
            post_balance,
            reward_type,
        } = value;
        Self {
            pubkey,
            lamports,
            post_balance,
            reward_type,
        }
    }
}

impl From<Reward> for StoredExtendedReward {
    fn from(value: Reward) -> Self {
        let Reward {
            pubkey,
            lamports,
            post_balance,
            reward_type,
            ..
        } = value;
        Self {
            pubkey,
            lamports,
            post_balance,
            reward_type,
        }
    }
}
