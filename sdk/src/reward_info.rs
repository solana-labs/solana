use crate::reward_type::RewardType;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, AbiExample, Clone, Copy)]
pub struct RewardInfo {
    pub reward_type: RewardType,
    /// Reward amount
    pub lamports: i64,
    /// Account balance in lamports after `lamports` was applied
    pub post_balance: u64,
    /// Vote account commission when the reward was credited, only present for voting and staking rewards
    pub commission: Option<u8>,
}
