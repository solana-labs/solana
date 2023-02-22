//! A type to hold data for the [`RewardHistory` sysvar][sv].
//!
//! [sv]: https://docs.solana.com/developing/runtime-facilities/sysvars#RewardHistory
//!
//! The sysvar ID is declared in [`sysvar::reward_history`].
//!
//! [`sysvar::reward_history`]: crate::sysvar::reward_history

pub use crate::clock::Epoch;
use std::ops::Deref;

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
pub struct RewardHistoryEntry {
    pub total: u64,       // total rewards at this epoch
    pub distributed: u64, // already distributed reward amount
    pub remaining: u64,   // remaining reward amount
    pub root_hash: Hash,
}

impl RewardHistoryEntry {
    pub fn new(total: u64, root_hash: Hash) -> Self {
        Self {
            total,
            distributed: 0,
            remaining: total,
            root_hash,
        }
    }
}

#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
pub struct RewardHistory(Vec<(Epoch, RewardHistoryEntry)>);

impl RewardHistory {
    pub fn get(&self, epoch: Epoch) -> Option<&RewardHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| &self[index].1)
    }

    pub fn add(&mut self, epoch: Epoch, entry: RewardHistoryEntry) {
        match self.binary_search_by(|probe| epoch.cmp(&probe.0)) {
            Ok(index) => (self.0)[index] = (epoch, entry),
            Err(index) => (self.0).insert(index, (epoch, entry)),
        }
        (self.0).truncate(MAX_ENTRIES);
    }
}

impl Deref for RewardHistory {
    type Target = Vec<(Epoch, RewardHistoryEntry)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reward_history() {
        let mut reward_history = RewardHistory::default();

        for i in 0..MAX_ENTRIES as u64 + 1 {
            reward_history.add(i, RewardHistoryEntry::new(i));
        }
        assert_eq!(reward_history.len(), MAX_ENTRIES);
        assert_eq!(reward_history.iter().map(|entry| entry.0).min().unwrap(), 1);
        assert_eq!(reward_history.get(0), None);
        assert_eq!(
            reward_history.get(1),
            Some(&RewardHistoryEntry {
                total: 1,
                distributed: 0,
                remaining: 1,
            })
        );
    }
}
