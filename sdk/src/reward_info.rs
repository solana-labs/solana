use crate::reward_type::RewardType;

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone, Copy)]
pub struct RewardInfo {
    pub reward_type: RewardType,
    /// Reward amount
    pub lamports: i64,
    /// Account balance in lamports after `lamports` was applied
    pub post_balance: u64,
    /// Vote account commission when the reward was credited, only present for voting and staking rewards
    pub commission: Option<u8>,
}
