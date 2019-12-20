//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries history about stake activations and de-activations
//!
pub use crate::clock::Epoch;

use crate::{account::Account, sysvar::Sysvar};
use std::ops::Deref;

crate::declare_sysvar_id!("SysvarStakeHistory1111111111111111111111111", StakeHistory);

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct StakeHistoryEntry {
    pub effective: u64,    // effective stake at this epoch
    pub activating: u64,   // sum of portion of stakes not fully warmed up
    pub deactivating: u64, // requested to be cooled down, not fully deactivated yet
}

#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct StakeHistory(Vec<(Epoch, StakeHistoryEntry)>);

impl Sysvar for StakeHistory {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        16392 // golden, update if MAX_ENTRIES changes
    }
}

impl StakeHistory {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn get(&self, epoch: &Epoch) -> Option<&StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| &self[index].1)
    }

    pub fn add(&mut self, epoch: Epoch, entry: StakeHistoryEntry) {
        match self.binary_search_by(|probe| epoch.cmp(&probe.0)) {
            Ok(index) => (self.0)[index] = (epoch, entry),
            Err(index) => (self.0).insert(index, (epoch, entry)),
        }
        (self.0).truncate(MAX_ENTRIES);
    }
}

impl Deref for StakeHistory {
    type Target = Vec<(Epoch, StakeHistoryEntry)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn create_account(lamports: u64, stake_history: &StakeHistory) -> Account {
    stake_history.create_account(lamports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_of() {
        assert_eq!(
            bincode::serialized_size(&StakeHistory(vec![
                (0, StakeHistoryEntry::default());
                MAX_ENTRIES
            ]))
            .unwrap() as usize,
            StakeHistory::size_of()
        );
    }

    #[test]
    fn test_create_account() {
        let lamports = 42;
        let account = StakeHistory::default().create_account(lamports);
        assert_eq!(account.data.len(), StakeHistory::size_of());

        let stake_history = StakeHistory::from_account(&account);
        assert_eq!(stake_history, Some(StakeHistory::default()));

        let mut stake_history = stake_history.unwrap();
        for i in 0..MAX_ENTRIES as u64 + 1 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }
        assert_eq!(stake_history.len(), MAX_ENTRIES);
        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 1);
        assert_eq!(stake_history.get(&0), None);
        assert_eq!(
            stake_history.get(&1),
            Some(&StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            })
        );
        // verify the account can hold a full instance
        assert_eq!(
            StakeHistory::from_account(&stake_history.create_account(lamports)),
            Some(stake_history)
        );
    }
}
