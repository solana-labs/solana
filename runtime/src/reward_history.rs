//! This module implements clone-on-write semantics for the `RewardHistory` to reduce
//! unnecessary cloning of the underlying vector.
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// The reward history with clone-on-write semantics
#[derive(Default, Clone, PartialEq, Eq, Debug, Deserialize, Serialize, AbiExample)]
pub struct RewardHistory(Arc<RewardHistoryInner>);

impl Deref for RewardHistory {
    type Target = RewardHistoryInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RewardHistory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

/// The inner type, which is the SDK's reward history
type RewardHistoryInner = solana_sdk::reward_history::RewardHistory;

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::reward_history::RewardHistoryEntry};

    fn rand_reward_history_entry() -> RewardHistoryEntry {
        RewardHistoryEntry {
            effective: rand::random(),
            activating: rand::random(),
            deactivating: rand::random(),
        }
    }

    /// Ensure that RewardHistory is indeed clone-on-write
    #[test]
    fn test_reward_history_is_cow() {
        let mut reward_history = RewardHistory::default();
        (100..109).for_each(|epoch| {
            let entry = rand_reward_history_entry();
            reward_history.add(epoch, entry);
        });

        // Test: Clone the stake history and **do not modify**.  Assert the underlying instances
        // are the same.
        {
            let reward_history2 = reward_history.clone();
            assert_eq!(reward_history, reward_history2);
            assert!(
                Arc::ptr_eq(&reward_history.0, &reward_history2.0),
                "Inner Arc must point to the same underlying instance"
            );
            assert!(
                std::ptr::eq(reward_history.deref(), reward_history2.deref()),
                "Deref must point to the same underlying instance"
            );
        }

        // Test: Clone the stake history and then modify.  Assert the underlying instances are
        // unique.
        {
            let mut reward_history2 = reward_history.clone();
            assert_eq!(reward_history, reward_history2);
            (200..209).for_each(|epoch| {
                let entry = rand_reward_history_entry();
                reward_history2.add(epoch, entry);
            });
            assert_ne!(reward_history, reward_history2);
            assert!(
                !Arc::ptr_eq(&reward_history.0, &reward_history2.0),
                "Inner Arc must point to a different underlying instance"
            );
            assert!(
                !std::ptr::eq(reward_history.deref(), reward_history2.deref()),
                "Deref must point to a different underlying instance"
            );
        }
    }

    /// Ensure that RewardHistory serializes and deserializes between the inner and outer types
    #[test]
    fn test_reward_history_serde() {
        let mut reward_history_outer = RewardHistory::default();
        let mut reward_history_inner = RewardHistoryInner::default();
        (2134..).take(11).for_each(|epoch| {
            let entry = rand_reward_history_entry();
            reward_history_outer.add(epoch, entry.clone());
            reward_history_inner.add(epoch, entry);
        });

        // Test: Assert that serializing the outer and inner types produces the same data
        assert_eq!(
            bincode::serialize(&reward_history_outer).unwrap(),
            bincode::serialize(&reward_history_inner).unwrap(),
        );

        // Test: Assert that serializing the outer type then deserializing to the inner type
        // produces the same values
        {
            let data = bincode::serialize(&reward_history_outer).unwrap();
            let deserialized_inner: RewardHistoryInner = bincode::deserialize(&data).unwrap();
            assert_eq!(&deserialized_inner, reward_history_outer.deref());
        }

        // Test: Assert that serializing the inner type then deserializing to the outer type
        // produces the same values
        {
            let data = bincode::serialize(&reward_history_inner).unwrap();
            let deserialized_outer: RewardHistory = bincode::deserialize(&data).unwrap();
            assert_eq!(deserialized_outer.deref(), &reward_history_inner);
        }
    }
}
