//! A type to hold data for the [`StakeHistory` sysvar][sv].
//!
//! [sv]: https://docs.solanalabs.com/runtime/sysvars#stakehistory
//!
//! The sysvar ID is declared in [`sysvar::stake_history`].
//!
//! [`sysvar::stake_history`]: crate::sysvar::stake_history

pub use crate::clock::Epoch;
use {
    crate::{account_info::AccountInfo, serialize_utils::cursor::read_u64},
    std::{cell::RefCell, io::Cursor, ops::Deref, rc::Rc, sync::Arc},
};

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
pub struct StakeHistoryEntry {
    pub effective: u64,    // effective stake at this epoch
    pub activating: u64,   // sum of portion of stakes not fully warmed up
    pub deactivating: u64, // requested to be cooled down, not fully deactivated yet
}

impl StakeHistoryEntry {
    pub fn with_effective(effective: u64) -> Self {
        Self {
            effective,
            ..Self::default()
        }
    }

    pub fn with_effective_and_activating(effective: u64, activating: u64) -> Self {
        Self {
            effective,
            activating,
            ..Self::default()
        }
    }

    pub fn with_deactivating(deactivating: u64) -> Self {
        Self {
            effective: deactivating,
            deactivating,
            ..Self::default()
        }
    }
}

impl std::ops::Add for StakeHistoryEntry {
    type Output = StakeHistoryEntry;
    fn add(self, rhs: StakeHistoryEntry) -> Self::Output {
        Self {
            effective: self.effective.saturating_add(rhs.effective),
            activating: self.activating.saturating_add(rhs.activating),
            deactivating: self.deactivating.saturating_add(rhs.deactivating),
        }
    }
}

#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
pub struct StakeHistory(Vec<(Epoch, StakeHistoryEntry)>);

impl StakeHistory {
    pub fn get(&self, epoch: Epoch) -> Option<&StakeHistoryEntry> {
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

// HANA my stuff below

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StakeHistoryData<'a>(Rc<RefCell<&'a mut [u8]>>);

impl<'a> StakeHistoryData<'a> {
    pub fn from_account_info(account_info: &AccountInfo<'a>) -> StakeHistoryData<'a> {
        // TODO check address
        StakeHistoryData(account_info.data.clone())
    }
}

// HANA i would like to change this to Option<&StakeHistoryEntry> but the types are complicated
// i dont think its possible to take a reference to the data inside the RefCell
// i also dont think theres any way to heap allocation in my new impl and get a normal reference
// and i dont think theres a way to use something like AsRef to let me box my new one and not the old one
// and anyway callers of the old one wouldnt have a type specialized to normal reference
// so really i would need like... hmm i can put `ReturnType: AsRef<StakeHistoryEntry>`
// and have each impl define its own return, Option<&StakeHistoryEntry> or Option<Box<StakeHistoryEntry>>
pub trait StakeHistoryGetEntry {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry>;
}

impl StakeHistoryGetEntry for StakeHistory {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| self[index].1.clone())
    }
}

// HANA this is only required as long as we use the sysvar cache
impl StakeHistoryGetEntry for Arc<StakeHistory> {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry> {
        self.deref().get_entry(epoch)
    }
}

impl StakeHistoryGetEntry for StakeHistoryData<'_> {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry> {
        // TODO binary search
        let data = self.0.borrow();
        let mut cursor = Cursor::new(&*data);
        let entry_count = read_u64(&mut cursor).unwrap();

        for _ in 0..entry_count {
            // TODO skip reading next three and just adjust the cursor
            let entry_epoch = read_u64(&mut cursor).unwrap();
            let effective = read_u64(&mut cursor).unwrap();
            let activating = read_u64(&mut cursor).unwrap();
            let deactivating = read_u64(&mut cursor).unwrap();

            if epoch == entry_epoch {
                return Some(StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            pubkey::Pubkey,
            sysvar::{Sysvar, SysvarId},
        },
    };

    #[test]
    fn test_stake_history() {
        let mut stake_history = StakeHistory::default();

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
        assert_eq!(stake_history.get_entry(0), None);
        assert_eq!(
            stake_history.get_entry(1),
            Some(StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            })
        );
    }

    #[test]
    fn test_stake_history_data() {
        let mut stake_history = StakeHistory::default();

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

        let id = StakeHistory::id();
        let mut lamports = 0;
        let mut data = vec![0; StakeHistory::size_of()];
        let owner = Pubkey::default();
        let mut account_info = AccountInfo::new(
            &id,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );
        stake_history.to_account_info(&mut account_info).unwrap();

        let stake_history_data = StakeHistoryData::from_account_info(&account_info);

        assert_eq!(stake_history_data.get_entry(0), None);
        assert_eq!(
            stake_history_data.get_entry(1),
            Some(StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            })
        );
    }
}
