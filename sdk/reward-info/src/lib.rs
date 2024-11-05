#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]
#[cfg(feature = "serde")]
use serde_derive::{Deserialize, Serialize};
#[cfg(feature = "frozen-abi")]
use solana_frozen_abi_macro::{AbiEnumVisitor, AbiExample};
use std::fmt;

#[cfg_attr(feature = "frozen-abi", derive(AbiExample, AbiEnumVisitor))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RewardType {
    Fee,
    Rent,
    Staking,
    Voting,
}

impl fmt::Display for RewardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RewardType::Fee => "fee",
                RewardType::Rent => "rent",
                RewardType::Staking => "staking",
                RewardType::Voting => "voting",
            }
        )
    }
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct RewardInfo {
    pub reward_type: RewardType,
    /// Reward amount
    pub lamports: i64,
    /// Account balance in lamports after `lamports` was applied
    pub post_balance: u64,
    /// Vote account commission when the reward was credited, only present for voting and staking rewards
    pub commission: Option<u8>,
}
