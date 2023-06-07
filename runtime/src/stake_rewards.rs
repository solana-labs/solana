//! Code for stake and vote rewards

use {
    crate::bank::RewardInfo,
    solana_sdk::{account::AccountSharedData, pubkey::Pubkey},
};

#[derive(AbiExample, Debug, Serialize, Deserialize, Clone, PartialEq)]
pub(crate) struct StakeReward {
    pub(crate) stake_pubkey: Pubkey,
    pub(crate) stake_reward_info: RewardInfo,
    pub(crate) stake_account: AccountSharedData,
}

impl StakeReward {
    pub(crate) fn get_stake_reward(&self) -> i64 {
        self.stake_reward_info.lamports
    }
}
