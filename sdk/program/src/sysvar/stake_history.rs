//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries history about stake activations and de-activations
//!
pub use crate::stake_history::StakeHistory;

use crate::{account::Account, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarStakeHistory1111111111111111111111111", StakeHistory);

impl Sysvar for StakeHistory {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        16392 // golden, update if MAX_ENTRIES changes
    }
}

pub fn create_account(lamports: u64, stake_history: &StakeHistory) -> Account {
    stake_history.create_account(lamports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stake_history::*;

    #[test]
    fn test_size_of() {
        let mut stake_history = StakeHistory::default();
        for i in 0..MAX_ENTRIES as u64 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }

        assert_eq!(
            bincode::serialized_size(&stake_history).unwrap() as usize,
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
