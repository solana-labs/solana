//! Logic for determining when rent should have been collected or a rewrite would have occurred for an account.
use {
    crate::{
        accounts_db::AccountsDb,
        accounts_hash::HashStats,
        bank::{Bank, PartitionIndex, Rewrites},
        rent_collector::{RentCollector, RentResult},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::sync::atomic::Ordering,
};

#[derive(Debug, PartialEq, Eq)]
pub struct ExpectedRentCollection {
    partition_from_pubkey: PartitionIndex,
    epoch_of_max_storage_slot: Epoch,
    partition_index_from_max_slot: PartitionIndex,
    first_slot_in_max_epoch: Slot,
    // these are the only 2 fields used by downstream calculations at the moment.
    // the rest are here for debugging
    expected_rent_collection_slot_max_epoch: Slot,
    rent_epoch: Epoch,
}

/*
A goal is to skip rewrites to improve performance.
Reasons this file exists:
A. When we encounter skipped-rewrite account data as part of a load, we may need to update rent_epoch.
B. When we encounter skipped-rewrite account data during accounts hash calc, the saved hash will be incorrect if we skipped a rewrite.
     We need slot and rent_epoch to recalculate a new hash.

cases of rent collection:

setup assumptions:
pubkey = abc
slots_per_epoch = 432,000
pubkey_partition_index of 'abc' = 80

So, rent will be collected or a rewrite is expected to occur:
 each time a slot's pubkey_partition is == [footnote1] pubkey_partition_index within an epoch. [footnote2]

 If we skip rewrites, then pubkey's account data will not be rewritten when its rent collecion partition index occurs.
 However, later we need to get a hash value for the most recent update to this account.
 That leads us to the purpose of this file.
 To calculate a hash for account data, we need to know:
   1. the slot the account was written in
   2. the rent_epoch field of the account, telling us which epoch is the next time we should evaluate rent on this account
 If we did not perform a rewrite, then the account in the append vec it was last written in has:
   1. The wrong slot, since the append vec is not associated with the slot that the rewrite would have occurred.
   2. The wrong rent_epoch since the account was not rewritten with a newer rent_epoch [footnote3]

Reason A for this file's existence:
When we encounter the skipped-rewrite account data as part of a load, we may need to update rent_epoch.
Many operations work like this:
  1. read account
  2. add lamports (ex: reward was paid)
  3. write account
If the account is written with the WRONG rent_epoch field, then we will store an 'incorrect' rent_epoch field and the hash will be incorrect.
So, at (1. read account), we must FIX the rent_epoch to be as it would be if the rewrite would have occurred.

Reason B for this file's existence:
When we encounter the skipped-rewrite account data during accounts hash calc, the saved hash will be incorrect if we skipped a rewrite.
We must compute the correct rent_epoch and slot and recompute the hash that we would have gotten if we would have done the rewrite.

Both reasons result in the need for the same answer.
Given
1. pubkey
2. pubkey's account data
3. the slot the data was loaded from (storage_slot)
4. the highest_root caller cares about

We also need a RentCollector and EpochSchedule for the highest root the caller cares about.
With these:
1. can calculate partition_index of
1.a. highest_root (slot)
1.b. storage_slot

We also need :
fn find_unskipped_slot(slot) -> root
which can return the lowest root >= slot - 1 (this is why 'historical_roots' is necessary) Also see [footnote1].

Given these inputs, we can determine the correct slot and rent_epoch where we SHOULD have found this account's data and also compute the hash that we SHOULD have stored at that slot.
Note that a slot could be (-432k..infinite) slots and (-1..infinite) epochs relative to the expected rent collection slot.
Examples:
1.
storage_slot: 1
highest_root: 1
since pubkey_partition_index is 80 and hasn't been reached, the current account's data is CORRECT
Every highest_root < 80 is this same result.

2.
storage_slot: 1
highest_root: 79
since pubkey_partition_index is 80 and hasn't been reached, the current account's data is CORRECT

3.
storage_slot: 1  (partition index = 1)
highest_root: 80 (partition index = 80)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 80
rent_epoch will be 0
Every highest_root >= 80 and < 432k + 80 is this same result

4.
storage_slot: 1  (partition index = 1)
find_unskipped_slot(80) returns 81 since slot 80 was SKIPPED
highest_root: 81 (partition index = 81)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 81 (note 81 because 80 was skipped)
rent_epoch will be 0
Every highest_root >= 80 and < 432k + 80 is this same result

5.
storage_slot:        1  (partition index = 1) (epoch 0)
highest_root: 432k + 1  (partition index = 1) (epoch 1)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 80 in epoch 0
partition_index 80 has NOT been reached in epoch 1
so, rent_epoch will be 0
Every highest_root >= 80 and < 432k + 80 is this same result

6.
storage_slot:         1  (partition index =  1) (epoch 0)
highest_root: 432k + 80  (partition index = 80) (epoch 1)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k + 80 in epoch 1
partition_index 80 HAS been reached in epoch 1
so, rent_epoch will be 1
Every highest_root >= 432k + 80 and < 432k * 2 + 80 is this same result

7.
storage_slot:         1  (partition index =  1) (epoch 0)
find_unskipped_slot(432k + 80) returns 432k + 81 since slot 432k + 80 was SKIPPED
highest_root: 432k + 81  (partition index = 81) (epoch 1)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k + 81 in epoch 1 (slot 432k + 80 was skipped)
partition_index 80 HAS been reached in epoch 1
so, rent_epoch will be 1
Every highest_root >= 432k + 81 and < 432k * 2 + 80 is this same result

8.
storage_slot:            1  (partition index = 1) (epoch 0)
highest_root: 432k * 2 + 1  (partition index = 1) (epoch 2)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k + 80 in epoch 1
partition_index 80 has NOT been reached in epoch 2
so, rent_epoch will 1
Every highest_root >= 432k + 80 and < 432k * 2 + 80 is this same result

9.
storage_slot:             1  (partition index =  1) (epoch 0)
highest_root: 432k * 2 + 80  (partition index = 80) (epoch 2)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k * 2 + 80 in epoch 2
partition_index 80 HAS been reached in epoch 2
so, rent_epoch will be 2
Every highest_root >= 432k * 2 + 80 and < 432k * 3 + 80 is this same result

[footnote1:]
  "each time a slot's pubkey_partition is == [footnote1] pubkey_partition_index within an epoch."
  Due to skipped slots, this is not true.
  In reality, it is:
    the first slot where a slot's pubkey_partition >= pubkey_partition_index - 1.
  So, simply, if the pubkey_partition for our rent is 80, then that slot occurs at these slot #s:
    Slot:...........     Epoch:
    0           + 80 for epoch 0
    432,000     + 80 for epoch 1
    432,000 * 2 + 80 for epoch 2
    ...
  However, sometimes we skip slots. So, just considering epoch 0:
    Normal, not skipping any slots:
      slot 78 is a root
      slot 79 is a root
      slot 80 is a root (account is rewritten/rent collected here)
    Scenario 1:
      slot 78 is a root
      slot 79 is skipped/not a root
      slot 80 is a root (account is rewritten/rent collected here along with accounts from slot 79)
    Scenario 2:
      slot 78 is a root
      slot 79 is skipped/not a root
      slot 80 is skipped/not a root
      slot 81 is a root (account is rewritten/rent collected here because of slot 80, along with accounts from slots 79 and 81)
    This gets us to looking for:
      the first slot where a slot's pubkey_partition >= pubkey_partition_index - 1.

[footnote2:]
  Describing partition_index within an epoch:
  example:
    slot=0       is epoch=0, partition_index=0
    slot=1       is epoch=0, partition_index=1
    slot=431,999 is epoch=0, partition_index=431,999
    slot=432,000 is epoch=1, partition_index=432,000
    This is NOT technically accurate because of first_normal_slot, but we'll ignore that.
    EpochSchedule::get_epoch_and_slot_index(slot) calculates epoch and partition_index from slot.

[footnote3:]
  when we do a rewrite of account data, only this data changes:
  1. rent_epoch
  2. computed hash value (which is a function of (data, lamports, executable, owner, rent_epoch, pubkey) + slot
  3. into a new append vec that is associated with the slot#

*/

/// specify a slot
/// and keep track of epoch info for that slot
/// The idea is that algorithms often iterate over a storage slot or over a max/bank slot.
/// The epoch info for that slot will be fixed for many pubkeys, so save the calculated results.
/// There are 2 slots required for rent collection algorithms. Some callers have storage slot fixed
///  while others have max/bank slot fixed. 'epoch_info' isn't always needed, so it is optionally
///  specified by caller and only used by callee if necessary.
#[derive(Default, Copy, Clone)]
pub struct SlotInfoInEpoch {
    /// the slot
    slot: Slot,
    /// possible info about this slot
    epoch_info: Option<SlotInfoInEpochInner>,
}

/// epoch info for a slot
#[derive(Default, Copy, Clone)]
pub struct SlotInfoInEpochInner {
    /// epoch of the slot
    epoch: Epoch,
    /// partition index of the slot within the epoch
    partition_index: PartitionIndex,
    /// number of slots in this epoch
    slots_in_epoch: Slot,
}

impl SlotInfoInEpoch {
    /// create, populating epoch info
    pub fn new(slot: Slot, epoch_schedule: &EpochSchedule) -> Self {
        let mut result = Self::new_small(slot);
        result.epoch_info = Some(result.get_epoch_info(epoch_schedule));
        result
    }
    /// create, without populating epoch info
    pub fn new_small(slot: Slot) -> Self {
        SlotInfoInEpoch {
            slot,
            ..SlotInfoInEpoch::default()
        }
    }
    /// get epoch info by returning already calculated or by calculating it now
    pub fn get_epoch_info(&self, epoch_schedule: &EpochSchedule) -> SlotInfoInEpochInner {
        if let Some(inner) = &self.epoch_info {
            *inner
        } else {
            let (epoch, partition_index) = epoch_schedule.get_epoch_and_slot_index(self.slot);
            SlotInfoInEpochInner {
                epoch,
                partition_index,
                slots_in_epoch: epoch_schedule.get_slots_in_epoch(epoch),
            }
        }
    }
}

impl ExpectedRentCollection {
    /// 'account' is being loaded from 'storage_slot' in 'bank_slot'
    /// adjusts 'account.rent_epoch' if we skipped the last rewrite on this account
    pub(crate) fn maybe_update_rent_epoch_on_load(
        account: &mut AccountSharedData,
        storage_slot: &SlotInfoInEpoch,
        bank_slot: &SlotInfoInEpoch,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        pubkey: &Pubkey,
        rewrites_skipped_this_slot: &Rewrites,
    ) {
        let result = Self::get_corrected_rent_epoch_on_load(
            account,
            storage_slot,
            bank_slot,
            epoch_schedule,
            rent_collector,
            pubkey,
            rewrites_skipped_this_slot,
        );
        if let Some(rent_epoch) = result {
            account.set_rent_epoch(rent_epoch);
        }
    }

    /// 'account' is being loaded
    /// we may need to adjust 'account.rent_epoch' if we skipped the last rewrite on this account
    /// returns Some(rent_epoch) if an adjustment needs to be made
    /// returns None if the account is up to date
    fn get_corrected_rent_epoch_on_load(
        account: &AccountSharedData,
        storage_slot: &SlotInfoInEpoch,
        bank_slot: &SlotInfoInEpoch,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        pubkey: &Pubkey,
        rewrites_skipped_this_slot: &Rewrites,
    ) -> Option<Epoch> {
        let next_epoch = match rent_collector.calculate_rent_result(pubkey, account, None) {
            RentResult::LeaveAloneNoRent => return None,
            RentResult::CollectRent {
                new_rent_epoch,
                rent_due: 0,
            } => new_rent_epoch,
            // Rent is due on this account in this epoch,
            // so we did not skip a rewrite.
            RentResult::CollectRent { .. } => return None,
        };
        {
            // grab epoch infno for bank slot and storage slot
            let bank_info = bank_slot.get_epoch_info(epoch_schedule);
            let (current_epoch, partition_from_current_slot) =
                (bank_info.epoch, bank_info.partition_index);
            let storage_info = storage_slot.get_epoch_info(epoch_schedule);
            let (storage_epoch, storage_slot_partition) =
                (storage_info.epoch, storage_info.partition_index);
            let partition_from_pubkey =
                Bank::partition_from_pubkey(pubkey, bank_info.slots_in_epoch);
            let mut possibly_update = true;
            if current_epoch == storage_epoch {
                // storage is in same epoch as bank
                if partition_from_pubkey > partition_from_current_slot {
                    // we haven't hit the slot's rent collection slot yet, and the storage was within this slot, so do not update
                    possibly_update = false;
                }
            } else if current_epoch == storage_epoch + 1 {
                // storage is in the previous epoch
                if storage_slot_partition >= partition_from_pubkey
                    && partition_from_pubkey > partition_from_current_slot
                {
                    // we did a rewrite in last epoch and we have not yet hit the rent collection slot in THIS epoch
                    possibly_update = false;
                }
            } // if more than 1 epoch old, then we need to collect rent because we clearly skipped it.

            let rewrites_skipped_this_pubkey_this_slot = || {
                rewrites_skipped_this_slot
                    .read()
                    .unwrap()
                    .contains_key(pubkey)
            };
            let rent_epoch = account.rent_epoch();
            if possibly_update && rent_epoch == 0 && current_epoch > 1 {
                if rewrites_skipped_this_pubkey_this_slot() {
                    return Some(next_epoch);
                } else {
                    // we know we're done
                    return None;
                }
            }

            // if an account was written >= its rent collection slot within the last epoch worth of slots, then we don't want to update it here
            if possibly_update && rent_epoch < current_epoch {
                let new_rent_epoch = if partition_from_pubkey < partition_from_current_slot
                    || (partition_from_pubkey == partition_from_current_slot
                        && rewrites_skipped_this_pubkey_this_slot())
                {
                    // partition_from_pubkey < partition_from_current_slot:
                    //  we already would have done a rewrite on this account IN this epoch
                    next_epoch
                } else {
                    // should have done rewrite up to last epoch
                    // we have not passed THIS epoch's rewrite slot yet, so the correct 'rent_epoch' is previous
                    next_epoch.saturating_sub(1)
                };
                if rent_epoch != new_rent_epoch {
                    // the point of this function:
                    // 'new_rent_epoch' is the correct rent_epoch that the account would have if we had done rewrites
                    return Some(new_rent_epoch);
                }
            } else if !possibly_update {
                // This is a non-trivial lookup. Would be nice to skip this.
                assert!(!rewrites_skipped_this_pubkey_this_slot(), "did not update rent_epoch: {}, new value for rent_epoch: {}, old: {}, current epoch: {}", pubkey, rent_epoch, next_epoch, current_epoch);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    /// it is possible 0.. rewrites were skipped on this account
    /// if so, return Some(correct hash as of 'storage_slot')
    /// if 'loaded_hash' is CORRECT, return None
    pub fn maybe_rehash_skipped_rewrite(
        loaded_account: &impl ReadableAccount,
        loaded_hash: &Hash,
        pubkey: &Pubkey,
        storage_slot: Slot,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        stats: &HashStats,
        max_slot_in_storages_inclusive: &SlotInfoInEpoch,
        find_unskipped_slot: impl Fn(Slot) -> Option<Slot>,
        filler_account_suffix: Option<&Pubkey>,
    ) -> Option<Hash> {
        use solana_measure::measure::Measure;
        let mut m = Measure::start("rehash_calc_us");
        let expected = ExpectedRentCollection::new(
            pubkey,
            loaded_account,
            storage_slot,
            epoch_schedule,
            rent_collector,
            max_slot_in_storages_inclusive,
            find_unskipped_slot,
            filler_account_suffix,
        );

        m.stop();
        stats.rehash_calc_us.fetch_add(m.as_us(), Ordering::Relaxed);
        let expected = match expected {
            None => {
                // use the previously calculated hash
                return None;
            }
            Some(expected) => expected,
        };
        let mut m = Measure::start("rehash_hash_us");
        let recalc_hash = AccountsDb::hash_account_with_rent_epoch(
            expected.expected_rent_collection_slot_max_epoch,
            loaded_account,
            pubkey,
            expected.rent_epoch,
        );
        m.stop();
        stats.rehash_hash_us.fetch_add(m.as_us(), Ordering::Relaxed);
        if &recalc_hash == loaded_hash {
            // unnecessary calculation occurred
            stats.rehash_unnecessary.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        stats.rehash_required.fetch_add(1, Ordering::Relaxed);

        // recomputed based on rent collection/rewrite slot
        // Rent would have been collected AT 'expected_rent_collection_slot', so hash according to that slot.
        // Note that a later storage (and slot) may contain this same pubkey. In that case, that newer hash will make this one irrelevant.
        Some(recalc_hash)
    }

    /// figure out whether the account stored at 'storage_slot' would have normally been rewritten at a slot that has already occurred: after 'storage_slot' but <= 'max_slot_in_storages_inclusive'
    /// returns Some(...) if the account would have normally been rewritten
    /// returns None if the account was updated wrt rent already or if it is known that there must exist a future rewrite of this account (for example, non-zero rent is due)
    fn new(
        pubkey: &Pubkey,
        loaded_account: &impl ReadableAccount,
        storage_slot: Slot,
        epoch_schedule: &EpochSchedule,
        rent_collector_max_epoch: &RentCollector,
        max_slot_in_storages_inclusive: &SlotInfoInEpoch,
        find_unskipped_slot: impl Fn(Slot) -> Option<Slot>,
        filler_account_suffix: Option<&Pubkey>,
    ) -> Option<Self> {
        let mut rent_collector = rent_collector_max_epoch;
        let SlotInfoInEpochInner {
            epoch: epoch_of_max_storage_slot,
            partition_index: partition_index_from_max_slot,
            slots_in_epoch: slots_per_epoch_max_epoch,
        } = max_slot_in_storages_inclusive.get_epoch_info(epoch_schedule);
        let mut partition_from_pubkey =
            crate::bank::Bank::partition_from_pubkey(pubkey, slots_per_epoch_max_epoch);
        // now, we have to find the root that is >= the slot where this pubkey's rent would have been collected
        let first_slot_in_max_epoch =
            max_slot_in_storages_inclusive.slot - partition_index_from_max_slot;
        let mut expected_rent_collection_slot_max_epoch =
            first_slot_in_max_epoch + partition_from_pubkey;
        let calculated_from_index_expected_rent_collection_slot_max_epoch =
            expected_rent_collection_slot_max_epoch;
        if expected_rent_collection_slot_max_epoch <= max_slot_in_storages_inclusive.slot {
            // may need to find a valid root
            if let Some(find) =
                find_unskipped_slot(calculated_from_index_expected_rent_collection_slot_max_epoch)
            {
                // found a root that is >= expected_rent_collection_slot.
                expected_rent_collection_slot_max_epoch = find;
            }
        }
        let mut use_previous_epoch_rent_collector = false;
        if expected_rent_collection_slot_max_epoch > max_slot_in_storages_inclusive.slot {
            // max slot has not hit the slot in the max epoch where we would have collected rent yet, so the most recent rent-collected rewrite slot for this pubkey would be in the previous epoch
            let previous_epoch = epoch_of_max_storage_slot.saturating_sub(1);
            let slots_per_epoch_previous_epoch = epoch_schedule.get_slots_in_epoch(previous_epoch);
            expected_rent_collection_slot_max_epoch =
                if slots_per_epoch_previous_epoch == slots_per_epoch_max_epoch {
                    // partition index remains the same
                    calculated_from_index_expected_rent_collection_slot_max_epoch
                        .saturating_sub(slots_per_epoch_max_epoch)
                } else {
                    // the newer epoch has a different # of slots, so the partition index will be different in the prior epoch
                    partition_from_pubkey = crate::bank::Bank::partition_from_pubkey(
                        pubkey,
                        slots_per_epoch_previous_epoch,
                    );
                    first_slot_in_max_epoch
                        .saturating_sub(slots_per_epoch_previous_epoch)
                        .saturating_add(partition_from_pubkey)
                };
            // since we are looking a different root, we have to call this again
            if let Some(find) = find_unskipped_slot(expected_rent_collection_slot_max_epoch) {
                // found a root (because we have a storage) that is >= expected_rent_collection_slot.
                expected_rent_collection_slot_max_epoch = find;
            }

            // since we have not hit the slot in the rent collector's epoch yet, we need to collect rent according to the previous epoch's rent collector.
            use_previous_epoch_rent_collector = true;
        }

        // the slot we're dealing with is where we expected the rent to be collected for this pubkey, so use what is in this slot
        // however, there are cases, such as adjusting the clock, where we store the account IN the same slot, but we do so BEFORE we collect rent. We later store the account AGAIN for rewrite/rent collection.
        // So, if storage_slot == expected_rent_collection_slot..., then we MAY have collected rent or may not have. So, it has to be >
        // rent_epoch=0 is a special case
        if storage_slot > expected_rent_collection_slot_max_epoch
            || loaded_account.rent_epoch() == 0
        {
            // no need to update hash
            return None;
        }

        let rent_collector_previous;
        if use_previous_epoch_rent_collector {
            // keep in mind the storage slot could be 0..inf epochs in the past
            // we want to swap the rent collector for one whose epoch is the previous epoch
            let mut rent_collector_temp = rent_collector.clone();
            rent_collector_temp.epoch = rent_collector.epoch.saturating_sub(1); // previous epoch
            rent_collector_previous = Some(rent_collector_temp);
            rent_collector = rent_collector_previous.as_ref().unwrap();
        }

        // ask the rent collector what rent should be collected.
        // Rent collector knows the current epoch.
        let rent_result =
            rent_collector.calculate_rent_result(pubkey, loaded_account, filler_account_suffix);
        let current_rent_epoch = loaded_account.rent_epoch();
        let new_rent_epoch = match rent_result {
            RentResult::CollectRent {
                new_rent_epoch: next_epoch,
                rent_due,
            } => {
                if next_epoch > current_rent_epoch && rent_due != 0 {
                    // this is an account that would have had rent collected since this storage slot, so just use the hash we have since there must be a newer version of this account already in a newer slot
                    // It would be a waste of time to recalcluate a hash.
                    return None;
                }
                std::cmp::max(next_epoch, current_rent_epoch)
            }
            RentResult::LeaveAloneNoRent => {
                // rent_epoch is not updated for this condition
                // But, a rewrite WOULD HAVE occured at the expected slot.
                // So, fall through with same rent_epoch, but we will have already calculated 'expected_rent_collection_slot_max_epoch'
                current_rent_epoch
            }
        };

        if expected_rent_collection_slot_max_epoch == storage_slot
            && new_rent_epoch == loaded_account.rent_epoch()
        {
            // no rewrite would have occurred
            return None;
        }

        Some(Self {
            partition_from_pubkey,
            epoch_of_max_storage_slot,
            partition_index_from_max_slot,
            first_slot_in_max_epoch,
            expected_rent_collection_slot_max_epoch,
            rent_epoch: new_rent_epoch,
        })
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_sdk::{
            account::{AccountSharedData, WritableAccount},
            genesis_config::GenesisConfig,
        },
    };

    #[test]
    fn test_expected_rent_collection() {
        solana_logger::setup();
        let pubkey = Pubkey::new(&[5; 32]);
        let owner = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(1, 0, &owner);
        let max_slot_in_storages_inclusive = 0;
        let epoch_schedule = EpochSchedule::default();
        let first_normal_slot = epoch_schedule.first_normal_slot;
        let storage_slot = first_normal_slot;
        let epoch = epoch_schedule.get_epoch(storage_slot);
        assert_eq!(
            (epoch, 0),
            epoch_schedule.get_epoch_and_slot_index(storage_slot)
        );
        let genesis_config = GenesisConfig::default();
        let mut rent_collector = RentCollector::new(
            epoch,
            epoch_schedule,
            genesis_config.slots_per_year(),
            genesis_config.rent,
        );
        rent_collector.rent.lamports_per_byte_year = 0; // temporarily disable rent
        let find_unskipped_slot = Some;
        // slot in current epoch
        let result = ExpectedRentCollection::new(
            &pubkey,
            &account,
            storage_slot,
            &epoch_schedule,
            &rent_collector,
            &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
            find_unskipped_slot,
            None,
        );
        assert!(result.is_none());

        let slots_per_epoch = 432_000;
        assert_eq!(
            slots_per_epoch,
            epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(storage_slot))
        );
        let partition_index_max_inclusive = slots_per_epoch - 1;
        account.set_rent_epoch(rent_collector.epoch);
        // several epochs ahead of now
        // first slot of new epoch is max slot EXclusive
        // so last slot of prior epoch is max slot INclusive
        let max_slot_in_storages_inclusive = slots_per_epoch * 3 + first_normal_slot - 1;
        rent_collector.epoch = epoch_schedule.get_epoch(max_slot_in_storages_inclusive);
        let partition_from_pubkey = 8470; // function of 432k slots and 'pubkey' above
        let first_slot_in_max_epoch = 1388256;
        let expected_rent_collection_slot_max_epoch =
            first_slot_in_max_epoch + partition_from_pubkey;
        let result = ExpectedRentCollection::new(
            &pubkey,
            &account,
            storage_slot,
            &epoch_schedule,
            &rent_collector,
            &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
            find_unskipped_slot,
            None,
        );
        assert_eq!(
            result,
            Some(ExpectedRentCollection {
                partition_from_pubkey,
                epoch_of_max_storage_slot: rent_collector.epoch,
                partition_index_from_max_slot: partition_index_max_inclusive,
                first_slot_in_max_epoch,
                expected_rent_collection_slot_max_epoch,
                rent_epoch: rent_collector.epoch,
            })
        );

        // LeaveAloneNoRent
        for leave_alone in [true, false] {
            account.set_executable(leave_alone);
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch,
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                result,
                (!leave_alone).then(|| ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: rent_collector.epoch,
                }),
                "leave_alone: {}",
                leave_alone
            );
        }

        // storage_slot > expected_rent_collection_slot_max_epoch
        // if greater, we return None
        for greater in [false, true] {
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch + if greater { 1 } else { 0 },
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                result,
                (!greater).then(|| ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: rent_collector.epoch,
                })
            );
        }

        // test rewrite would have occurred in previous epoch from max_slot_in_storages_inclusive's epoch
        // the change is in 'rent_epoch' returned in 'expected'
        for previous_epoch in [false, true] {
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch,
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(
                    max_slot_in_storages_inclusive
                        + if previous_epoch { slots_per_epoch } else { 0 },
                    &epoch_schedule,
                ),
                find_unskipped_slot,
                None,
            );
            let epoch_delta = if previous_epoch { 1 } else { 0 };
            let slot_delta = epoch_delta * slots_per_epoch;
            assert_eq!(
                result,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch + epoch_delta,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch: first_slot_in_max_epoch + slot_delta,
                    expected_rent_collection_slot_max_epoch: expected_rent_collection_slot_max_epoch
                        + slot_delta,
                    rent_epoch: rent_collector.epoch,
                }),
                "previous_epoch: {}",
                previous_epoch,
            );
        }

        // if account's rent_epoch is already > our rent epoch, rent was collected already
        // if greater, we return None
        let original_rent_epoch = account.rent_epoch();
        for already_collected in [true, false] {
            // to consider: maybe if we already collected rent_epoch IN this slot and slot matches what we need, then we should return None here
            account.set_rent_epoch(original_rent_epoch + if already_collected { 1 } else { 0 });
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch,
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                result,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: std::cmp::max(rent_collector.epoch, account.rent_epoch()),
                }),
                "rent_collector.epoch: {}, already_collected: {}",
                rent_collector.epoch,
                already_collected
            );
        }
        account.set_rent_epoch(original_rent_epoch);

        let storage_slot = max_slot_in_storages_inclusive - slots_per_epoch;
        // check partition from pubkey code
        for end_partition_index in [0, 1, 2, 100, slots_per_epoch - 2, slots_per_epoch - 1] {
            // generate a pubkey range
            let range = crate::bank::Bank::pubkey_range_from_partition((
                // start_index:
                end_partition_index.saturating_sub(1), // this can end up at start=0, end=0 (this is a desired test case)
                // end_index:
                end_partition_index,
                epoch_schedule.get_slots_in_epoch(rent_collector.epoch),
            ));
            // use both start and end from INclusive range separately
            for pubkey in [&range.start(), &range.end()] {
                let result = ExpectedRentCollection::new(
                    pubkey,
                    &account,
                    storage_slot,
                    &epoch_schedule,
                    &rent_collector,
                    &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                    find_unskipped_slot,
                    None,
                );
                assert_eq!(
                    result,
                    Some(ExpectedRentCollection {
                        partition_from_pubkey: end_partition_index,
                        epoch_of_max_storage_slot: rent_collector.epoch,
                        partition_index_from_max_slot: partition_index_max_inclusive,
                        first_slot_in_max_epoch,
                        expected_rent_collection_slot_max_epoch: first_slot_in_max_epoch + end_partition_index,
                        rent_epoch: rent_collector.epoch,
                    }),
                    "range: {:?}, pubkey: {:?}, end_partition_index: {}, max_slot_in_storages_inclusive: {}",
                    range,
                    pubkey,
                    end_partition_index,
                    max_slot_in_storages_inclusive,
                );
            }
        }

        // check max_slot_in_storages_inclusive related code
        // so sweep through max_slot_in_storages_inclusive values within an epoch
        let first_slot_in_max_epoch = first_normal_slot + slots_per_epoch;
        rent_collector.epoch = epoch_schedule.get_epoch(first_slot_in_max_epoch);
        // an epoch in the past so we always collect rent
        let storage_slot = first_normal_slot;
        for partition_index in [
            0,
            1,
            2,
            partition_from_pubkey - 1,
            partition_from_pubkey,
            partition_from_pubkey + 1,
            100,
            slots_per_epoch - 2,
            slots_per_epoch - 1,
        ] {
            // partition_index=0 means first slot of second normal epoch
            // second normal epoch because we want to deal with accounts stored in the first normal epoch
            // + 1 because of exclusive
            let max_slot_in_storages_inclusive = first_slot_in_max_epoch + partition_index;
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                storage_slot,
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                find_unskipped_slot,
                None,
            );
            let partition_index_passed_pubkey = partition_from_pubkey <= partition_index;
            let expected_rent_epoch =
                rent_collector.epoch - if partition_index_passed_pubkey { 0 } else { 1 };
            let expected_rent_collection_slot_max_epoch = first_slot_in_max_epoch
                + partition_from_pubkey
                - if partition_index_passed_pubkey {
                    0
                } else {
                    slots_per_epoch
                };

            assert_eq!(
                result,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: expected_rent_epoch,
                }),
                "partition_index: {}, max_slot_in_storages_inclusive: {}, storage_slot: {}, first_normal_slot: {}",
                partition_index,
                max_slot_in_storages_inclusive,
                storage_slot,
                first_normal_slot,
            );
        }

        // test account.rent_epoch = 0
        let first_slot_in_max_epoch = 1388256;
        for account_rent_epoch in [0, epoch] {
            account.set_rent_epoch(account_rent_epoch);
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                storage_slot,
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                result,
                (account_rent_epoch != 0).then(|| ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch + 1,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: rent_collector.epoch,
                })
            );
        }

        // test find_unskipped_slot
        for find_unskipped_slot in [
            |_| None,
            Some,                  // identity
            |slot| Some(slot + 1), // increment
            |_| Some(Slot::MAX),   // max
        ] {
            let test_value = 10;
            let find_result = find_unskipped_slot(test_value);
            let increment = find_result.unwrap_or_default() == test_value + 1;
            let result = ExpectedRentCollection::new(
                &pubkey,
                &account,
                storage_slot,
                &epoch_schedule,
                &rent_collector,
                &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                find_unskipped_slot,
                None,
            );
            // the test case of max is hacky
            let prior_epoch = (partition_from_pubkey > partition_index_max_inclusive)
                || find_unskipped_slot(0) == Some(Slot::MAX);
            assert_eq!(
                result,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch + 1,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch: if find_result.unwrap_or_default()
                        == Slot::MAX
                    {
                        Slot::MAX
                    } else if increment {
                        expected_rent_collection_slot_max_epoch + 1
                    } else {
                        expected_rent_collection_slot_max_epoch
                    },
                    rent_epoch: rent_collector.epoch - if prior_epoch { 1 } else { 0 },
                }),
                "find_unskipped_slot(0): {:?}, rent_collector.epoch: {}, prior_epoch: {}",
                find_unskipped_slot(0),
                rent_collector.epoch,
                prior_epoch,
            );
        }
    }

    #[test]
    fn test_simplified_rent_collection() {
        solana_logger::setup();
        let pubkey = Pubkey::new(&[5; 32]);
        let owner = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(1, 0, &owner);
        let mut epoch_schedule = EpochSchedule {
            first_normal_epoch: 0,
            ..EpochSchedule::default()
        };
        epoch_schedule.first_normal_slot = 0;
        let first_normal_slot = epoch_schedule.first_normal_slot;
        let slots_per_epoch = 432_000;
        let partition_from_pubkey = 8470; // function of 432k slots and 'pubkey' above
                                          // start in epoch=1 because of issues at rent_epoch=1
        let storage_slot = first_normal_slot + partition_from_pubkey + slots_per_epoch;
        let epoch = epoch_schedule.get_epoch(storage_slot);
        assert_eq!(
            (epoch, partition_from_pubkey),
            epoch_schedule.get_epoch_and_slot_index(storage_slot)
        );
        let genesis_config = GenesisConfig::default();
        let mut rent_collector = RentCollector::new(
            epoch,
            epoch_schedule,
            genesis_config.slots_per_year(),
            genesis_config.rent,
        );
        rent_collector.rent.lamports_per_byte_year = 0; // temporarily disable rent

        assert_eq!(
            slots_per_epoch,
            epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(storage_slot))
        );
        account.set_rent_epoch(1); // has to be not 0

        /*
        test this:
        pubkey_partition_index: 8470
        storage_slot: 8470
        account.rent_epoch: 1 (has to be not 0)

        max_slot: 8469 + 432k * 1
        max_slot: 8470 + 432k * 1
        max_slot: 8471 + 432k * 1
        max_slot: 8472 + 432k * 1
        max_slot: 8469 + 432k * 2
        max_slot: 8470 + 432k * 2
        max_slot: 8471 + 432k * 2
        max_slot: 8472 + 432k * 2
        max_slot: 8469 + 432k * 3
        max_slot: 8470 + 432k * 3
        max_slot: 8471 + 432k * 3
        max_slot: 8472 + 432k * 3

        one run without skipping slot 8470, once WITH skipping slot 8470
        */

        for skipped_slot in [false, true] {
            let find_unskipped_slot = if skipped_slot {
                |slot| Some(slot + 1)
            } else {
                Some
            };

            // starting at epoch = 0 has issues because of rent_epoch=0 special casing
            for epoch in 1..4 {
                for partition_index_from_max_slot in
                    partition_from_pubkey - 1..=partition_from_pubkey + 2
                {
                    let max_slot_in_storages_inclusive =
                        slots_per_epoch * epoch + first_normal_slot + partition_index_from_max_slot;
                    if storage_slot > max_slot_in_storages_inclusive {
                        continue; // illegal combination
                    }
                    rent_collector.epoch = epoch_schedule.get_epoch(max_slot_in_storages_inclusive);
                    let first_slot_in_max_epoch = max_slot_in_storages_inclusive
                        - max_slot_in_storages_inclusive % slots_per_epoch;
                    let skip_offset = if skipped_slot { 1 } else { 0 };
                    let mut expected_rent_collection_slot_max_epoch =
                        first_slot_in_max_epoch + partition_from_pubkey + skip_offset;
                    let hit_this_epoch =
                        expected_rent_collection_slot_max_epoch <= max_slot_in_storages_inclusive;
                    if !hit_this_epoch {
                        expected_rent_collection_slot_max_epoch -= slots_per_epoch;
                    }

                    assert_eq!(
                        (epoch, partition_index_from_max_slot),
                        epoch_schedule.get_epoch_and_slot_index(max_slot_in_storages_inclusive)
                    );
                    assert_eq!(
                        (epoch, 0),
                        epoch_schedule.get_epoch_and_slot_index(first_slot_in_max_epoch)
                    );
                    account.set_rent_epoch(1);
                    let result = ExpectedRentCollection::new(
                        &pubkey,
                        &account,
                        storage_slot,
                        &epoch_schedule,
                        &rent_collector,
                        &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                        find_unskipped_slot,
                        None,
                    );
                    let some_expected = if epoch == 1 {
                        skipped_slot && partition_index_from_max_slot > partition_from_pubkey
                    } else if epoch == 2 {
                        partition_index_from_max_slot >= partition_from_pubkey - skip_offset
                    } else {
                        true
                    };
                    assert_eq!(
                        result,
                        some_expected.then(|| ExpectedRentCollection {
                            partition_from_pubkey,
                            epoch_of_max_storage_slot: rent_collector.epoch,
                            partition_index_from_max_slot,
                            first_slot_in_max_epoch,
                            expected_rent_collection_slot_max_epoch,
                            rent_epoch: rent_collector.epoch - if hit_this_epoch { 0 } else {1},
                        }),
                        "partition_index_from_max_slot: {}, epoch: {}, hit_this_epoch: {}, skipped_slot: {}",
                        partition_index_from_max_slot,
                        epoch,
                        hit_this_epoch,
                        skipped_slot,
                    );

                    // test RentResult::LeaveAloneNoRent
                    {
                        let result = ExpectedRentCollection::new(
                            &pubkey,
                            &account,
                            storage_slot,
                            &epoch_schedule,
                            &rent_collector,
                            &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                            find_unskipped_slot,
                            // treat this pubkey like a filler account so we get a 'LeaveAloneNoRent' result
                            Some(&pubkey),
                        );
                        assert_eq!(
                            result,
                            some_expected.then(|| ExpectedRentCollection {
                                partition_from_pubkey,
                                epoch_of_max_storage_slot: rent_collector.epoch,
                                partition_index_from_max_slot,
                                first_slot_in_max_epoch,
                                expected_rent_collection_slot_max_epoch,
                                // this will not be adjusted for 'LeaveAloneNoRent'
                                rent_epoch: account.rent_epoch(),
                            }),
                            "partition_index_from_max_slot: {}, epoch: {}",
                            partition_index_from_max_slot,
                            epoch,
                        );
                    }

                    // test maybe_rehash_skipped_rewrite
                    let hash = AccountsDb::hash_account(storage_slot, &account, &pubkey);
                    let maybe_rehash = ExpectedRentCollection::maybe_rehash_skipped_rewrite(
                        &account,
                        &hash,
                        &pubkey,
                        storage_slot,
                        &epoch_schedule,
                        &rent_collector,
                        &HashStats::default(),
                        &SlotInfoInEpoch::new(max_slot_in_storages_inclusive, &epoch_schedule),
                        find_unskipped_slot,
                        None,
                    );
                    assert_eq!(
                        maybe_rehash,
                        some_expected.then(|| {
                            AccountsDb::hash_account_with_rent_epoch(
                                result
                                    .as_ref()
                                    .unwrap()
                                    .expected_rent_collection_slot_max_epoch,
                                &account,
                                &pubkey,
                                result.as_ref().unwrap().rent_epoch,
                            )
                        })
                    );
                }
            }
        }
    }

    #[test]
    fn test_get_corrected_rent_epoch_on_load() {
        solana_logger::setup();
        let pubkey = Pubkey::new(&[5; 32]);
        let owner = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(1, 0, &owner);
        let mut epoch_schedule = EpochSchedule {
            first_normal_epoch: 0,
            ..EpochSchedule::default()
        };
        epoch_schedule.first_normal_slot = 0;
        let first_normal_slot = epoch_schedule.first_normal_slot;
        let slots_per_epoch = 432_000;
        let partition_from_pubkey = 8470; // function of 432k slots and 'pubkey' above
                                          // start in epoch=1 because of issues at rent_epoch=1
        let storage_slot = first_normal_slot + partition_from_pubkey + slots_per_epoch;
        let epoch = epoch_schedule.get_epoch(storage_slot);
        assert_eq!(
            (epoch, partition_from_pubkey),
            epoch_schedule.get_epoch_and_slot_index(storage_slot)
        );
        let genesis_config = GenesisConfig::default();
        let mut rent_collector = RentCollector::new(
            epoch,
            epoch_schedule,
            genesis_config.slots_per_year(),
            genesis_config.rent,
        );
        rent_collector.rent.lamports_per_byte_year = 0; // temporarily disable rent

        assert_eq!(
            slots_per_epoch,
            epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(storage_slot))
        );
        account.set_rent_epoch(1); // has to be not 0

        /*
        test this:
        pubkey_partition_index: 8470
        storage_slot: 8470
        account.rent_epoch: 1 (has to be not 0)

        max_slot: 8469 + 432k * 1
        max_slot: 8470 + 432k * 1
        max_slot: 8471 + 432k * 1
        max_slot: 8472 + 432k * 1
        max_slot: 8469 + 432k * 2
        max_slot: 8470 + 432k * 2
        max_slot: 8471 + 432k * 2
        max_slot: 8472 + 432k * 2
        max_slot: 8469 + 432k * 3
        max_slot: 8470 + 432k * 3
        max_slot: 8471 + 432k * 3
        max_slot: 8472 + 432k * 3

        one run without skipping slot 8470, once WITH skipping slot 8470
        */

        for new_small in [false, true] {
            for rewrite_already in [false, true] {
                // starting at epoch = 0 has issues because of rent_epoch=0 special casing
                for epoch in 1..4 {
                    for partition_index_bank_slot in
                        partition_from_pubkey - 1..=partition_from_pubkey + 2
                    {
                        let bank_slot =
                            slots_per_epoch * epoch + first_normal_slot + partition_index_bank_slot;
                        if storage_slot > bank_slot {
                            continue; // illegal combination
                        }
                        rent_collector.epoch = epoch_schedule.get_epoch(bank_slot);
                        let first_slot_in_max_epoch = bank_slot - bank_slot % slots_per_epoch;

                        assert_eq!(
                            (epoch, partition_index_bank_slot),
                            epoch_schedule.get_epoch_and_slot_index(bank_slot)
                        );
                        assert_eq!(
                            (epoch, 0),
                            epoch_schedule.get_epoch_and_slot_index(first_slot_in_max_epoch)
                        );
                        account.set_rent_epoch(1);
                        let rewrites = Rewrites::default();
                        if rewrite_already {
                            if partition_index_bank_slot != partition_from_pubkey {
                                // this is an invalid test occurrence.
                                // we wouldn't have inserted pubkey into 'rewrite_already' for this slot if the current partition index wasn't at the pubkey's partition idnex yet.
                                continue;
                            }

                            rewrites.write().unwrap().insert(pubkey, Hash::default());
                        }
                        let expected_new_rent_epoch =
                            if partition_index_bank_slot > partition_from_pubkey {
                                if epoch > account.rent_epoch() {
                                    Some(rent_collector.epoch)
                                } else {
                                    None
                                }
                            } else if partition_index_bank_slot == partition_from_pubkey
                                && rewrite_already
                            {
                                let expected_rent_epoch = rent_collector.epoch;
                                if expected_rent_epoch == account.rent_epoch() {
                                    None
                                } else {
                                    Some(expected_rent_epoch)
                                }
                            } else if partition_index_bank_slot <= partition_from_pubkey
                                && epoch > account.rent_epoch()
                            {
                                let expected_rent_epoch = rent_collector.epoch.saturating_sub(1);
                                if expected_rent_epoch == account.rent_epoch() {
                                    None
                                } else {
                                    Some(expected_rent_epoch)
                                }
                            } else {
                                None
                            };
                        let get_slot_info = |slot| {
                            if new_small {
                                SlotInfoInEpoch::new_small(slot)
                            } else {
                                SlotInfoInEpoch::new(slot, &epoch_schedule)
                            }
                        };
                        let new_rent_epoch =
                            ExpectedRentCollection::get_corrected_rent_epoch_on_load(
                                &account,
                                &get_slot_info(storage_slot),
                                &get_slot_info(bank_slot),
                                &epoch_schedule,
                                &rent_collector,
                                &pubkey,
                                &rewrites,
                            );
                        assert_eq!(new_rent_epoch, expected_new_rent_epoch);
                    }
                }
            }
        }
    }
}
