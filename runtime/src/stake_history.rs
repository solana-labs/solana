//! This module implements clone-on-write semantics for the SDK's `StakeHistory` to reduce
//! unnecessary cloning of the underlying vector.
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// The SDK's stake history with clone-on-write semantics
#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]
pub struct StakeHistory(Arc<StakeHistoryInner>);

impl Deref for StakeHistory {
    type Target = StakeHistoryInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for StakeHistory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

/// The inner type, which is the SDK's stake history
type StakeHistoryInner = solana_sdk::stake_history::StakeHistory;

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::stake_history::StakeHistoryEntry;

    fn rand_stake_history_entry() -> StakeHistoryEntry {
        StakeHistoryEntry {
            effective: rand::random(),
            activating: rand::random(),
            deactivating: rand::random(),
        }
    }

    #[test]
    fn test_stake_history_is_cow() {
        let mut stake_history = StakeHistory::default();
        (100..109).for_each(|epoch| {
            let entry = rand_stake_history_entry();
            stake_history.add(epoch, entry);
        });

        // Test: Clone the stake history and **do not modify**.  Assert the underlying instances
        // are the same.
        {
            let stake_history2 = stake_history.clone();
            assert_eq!(stake_history, stake_history2);
            assert!(
                Arc::ptr_eq(&stake_history.0, &stake_history2.0),
                "Inner Arc must point to the same underlying instance"
            );
            assert!(
                std::ptr::eq(stake_history.deref(), stake_history2.deref()),
                "Deref must point to the same underlying instance"
            );
        }

        // Test: Clone the stake history and then modify.  Assert the underlying instances are
        // unique.
        {
            let mut stake_history2 = stake_history.clone();
            assert_eq!(stake_history, stake_history2);
            (200..209).for_each(|epoch| {
                let entry = rand_stake_history_entry();
                stake_history2.add(epoch, entry);
            });
            assert_ne!(stake_history, stake_history2);
            assert!(
                !Arc::ptr_eq(&stake_history.0, &stake_history2.0),
                "Inner Arc must point to a different underlying instance"
            );
            assert!(
                !std::ptr::eq(stake_history.deref(), stake_history2.deref()),
                "Deref must point to a different underlying instance"
            );
        }
    }
}
