//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries history about stake activations and de-activations
//!
use crate::account::Account;
use crate::sysvar;
use bincode::serialized_size;
use std::collections::HashMap;
use std::ops::Deref;

pub use crate::timing::Epoch;

const ID: [u8; 32] = [
    6, 167, 213, 23, 25, 53, 132, 208, 254, 237, 155, 179, 67, 29, 19, 32, 107, 229, 68, 40, 27,
    87, 184, 86, 108, 197, 55, 95, 244, 0, 0, 0,
];

crate::solana_name_id!(ID, "SysvarStakeHistory1111111111111111111111111");

pub const MAX_STAKE_HISTORY: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct StakeHistoryEntry {
    pub effective: u64,    // effective stake at this epoch
    pub activating: u64,   // sum of portion of stakes not fully warmed up
    pub deactivating: u64, // requested to be cooled down, not fully deactivated yet
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct StakeHistory {
    inner: HashMap<Epoch, StakeHistoryEntry>,
}

impl StakeHistory {
    pub fn from(account: &Account) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut Account) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(
            &(0..MAX_STAKE_HISTORY)
                .map(|i| (i as u64, StakeHistoryEntry::default()))
                .collect::<HashMap<_, _>>(),
        )
        .unwrap() as usize
    }
    pub fn add(&mut self, epoch: Epoch, entry: StakeHistoryEntry) {
        self.inner.insert(epoch, entry);

        if self.len() > MAX_STAKE_HISTORY {
            let oldest = *self.inner.keys().min().unwrap();
            self.inner.remove(&oldest);
        }
    }
}

impl Deref for StakeHistory {
    type Target = HashMap<Epoch, StakeHistoryEntry>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub fn create_account(lamports: u64, stake_history: &StakeHistory) -> Account {
    let mut account = Account::new(lamports, StakeHistory::size_of(), &sysvar::id());
    stake_history.to(&mut account).unwrap();
    account
}

use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
pub fn from_keyed_account(account: &KeyedAccount) -> Result<StakeHistory, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    StakeHistory::from(account.account).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let lamports = 42;
        let account = create_account(lamports, &StakeHistory::default());
        assert_eq!(account.data.len(), StakeHistory::size_of());

        let stake_history = StakeHistory::from(&account);
        assert_eq!(stake_history, Some(StakeHistory::default()));

        let mut stake_history = stake_history.unwrap();
        for i in 0..MAX_STAKE_HISTORY as u64 + 1 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }
        assert_eq!(stake_history.len(), MAX_STAKE_HISTORY);
        assert_eq!(*stake_history.keys().min().unwrap(), 1);
        // verify the account can hold a full instance
        assert_eq!(
            StakeHistory::from(&create_account(lamports, &stake_history)),
            Some(stake_history)
        );
    }
}
