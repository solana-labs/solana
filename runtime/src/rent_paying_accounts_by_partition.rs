//! Provide fast iteration of all pubkeys which could possibly be rent paying, grouped by rent collection partition
use {
    crate::bank::{Bank, PartitionIndex, PartitionsPerCycle},
    solana_sdk::{epoch_schedule::EpochSchedule, pubkey::Pubkey},
    std::collections::HashSet,
};

lazy_static! {
    pub static ref EMPTY_HASHSET: HashSet<Pubkey> = HashSet::default();
}

/// populated at startup with the accounts that were found that are rent paying.
/// These are the 'possible' rent paying accounts.
/// This set can never grow during runtime since it is not possible to create rent paying accounts now.
/// It can shrink during execution if a rent paying account is dropped to lamports=0 or is topped off.
/// The next time the validator restarts, it will remove the account from this list.
#[derive(Debug, Default)]
pub struct RentPayingAccountsByPartition {
    /// 1st index is partition end index, 0..=432_000
    /// 2nd dimension is list of pubkeys which were identified at startup to be rent paying
    /// At the moment, we use this data structure to verify all rent paying accounts are expected.
    /// When we stop iterating the accounts index to FIND rent paying accounts, we will no longer need this to be a hashset.
    /// It can just be a vec.
    pub accounts: Vec<HashSet<Pubkey>>,
    partition_count: PartitionsPerCycle,
}

impl RentPayingAccountsByPartition {
    /// create new struct. Need slots per epoch from 'epoch_schedule'
    pub fn new(epoch_schedule: &EpochSchedule) -> Self {
        let partition_count = epoch_schedule.slots_per_epoch;
        Self {
            partition_count,
            accounts: (0..=partition_count)
                .map(|_| HashSet::<Pubkey>::default())
                .collect(),
        }
    }
    /// Remember that 'pubkey' can possibly be rent paying.
    pub fn add_account(&mut self, pubkey: &Pubkey) {
        let partition_end_index = Bank::partition_from_pubkey(pubkey, self.partition_count);
        let list = &mut self.accounts[partition_end_index as usize];

        list.insert(*pubkey);
    }
    /// return all pubkeys that can possibly be rent paying with this partition end_index
    pub fn get_pubkeys_in_partition_index(
        &self,
        partition_end_index: PartitionIndex,
    ) -> &HashSet<Pubkey> {
        self.accounts
            .get(partition_end_index as usize)
            .unwrap_or(&EMPTY_HASHSET)
    }
    pub fn is_initialized(&self) -> bool {
        self.partition_count != 0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let mut test = RentPayingAccountsByPartition::new(&EpochSchedule::custom(32, 0, false));
        let pk = Pubkey::new(&[1; 32]);
        test.add_account(&pk);
        // make sure duplicate adds only result in a single item
        test.add_account(&pk);
        assert_eq!(test.get_pubkeys_in_partition_index(0).len(), 1);
        assert!(test.get_pubkeys_in_partition_index(1).is_empty());
        assert!(test.is_initialized());
        let test = RentPayingAccountsByPartition::default();
        assert!(!test.is_initialized());
    }
}
