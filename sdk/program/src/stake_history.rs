//! A type to hold data for the [`StakeHistory` sysvar][sv].
//!
//! [sv]: https://docs.solanalabs.com/runtime/sysvars#stakehistory
//!
//! The sysvar ID is declared in [`sysvar::stake_history`].
//!
//! [`sysvar::stake_history`]: crate::sysvar::stake_history

pub use crate::clock::Epoch;
use {
    crate::{account_info::AccountInfo, program_error::ProgramError, sysvar::SysvarId},
    bytemuck::{Pod, Zeroable},
    std::{ops::Deref, sync::Arc},
};

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[repr(C)]
#[derive(
    Debug, Serialize, Deserialize, PartialEq, Eq, Default, Copy, Clone, Pod, Zeroable, AbiExample,
)]
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
    // deprecated due to naming clash with `Sysvar::get()`
    #[deprecated(note = "Please use `get_entry` instead")]
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

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StakeHistoryData<'a>(&'a [u8]);

impl<'a> StakeHistoryData<'a> {
    pub fn take_account_info(account_info: &AccountInfo<'a>) -> Result<Self, ProgramError> {
        if *account_info.unsigned_key() != StakeHistory::id() {
            return Err(ProgramError::InvalidArgument);
        }
        Ok(StakeHistoryData(account_info.data.take()))
    }
}

pub trait StakeHistoryGetEntry {
    fn get_entry(&self, epoch: Epoch) -> Option<&StakeHistoryEntry>;
}

impl StakeHistoryGetEntry for StakeHistory {
    fn get_entry(&self, epoch: Epoch) -> Option<&StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| &self[index].1)
    }
}

// this is only required for SysvarCache
// we dont impl for Deref directly because it would prevent a matching impl for StakeHistoryData
impl StakeHistoryGetEntry for Arc<StakeHistory> {
    fn get_entry(&self, epoch: Epoch) -> Option<&StakeHistoryEntry> {
        self.deref().get_entry(epoch)
    }
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, Copy, Clone, Pod, Zeroable)]
struct StakeHistoryEpochEntry {
    epoch: Epoch,
    entry: StakeHistoryEntry,
}

impl StakeHistoryGetEntry for StakeHistoryData<'_> {
    fn get_entry(&self, epoch: Epoch) -> Option<&StakeHistoryEntry> {
        let data = self.0;
        if let Ok(history) = bytemuck::try_cast_slice::<u8, StakeHistoryEpochEntry>(&data[8..]) {
            history
                .binary_search_by(|probe| epoch.cmp(&probe.epoch))
                .ok()
                .map(|index| &history[index].entry)
        } else {
            None
        }
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
        let stake_history_data = StakeHistoryData::take_account_info(&account_info).unwrap();

        assert_eq!(stake_history.get_entry(0), None);
        assert_eq!(stake_history_data.get_entry(0), None);
        for i in 1..MAX_ENTRIES as u64 + 1 {
            let expected = Some(StakeHistoryEntry {
                activating: i,
                ..StakeHistoryEntry::default()
            });

            assert_eq!(stake_history.get_entry(i), expected.as_ref());
            assert_eq!(stake_history_data.get_entry(i), expected.as_ref());
        }
    }
}
