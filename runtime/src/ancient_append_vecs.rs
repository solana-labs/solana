//! helpers for squashing append vecs into ancient append vecs
//! an ancient append vec is:
//! 1. a slot that is older than an epoch old
//! 2. multiple 'slots' squashed into a single older (ie. ancient) slot for convenience and performance
//! Otherwise, an ancient append vec is the same as any other append vec
use {
    crate::{
        account_storage::{meta::StoredAccountMeta, ShrinkInProgress},
        accounts_db::{
            AccountStorageEntry, AccountsDb, AliveAccounts, GetUniqueAccountsResult, ShrinkCollect,
            ShrinkCollectAliveSeparatedByRefs, ShrinkStatsSub, StoreReclaims,
            INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
        },
        accounts_file::AccountsFile,
        accounts_index::ZeroLamport,
        active_stats::ActiveStatItem,
        append_vec::aligned_stored_size,
        storable_accounts::{StorableAccounts, StorableAccountsBySlot},
    },
    rand::{thread_rng, Rng},
    solana_measure::measure_us,
    solana_sdk::{account::ReadableAccount, clock::Slot, hash::Hash, saturating_add_assign},
    std::{
        collections::HashMap,
        num::NonZeroU64,
        sync::{atomic::Ordering, Arc},
    },
};

/// ancient packing algorithm tuning per pass
#[derive(Debug)]
#[allow(dead_code)]
struct PackedAncientStorageTuning {
    /// shrink enough of these ancient append vecs to realize this% of the total dead data that needs to be shrunk
    /// Doing too much burns too much time and disk i/o.
    /// Doing too little could cause us to never catch up and have old data accumulate.
    percent_of_alive_shrunk_data: u64,
    /// number of ancient slots we should aim to have. If we have more than this, combine further.
    max_ancient_slots: usize,
    /// # of bytes in an ideal ancient storage size
    ideal_storage_size: NonZeroU64,
    /// true if storages can be randomly shrunk even if they aren't eligible
    can_randomly_shrink: bool,
}

/// info about a storage eligible to be combined into an ancient append vec.
/// Useful to help sort vecs of storages.
#[derive(Debug)]
#[allow(dead_code)]
struct SlotInfo {
    storage: Arc<AccountStorageEntry>,
    /// slot of storage
    slot: Slot,
    /// total capacity of storage
    capacity: u64,
    /// # alive bytes in storage
    alive_bytes: u64,
    /// true if this should be shrunk due to ratio
    should_shrink: bool,
}

/// info for all storages in ancient slots
/// 'all_infos' contains all slots and storages that are ancient
#[derive(Default, Debug)]
struct AncientSlotInfos {
    /// info on all ancient storages
    all_infos: Vec<SlotInfo>,
    /// indexes to 'all_info' for storages that should be shrunk because alive ratio is too low.
    /// subset of all_infos
    shrink_indexes: Vec<usize>,
    /// total alive bytes across contents of 'shrink_indexes'
    total_alive_bytes_shrink: u64,
    /// total alive bytes across all slots
    total_alive_bytes: u64,
}

impl AncientSlotInfos {
    /// add info for 'storage'
    fn add(&mut self, slot: Slot, storage: Arc<AccountStorageEntry>, can_randomly_shrink: bool) {
        let alive_bytes = storage.alive_bytes() as u64;
        if alive_bytes > 0 {
            let capacity = storage.accounts.capacity();
            let should_shrink = if capacity > 0 {
                let alive_ratio = alive_bytes * 100 / capacity;
                (alive_ratio < 90) || (can_randomly_shrink && thread_rng().gen_range(0, 10000) == 0)
            } else {
                false
            };
            // two criteria we're shrinking by later:
            // 1. alive ratio so that we don't consume too much disk space with dead accounts
            // 2. # of active ancient roots, so that we don't consume too many open file handles

            if should_shrink {
                // alive ratio is too low, so prioritize combining this slot with others
                // to reduce disk space used
                saturating_add_assign!(self.total_alive_bytes_shrink, alive_bytes);
                self.shrink_indexes.push(self.all_infos.len());
            }
            self.all_infos.push(SlotInfo {
                slot,
                capacity,
                storage,
                alive_bytes,
                should_shrink,
            });
            saturating_add_assign!(self.total_alive_bytes, alive_bytes);
        }
    }

    /// modify 'self' to contain only the slot infos for the slots that should be combined
    /// (and in this process effectively shrunk)
    #[allow(dead_code)]
    fn filter_ancient_slots(&mut self, tuning: &PackedAncientStorageTuning) {
        // figure out which slots to combine
        // 1. should_shrink: largest bytes saved above some cutoff of ratio
        self.choose_storages_to_shrink(tuning.percent_of_alive_shrunk_data);
        // 2. smallest files so we get the largest number of files to remove
        self.filter_by_smallest_capacity(tuning.max_ancient_slots, tuning.ideal_storage_size);
    }

    // sort 'shrink_indexes' by most bytes saved, highest to lowest
    #[allow(dead_code)]
    fn sort_shrink_indexes_by_bytes_saved(&mut self) {
        self.shrink_indexes.sort_unstable_by(|l, r| {
            let amount_shrunk = |index: &usize| {
                let item = &self.all_infos[*index];
                item.capacity - item.alive_bytes
            };
            amount_shrunk(r).cmp(&amount_shrunk(l))
        });
    }

    /// clear 'should_shrink' for storages after a cutoff to limit how many storages we shrink
    #[allow(dead_code)]
    fn clear_should_shrink_after_cutoff(&mut self, percent_of_alive_shrunk_data: u64) {
        let mut bytes_to_shrink_due_to_ratio = 0;
        // shrink enough slots to write 'percent_of_alive_shrunk_data'% of the total alive data
        // from slots that exceeded the shrink threshold.
        // The goal is to limit overall i/o in this pass while making progress.
        let threshold_bytes = self.total_alive_bytes_shrink * percent_of_alive_shrunk_data / 100;
        for info_index in &self.shrink_indexes {
            let info = &mut self.all_infos[*info_index];
            if bytes_to_shrink_due_to_ratio >= threshold_bytes {
                // we exceeded the amount to shrink due to alive ratio, so don't shrink this one just due to 'should_shrink'
                // It MAY be shrunk based on total capacity still.
                // Mark it as false for 'should_shrink' so it gets evaluated solely based on # of files.
                info.should_shrink = false;
            } else {
                saturating_add_assign!(bytes_to_shrink_due_to_ratio, info.alive_bytes);
            }
        }
    }

    /// after this function, only slots that were chosen to shrink are marked with
    /// 'should_shrink'
    /// There are likely more candidates to shrink than will be chosen.
    #[allow(dead_code)]
    fn choose_storages_to_shrink(&mut self, percent_of_alive_shrunk_data: u64) {
        // sort the shrink_ratio_slots by most bytes saved to fewest
        // most bytes saved is more valuable to shrink
        self.sort_shrink_indexes_by_bytes_saved();

        self.clear_should_shrink_after_cutoff(percent_of_alive_shrunk_data);
    }

    /// truncate 'all_infos' such that when the remaining entries in
    /// 'all_infos' are combined, the total number of storages <= 'max_storages'
    /// The idea is that 'all_infos' is sorted from smallest capacity to largest,
    /// but that isn't required for this function to be 'correct'.
    #[allow(dead_code)]
    fn truncate_to_max_storages(&mut self, max_storages: usize, ideal_storage_size: NonZeroU64) {
        // these indexes into 'all_infos' are useless once we truncate 'all_infos', so make sure they're cleared out to avoid any issues
        self.shrink_indexes.clear();
        let total_storages = self.all_infos.len();
        let mut cumulative_bytes = 0u64;
        for (i, info) in self.all_infos.iter().enumerate() {
            saturating_add_assign!(cumulative_bytes, info.alive_bytes);
            let ancient_storages_required = (cumulative_bytes / ideal_storage_size + 1) as usize;
            let storages_remaining = total_storages - i - 1;
            // if the remaining uncombined storages and the # of resulting
            // combined ancient storages is less than the threshold, then
            // we've gone too far, so get rid of this entry and all after it.
            // Every storage after this one is larger.
            if storages_remaining + ancient_storages_required < max_storages {
                self.all_infos.truncate(i);
                break;
            }
        }
    }

    /// remove entries from 'all_infos' such that combining
    /// the remaining entries into storages of 'ideal_storage_size'
    /// will get us below 'max_storages'
    /// The entires that are removed will be reconsidered the next time around.
    /// Combining too many storages costs i/o and cpu so the goal is to find the sweet spot so
    /// that we make progress in cleaning/shrinking/combining but that we don't cause unnecessary
    /// churn.
    #[allow(dead_code)]
    fn filter_by_smallest_capacity(&mut self, max_storages: usize, ideal_storage_size: NonZeroU64) {
        let total_storages = self.all_infos.len();
        if total_storages <= max_storages {
            // currently fewer storages than max, so nothing to shrink
            self.shrink_indexes.clear();
            self.all_infos.clear();
            return;
        }

        // sort by 'should_shrink' then smallest capacity to largest
        self.all_infos.sort_unstable_by(|l, r| {
            r.should_shrink
                .cmp(&l.should_shrink)
                .then_with(|| l.capacity.cmp(&r.capacity))
        });

        // remove any storages we don't need to combine this pass to achieve
        // # resulting storages <= 'max_storages'
        self.truncate_to_max_storages(max_storages, ideal_storage_size);
    }
}

/// Used to hold the result of writing a single ancient storage
/// and results of writing multiple ancient storages
#[derive(Debug, Default)]
#[allow(dead_code)]
struct WriteAncientAccounts<'a> {
    /// 'ShrinkInProgress' instances created by starting a shrink operation
    shrinks_in_progress: HashMap<Slot, ShrinkInProgress<'a>>,

    metrics: ShrinkStatsSub,
}

impl AccountsDb {
    #[allow(dead_code)]
    /// Combine account data from storages in 'sorted_slots' into packed storages.
    /// This keeps us from accumulating storages for each slot older than an epoch.
    /// Ater this function the number of alive roots is <= # alive roots when it was called.
    /// In practice, the # of alive roots after will be significantly less than # alive roots when called.
    /// Trying to reduce # roots and storages (one per root) required to store all the data in ancient slots
    pub(crate) fn combine_ancient_slots_packed(
        &self,
        sorted_slots: Vec<Slot>,
        can_randomly_shrink: bool,
    ) {
        let tuning = PackedAncientStorageTuning {
            // only allow 10k slots old enough to be ancient
            max_ancient_slots: 10_000,
            // re-combine/shrink 55% of the data savings this pass
            percent_of_alive_shrunk_data: 55,
            ideal_storage_size: NonZeroU64::new(get_ancient_append_vec_capacity()).unwrap(),
            can_randomly_shrink,
        };

        let _guard = self.active_stats.activate(ActiveStatItem::SquashAncient);

        let mut stats_sub = ShrinkStatsSub::default();

        let (_, total_us) = measure_us!(self.combine_ancient_slots_packed_internal(
            sorted_slots,
            tuning,
            &mut stats_sub
        ));

        Self::update_shrink_stats(&self.shrink_ancient_stats.shrink_stats, stats_sub);
        self.shrink_ancient_stats
            .total_us
            .fetch_add(total_us, Ordering::Relaxed);

        // only log when we've spent 1s total
        // results will continue to accumulate otherwise
        if self.shrink_ancient_stats.total_us.load(Ordering::Relaxed) > 1_000_000 {
            self.shrink_ancient_stats.report();
        }
    }

    #[allow(dead_code)]
    fn combine_ancient_slots_packed_internal(
        &self,
        sorted_slots: Vec<Slot>,
        tuning: PackedAncientStorageTuning,
        metrics: &mut ShrinkStatsSub,
    ) {
        let ancient_slot_infos = self.collect_sort_filter_ancient_slots(sorted_slots, &tuning);

        if ancient_slot_infos.all_infos.is_empty() {
            return; // nothing to do
        }
        let accounts_per_storage = self
            .get_unique_accounts_from_storage_for_combining_ancient_slots(
                &ancient_slot_infos.all_infos[..],
            );

        let accounts_to_combine = self.calc_accounts_to_combine(&accounts_per_storage);

        // pack the accounts with 1 ref
        let pack = PackedAncientStorage::pack(
            accounts_to_combine
                .accounts_to_combine
                .iter()
                .map(|shrink_collect| &shrink_collect.alive_accounts.one_ref),
            tuning.ideal_storage_size,
        );

        if pack.len() > accounts_to_combine.target_slots_sorted.len() {
            return; // not enough slots to contain the storages we are trying to pack
        }

        let write_ancient_accounts = self.write_packed_storages(&accounts_to_combine, pack);

        self.finish_combine_ancient_slots_packed_internal(
            accounts_to_combine,
            write_ancient_accounts,
            metrics,
        );
    }

    /// calculate all storage info for the storages in slots
    /// Then, apply 'tuning' to filter out slots we do NOT want to combine.
    #[allow(dead_code)]
    fn collect_sort_filter_ancient_slots(
        &self,
        slots: Vec<Slot>,
        tuning: &PackedAncientStorageTuning,
    ) -> AncientSlotInfos {
        let mut ancient_slot_infos = self.calc_ancient_slot_info(slots, tuning.can_randomly_shrink);

        ancient_slot_infos.filter_ancient_slots(tuning);
        ancient_slot_infos
    }

    /// create append vec of size 'bytes'
    /// write 'accounts_to_write' into it
    /// return shrink_in_progress and some metrics
    #[allow(dead_code)]
    fn write_ancient_accounts<'a, 'b: 'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &'b self,
        bytes: u64,
        accounts_to_write: impl StorableAccounts<'a, T>,
        write_ancient_accounts: &mut WriteAncientAccounts<'b>,
    ) {
        let target_slot = accounts_to_write.target_slot();
        let (shrink_in_progress, create_and_insert_store_elapsed_us) =
            measure_us!(self.get_store_for_shrink(target_slot, bytes));
        let (store_accounts_timing, rewrite_elapsed_us) = measure_us!(self.store_accounts_frozen(
            accounts_to_write,
            None::<Vec<Hash>>,
            shrink_in_progress.new_storage(),
            None,
            StoreReclaims::Ignore,
        ));

        write_ancient_accounts.metrics.accumulate(&ShrinkStatsSub {
            store_accounts_timing,
            rewrite_elapsed_us,
            create_and_insert_store_elapsed_us,
        });
        write_ancient_accounts
            .shrinks_in_progress
            .insert(target_slot, shrink_in_progress);
    }
    /// go through all slots and populate 'SlotInfo', per slot
    /// This provides the list of possible ancient slots to sort, filter, and then combine.
    #[allow(dead_code)]
    fn calc_ancient_slot_info(
        &self,
        slots: Vec<Slot>,
        can_randomly_shrink: bool,
    ) -> AncientSlotInfos {
        let len = slots.len();
        let mut infos = AncientSlotInfos {
            shrink_indexes: Vec::with_capacity(len),
            all_infos: Vec::with_capacity(len),
            ..AncientSlotInfos::default()
        };

        for slot in &slots {
            if let Some(storage) = self.storage.get_slot_storage_entry(*slot) {
                infos.add(*slot, storage, can_randomly_shrink);
            }
        }
        infos
    }

    /// write packed storages as described in 'accounts_to_combine'
    /// and 'packed_contents'
    #[allow(dead_code)]
    fn write_packed_storages<'a, 'b>(
        &'a self,
        accounts_to_combine: &'b AccountsToCombine<'b>,
        packed_contents: Vec<PackedAncientStorage<'b>>,
    ) -> WriteAncientAccounts<'a> {
        let mut write_ancient_accounts = WriteAncientAccounts::default();

        // ok if we have more slots, but NOT ok if we have fewer slots than we have contents
        assert!(accounts_to_combine.target_slots_sorted.len() >= packed_contents.len());
        // write packed storages containing contents from many original slots
        // iterate slots in highest to lowest
        accounts_to_combine
            .target_slots_sorted
            .iter()
            .rev()
            .zip(packed_contents)
            .for_each(|(target_slot, pack)| {
                self.write_one_packed_storage(&pack, *target_slot, &mut write_ancient_accounts);
            });

        // write new storages where contents were unable to move because ref_count > 1
        self.write_ancient_accounts_to_same_slot_multiple_refs(
            accounts_to_combine.accounts_keep_slots.values(),
            &mut write_ancient_accounts,
        );
        write_ancient_accounts
    }

    /// for each slot in 'ancient_slots', collect all accounts in that slot
    /// return the collection of accounts by slot
    #[allow(dead_code)]
    fn get_unique_accounts_from_storage_for_combining_ancient_slots<'a>(
        &self,
        ancient_slots: &'a [SlotInfo],
    ) -> Vec<(&'a SlotInfo, GetUniqueAccountsResult<'a>)> {
        let mut accounts_to_combine = Vec::with_capacity(ancient_slots.len());

        for info in ancient_slots {
            let unique_accounts = self.get_unique_accounts_from_storage_for_shrink(
                &info.storage,
                &self.shrink_ancient_stats.shrink_stats,
            );
            accounts_to_combine.push((info, unique_accounts));
        }

        accounts_to_combine
    }

    /// finish shrink operation on slots where a new storage was created
    /// drop root and storage for all original slots whose contents were combined into other storages
    #[allow(dead_code)]
    fn finish_combine_ancient_slots_packed_internal(
        &self,
        accounts_to_combine: AccountsToCombine<'_>,
        mut write_ancient_accounts: WriteAncientAccounts,
        metrics: &mut ShrinkStatsSub,
    ) {
        let mut dropped_roots = Vec::with_capacity(accounts_to_combine.accounts_to_combine.len());
        for shrink_collect in accounts_to_combine.accounts_to_combine {
            let slot = shrink_collect.slot;

            let shrink_in_progress = write_ancient_accounts.shrinks_in_progress.remove(&slot);
            if shrink_in_progress.is_none() {
                dropped_roots.push(slot);
            }
            self.remove_old_stores_shrink(
                &shrink_collect,
                &self.shrink_ancient_stats.shrink_stats,
                shrink_in_progress,
                true,
            );
        }
        self.handle_dropped_roots_for_ancient(dropped_roots.into_iter());
        metrics.accumulate(&write_ancient_accounts.metrics);
    }

    /// given all accounts per ancient slot, in slots that we want to combine together:
    /// 1. Look up each pubkey in the index
    /// 2. separate, by slot, into:
    /// 2a. pubkeys with refcount = 1. This means this pubkey exists NOWHERE else in accounts db.
    /// 2b. pubkeys with refcount > 1
    /// Note that the return value can contain fewer items than 'accounts_per_storage' if we find storages which won't be affected.
    /// 'accounts_per_storage' should be sorted by slot
    #[allow(dead_code)]
    fn calc_accounts_to_combine<'a>(
        &self,
        accounts_per_storage: &'a Vec<(&'a SlotInfo, GetUniqueAccountsResult<'a>)>,
    ) -> AccountsToCombine<'a> {
        let mut accounts_keep_slots = HashMap::default();
        let len = accounts_per_storage.len();
        let mut target_slots_sorted = Vec::with_capacity(len);

        let mut accounts_to_combine = Vec::with_capacity(len);
        for (info, unique_accounts) in accounts_per_storage {
            let mut shrink_collect = self.shrink_collect::<ShrinkCollectAliveSeparatedByRefs<'_>>(
                &info.storage,
                unique_accounts,
                &self.shrink_ancient_stats.shrink_stats,
            );
            let many_refs = &mut shrink_collect.alive_accounts.many_refs;
            if !many_refs.accounts.is_empty() {
                // there are accounts with ref_count > 1. This means this account must remain IN this slot.
                // The same account could exist in a newer or older slot. Moving this account across slots could result
                // in this alive version of the account now being in a slot OLDER than the non-alive instances.
                accounts_keep_slots.insert(info.slot, std::mem::take(many_refs));
            } else {
                // No alive accounts in this slot have a ref_count > 1. So, ALL alive accounts in this slot can be written to any other slot
                // we find convenient. There is NO other instance of any account to conflict with.
                target_slots_sorted.push(info.slot);
            }
            accounts_to_combine.push(shrink_collect);
        }
        AccountsToCombine {
            accounts_to_combine,
            accounts_keep_slots,
            target_slots_sorted,
        }
    }

    /// create packed storage and write contents of 'packed' to it.
    /// accumulate results in 'write_ancient_accounts'
    #[allow(dead_code)]
    fn write_one_packed_storage<'a, 'b: 'a>(
        &'b self,
        packed: &'a PackedAncientStorage<'a>,
        target_slot: Slot,
        write_ancient_accounts: &mut WriteAncientAccounts<'b>,
    ) {
        let PackedAncientStorage {
            bytes: bytes_total,
            accounts: accounts_to_write,
        } = packed;
        let accounts_to_write = StorableAccountsBySlot::new(
            target_slot,
            accounts_to_write,
            INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
        );

        self.write_ancient_accounts(*bytes_total, accounts_to_write, write_ancient_accounts)
    }

    /// For each slot and alive accounts in 'accounts_to_combine'
    /// create a PackedAncientStorage that only contains the given alive accounts.
    /// This will represent only the accounts with ref_count > 1 from the original storage.
    /// These accounts need to be rewritten in their same slot, Ideally with no other accounts in the slot.
    /// Other accounts would have ref_count = 1.
    /// ref_count = 1 accounts will be combined together with other slots into larger append vecs elsewhere.
    #[allow(dead_code)]
    fn write_ancient_accounts_to_same_slot_multiple_refs<'a, 'b: 'a>(
        &'b self,
        accounts_to_combine: impl Iterator<Item = &'a AliveAccounts<'a>>,
        write_ancient_accounts: &mut WriteAncientAccounts<'b>,
    ) {
        for alive_accounts in accounts_to_combine {
            let packed = PackedAncientStorage {
                bytes: alive_accounts.bytes as u64,
                accounts: vec![(alive_accounts.slot, &alive_accounts.accounts[..])],
            };

            self.write_one_packed_storage(&packed, alive_accounts.slot, write_ancient_accounts);
        }
    }
}

/// hold all alive accounts to be shrunk and/or combined
#[allow(dead_code)]
#[derive(Debug, Default)]
struct AccountsToCombine<'a> {
    /// slots and alive accounts that must remain in the slot they are currently in
    /// because the account exists in more than 1 slot in accounts db
    /// This hashmap contains an entry for each slot that contains at least one account with ref_count > 1.
    /// The value of the entry is all alive accounts in that slot whose ref_count > 1.
    /// Any OTHER accounts in that slot whose ref_count = 1 are in 'accounts_to_combine' because they can be moved
    /// to any slot.
    /// We want to keep the ref_count > 1 accounts by themselves, expecting the multiple ref_counts will be resolved
    /// soon and we can clean the duplicates up (which maybe THIS one).
    accounts_keep_slots: HashMap<Slot, AliveAccounts<'a>>,
    /// all the rest of alive accounts that can move slots and should be combined
    /// This includes all accounts with ref_count = 1 from the slots in 'accounts_keep_slots'.
    /// There is one entry here for each storage we are processing. Even if all accounts are in 'accounts_keep_slots'.
    accounts_to_combine: Vec<ShrinkCollect<'a, ShrinkCollectAliveSeparatedByRefs<'a>>>,
    /// slots that contain alive accounts that can move into ANY other ancient slot
    /// these slots will NOT be in 'accounts_keep_slots'
    /// Some of these slots will have ancient append vecs created at them to contain everything in 'accounts_to_combine'
    /// The rest will become dead slots with no accounts in them.
    target_slots_sorted: Vec<Slot>,
}

#[allow(dead_code)]
#[derive(Default)]
/// intended contents of a packed ancient storage
struct PackedAncientStorage<'a> {
    /// accounts to move into this storage, along with the slot the accounts are currently stored in
    accounts: Vec<(Slot, &'a [&'a StoredAccountMeta<'a>])>,
    /// total bytes required to hold 'accounts'
    bytes: u64,
}

impl<'a> PackedAncientStorage<'a> {
    #[allow(dead_code)]
    /// return a minimal set of 'PackedAncientStorage's to contain all 'accounts_to_combine' with
    /// the new storages having a size guided by 'ideal_size'
    fn pack(
        mut accounts_to_combine: impl Iterator<Item = &'a AliveAccounts<'a>>,
        ideal_size: NonZeroU64,
    ) -> Vec<PackedAncientStorage<'a>> {
        let mut result = Vec::default();
        let ideal_size: u64 = ideal_size.into();
        let ideal_size = ideal_size as usize;
        let mut current_alive_accounts = accounts_to_combine.next();
        // starting at first entry in current_alive_accounts
        let mut partial_inner_index = 0;
        // 0 bytes written so far from the current set of accounts
        let mut partial_bytes_written = 0;
        // pack a new storage each iteration of this outer loop
        loop {
            let mut bytes_total = 0usize;
            let mut accounts_to_write = Vec::default();

            // walk through each set of alive accounts to pack the current new storage up to ideal_size
            let mut full = false;
            while !full && current_alive_accounts.is_some() {
                let alive_accounts = current_alive_accounts.unwrap();
                if partial_inner_index >= alive_accounts.accounts.len() {
                    // current_alive_accounts have all been written, so advance to next set from accounts_to_combine
                    current_alive_accounts = accounts_to_combine.next();
                    // reset partial progress since we're starting over with a new set of alive accounts
                    partial_inner_index = 0;
                    partial_bytes_written = 0;
                    continue;
                }
                let bytes_remaining_this_slot =
                    alive_accounts.bytes.saturating_sub(partial_bytes_written);
                let bytes_total_with_this_slot =
                    bytes_total.saturating_add(bytes_remaining_this_slot);
                let mut partial_inner_index_max_exclusive;
                if bytes_total_with_this_slot <= ideal_size {
                    partial_inner_index_max_exclusive = alive_accounts.accounts.len();
                    bytes_total = bytes_total_with_this_slot;
                } else {
                    partial_inner_index_max_exclusive = partial_inner_index;
                    // adding all the alive accounts in this storage would exceed the ideal size, so we have to break these accounts up
                    // look at each account and stop when we exceed the ideal size
                    while partial_inner_index_max_exclusive < alive_accounts.accounts.len() {
                        let account = alive_accounts.accounts[partial_inner_index_max_exclusive];
                        let account_size = aligned_stored_size(account.data().len());
                        let new_size = bytes_total.saturating_add(account_size);
                        if new_size > ideal_size && bytes_total > 0 {
                            full = true;
                            // partial_inner_index_max_exclusive is the index of the first account that puts us over the ideal size
                            // so, save it for next time
                            break;
                        }
                        // this account fits
                        saturating_add_assign!(partial_bytes_written, account_size);
                        bytes_total = new_size;
                        partial_inner_index_max_exclusive += 1;
                    }
                }

                if partial_inner_index < partial_inner_index_max_exclusive {
                    // these accounts belong in the current packed storage we're working on
                    accounts_to_write.push((
                        alive_accounts.slot,
                        // maybe all alive accounts from the current or could be partial
                        &alive_accounts.accounts
                            [partial_inner_index..partial_inner_index_max_exclusive],
                    ));
                }
                // start next storage with the account we ended with
                // this could be the end of the current alive accounts or could be anywhere within that vec
                partial_inner_index = partial_inner_index_max_exclusive;
            }
            if accounts_to_write.is_empty() {
                // if we returned without any accounts to write, then we have exhausted source data and have packaged all the storages we need
                break;
            }
            // we know the full contents of this packed storage now
            result.push(PackedAncientStorage {
                bytes: bytes_total as u64,
                accounts: accounts_to_write,
            });
        }
        result
    }
}

/// a set of accounts need to be stored.
/// If there are too many to fit in 'Primary', the rest are put in 'Overflow'
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StorageSelector {
    Primary,
    Overflow,
}

/// reference a set of accounts to store
/// The accounts may have to be split between 2 storages (primary and overflow) if there is not enough room in the primary storage.
/// The 'store' functions need data stored in a slice of specific type.
/// We need 1-2 of these slices constructed based on available bytes and individual account sizes.
/// The slice arithmetic across both hashes and account data gets messy. So, this struct abstracts that.
pub struct AccountsToStore<'a> {
    accounts: &'a [&'a StoredAccountMeta<'a>],
    /// if 'accounts' contains more items than can be contained in the primary storage, then we have to split these accounts.
    /// 'index_first_item_overflow' specifies the index of the first item in 'accounts' that will go into the overflow storage
    index_first_item_overflow: usize,
    pub slot: Slot,
}

impl<'a> AccountsToStore<'a> {
    /// break 'stored_accounts' into primary and overflow
    /// available_bytes: how many bytes remain in the primary storage. Excess accounts will be directed to an overflow storage
    pub fn new(
        mut available_bytes: u64,
        accounts: &'a [&'a StoredAccountMeta<'a>],
        alive_total_bytes: usize,
        slot: Slot,
    ) -> Self {
        let num_accounts = accounts.len();
        // index of the first account that doesn't fit in the current append vec
        let mut index_first_item_overflow = num_accounts; // assume all fit
        if alive_total_bytes > available_bytes as usize {
            // not all the alive bytes fit, so we have to find how many accounts fit within available_bytes
            for (i, account) in accounts.iter().enumerate() {
                let account_size = account.stored_size() as u64;
                if available_bytes >= account_size {
                    available_bytes = available_bytes.saturating_sub(account_size);
                } else if index_first_item_overflow == num_accounts {
                    // the # of accounts we have so far seen is the most that will fit in the current ancient append vec
                    index_first_item_overflow = i;
                    break;
                }
            }
        }
        Self {
            accounts,
            index_first_item_overflow,
            slot,
        }
    }

    /// true if a request to 'get' 'Overflow' would return accounts & hashes
    pub fn has_overflow(&self) -> bool {
        self.index_first_item_overflow < self.accounts.len()
    }

    /// get the accounts to store in the given 'storage'
    pub fn get(&self, storage: StorageSelector) -> &[&'a StoredAccountMeta<'a>] {
        let range = match storage {
            StorageSelector::Primary => 0..self.index_first_item_overflow,
            StorageSelector::Overflow => self.index_first_item_overflow..self.accounts.len(),
        };
        &self.accounts[range]
    }
}

/// capacity of an ancient append vec
pub fn get_ancient_append_vec_capacity() -> u64 {
    use crate::append_vec::MAXIMUM_APPEND_VEC_FILE_SIZE;
    // smaller than max by a bit just in case
    // some functions add slop on allocation
    // The bigger an append vec is, the more unwieldy it becomes to shrink, create, write.
    // 1/10 of max is a reasonable size in practice.
    MAXIMUM_APPEND_VEC_FILE_SIZE / 10 - 2048
}

/// is this a max-size append vec designed to be used as an ancient append vec?
pub fn is_ancient(storage: &AccountsFile) -> bool {
    match storage {
        AccountsFile::AppendVec(storage) => storage.capacity() >= get_ancient_append_vec_capacity(),
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            account_storage::meta::{AccountMeta, StoredAccountMeta, StoredMeta},
            accounts_db::{
                get_temp_accounts_paths,
                tests::{
                    append_single_account_with_default_hash, compare_all_accounts,
                    create_db_with_storages_and_index, create_storages_and_update_index,
                    get_all_accounts, remove_account_for_tests, CAN_RANDOMLY_SHRINK_FALSE,
                },
                INCLUDE_SLOT_IN_HASH_TESTS,
            },
            append_vec::{aligned_stored_size, AppendVec, AppendVecAccountMeta},
            storable_accounts::StorableAccountsBySlot,
        },
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount, WritableAccount},
            hash::Hash,
            pubkey::Pubkey,
        },
        std::ops::Range,
        strum::IntoEnumIterator,
        strum_macros::EnumIter,
    };

    fn get_sample_storages(
        slots: usize,
        account_data_size: Option<u64>,
    ) -> (
        AccountsDb,
        Vec<Arc<AccountStorageEntry>>,
        Range<u64>,
        Vec<SlotInfo>,
    ) {
        let alive = true;
        let (db, slot1) = create_db_with_storages_and_index(alive, slots, account_data_size);
        let original_stores = (0..slots)
            .filter_map(|slot| db.storage.get_slot_storage_entry((slot as Slot) + slot1))
            .collect::<Vec<_>>();
        let slot_infos = original_stores
            .iter()
            .map(|storage| SlotInfo {
                storage: Arc::clone(storage),
                slot: storage.slot(),
                capacity: 0,
                alive_bytes: 0,
                should_shrink: false,
            })
            .collect();
        (
            db,
            original_stores,
            slot1..(slot1 + slots as Slot),
            slot_infos,
        )
    }

    fn unique_to_accounts<'a>(
        one: impl Iterator<Item = &'a GetUniqueAccountsResult<'a>>,
    ) -> Vec<(Pubkey, AccountSharedData)> {
        one.flat_map(|result| {
            result
                .stored_accounts
                .iter()
                .map(|result| (*result.pubkey(), result.to_account_shared_data()))
        })
        .collect()
    }

    pub(crate) fn compare_all_vec_accounts<'a>(
        one: impl Iterator<Item = &'a GetUniqueAccountsResult<'a>>,
        two: impl Iterator<Item = &'a GetUniqueAccountsResult<'a>>,
    ) {
        compare_all_accounts(&unique_to_accounts(one), &unique_to_accounts(two));
    }

    #[test]
    fn test_write_packed_storages_empty() {
        let (db, _storages, _slots, _infos) = get_sample_storages(0, None);
        let write_ancient_accounts =
            db.write_packed_storages(&AccountsToCombine::default(), Vec::default());
        assert!(write_ancient_accounts.shrinks_in_progress.is_empty());
    }

    #[test]
    #[should_panic(
        expected = "assertion failed: accounts_to_combine.target_slots_sorted.len() >= packed_contents.len()"
    )]
    fn test_write_packed_storages_too_few_slots() {
        let (db, storages, slots, _infos) = get_sample_storages(1, None);
        let accounts_to_combine = AccountsToCombine::default();
        let accounts = [storages
            .first()
            .unwrap()
            .accounts
            .accounts(0)
            .pop()
            .unwrap()];
        let accounts = accounts.iter().collect::<Vec<_>>();
        let packed_contents = vec![PackedAncientStorage {
            bytes: 0,
            accounts: vec![(slots.start, &accounts[..])],
        }];
        db.write_packed_storages(&accounts_to_combine, packed_contents);
    }

    #[test]
    fn test_write_ancient_accounts_to_same_slot_multiple_refs_empty() {
        let (db, _storages, _slots, _infos) = get_sample_storages(0, None);
        let mut write_ancient_accounts = WriteAncientAccounts::default();
        db.write_ancient_accounts_to_same_slot_multiple_refs(
            AccountsToCombine::default().accounts_keep_slots.values(),
            &mut write_ancient_accounts,
        );
        assert!(write_ancient_accounts.shrinks_in_progress.is_empty());
    }

    #[test]
    fn test_pack_ancient_storages_one_account_per_storage() {
        for num_slots in 0..4 {
            for (ideal_size, expected_storages) in [
                (1, num_slots),
                (get_ancient_append_vec_capacity(), 1.min(num_slots)),
            ] {
                let (db, storages, slots, _infos) = get_sample_storages(num_slots, None);
                let original_results = storages
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();

                let slots_vec = slots.collect::<Vec<_>>();
                let accounts_to_combine = original_results
                    .iter()
                    .zip(slots_vec.iter().cloned())
                    .map(|(accounts, slot)| AliveAccounts {
                        accounts: accounts.stored_accounts.iter().collect::<Vec<_>>(),
                        bytes: accounts
                            .stored_accounts
                            .iter()
                            .map(|account| aligned_stored_size(account.data().len()))
                            .sum(),
                        slot,
                    })
                    .collect::<Vec<_>>();

                let result = PackedAncientStorage::pack(
                    accounts_to_combine.iter(),
                    NonZeroU64::new(ideal_size).unwrap(),
                );
                let storages_needed = result.len();
                assert_eq!(storages_needed, expected_storages);
            }
        }
    }

    #[test]
    fn test_pack_ancient_storages_one_partial() {
        // n slots
        // m accounts per slot
        // divide into different ideal sizes so that we combine multiple slots sometimes and combine partial slots
        solana_logger::setup();
        let total_accounts_per_storage = 10;
        let account_size = 184;
        for num_slots in 0..4 {
            for (ideal_size, expected_storages) in [
                (1, num_slots * total_accounts_per_storage),
                (account_size - 1, num_slots * total_accounts_per_storage),
                (account_size, num_slots * total_accounts_per_storage),
                (account_size + 1, num_slots * total_accounts_per_storage),
                (account_size * 2 - 1, num_slots * total_accounts_per_storage),
                (account_size * 2, num_slots * total_accounts_per_storage / 2),
                (get_ancient_append_vec_capacity(), 1.min(num_slots)),
            ] {
                let (db, storages, slots, _infos) = get_sample_storages(num_slots, None);

                let account_template = storages
                    .first()
                    .map(|storage| {
                        storage
                            .accounts
                            .account_iter()
                            .next()
                            .unwrap()
                            .to_account_shared_data()
                    })
                    .unwrap_or_default();
                // add some accounts to each storage so we can make partial progress
                let mut lamports = 1000;
                let _pubkeys_and_accounts = storages
                    .iter()
                    .map(|storage| {
                        (0..(total_accounts_per_storage - 1))
                            .map(|_| {
                                let pk = solana_sdk::pubkey::new_rand();
                                let mut account = account_template.clone();
                                account.set_lamports(lamports);
                                lamports += 1;
                                append_single_account_with_default_hash(
                                    storage,
                                    &pk,
                                    &account,
                                    0,
                                    true,
                                    Some(&db.accounts_index),
                                );
                                (pk, account)
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();

                let original_results = storages
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();

                let slots_vec = slots.collect::<Vec<_>>();
                let accounts_to_combine = original_results
                    .iter()
                    .zip(slots_vec.iter().cloned())
                    .map(|(accounts, slot)| AliveAccounts {
                        accounts: accounts.stored_accounts.iter().collect::<Vec<_>>(),
                        bytes: accounts
                            .stored_accounts
                            .iter()
                            .map(|account| aligned_stored_size(account.data().len()))
                            .sum(),
                        slot,
                    })
                    .collect::<Vec<_>>();

                let result = PackedAncientStorage::pack(
                    accounts_to_combine.iter(),
                    NonZeroU64::new(ideal_size).unwrap(),
                );
                let storages_needed = result.len();
                assert_eq!(storages_needed, expected_storages, "num_slots: {num_slots}, expected_storages: {expected_storages}, storages_needed: {storages_needed}, ideal_size: {ideal_size}");
                compare_all_accounts(
                    &packed_to_compare(&result)[..],
                    &unique_to_compare(&original_results)[..],
                );
            }
        }
    }

    fn packed_to_compare(packed: &[PackedAncientStorage]) -> Vec<(Pubkey, AccountSharedData)> {
        packed
            .iter()
            .flat_map(|packed| {
                packed.accounts.iter().flat_map(|(_slot, stored_metas)| {
                    stored_metas.iter().map(|stored_meta| {
                        (*stored_meta.pubkey(), stored_meta.to_account_shared_data())
                    })
                })
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn test_pack_ancient_storages_varying() {
        // n slots
        // different number of accounts in each slot
        // each account has different size
        // divide into different ideal sizes so that we combine multiple slots sometimes and combine partial slots
        // compare at end that all accounts are in result exactly once
        solana_logger::setup();
        let total_accounts_per_storage = 10;
        let account_size = 184;
        for num_slots in 0..4 {
            for ideal_size in [
                1,
                account_size - 1,
                account_size,
                account_size + 1,
                account_size * 2 - 1,
                account_size * 2,
                get_ancient_append_vec_capacity(),
            ] {
                let (db, storages, slots, _infos) = get_sample_storages(num_slots, None);

                let account_template = storages
                    .first()
                    .map(|storage| {
                        storage
                            .accounts
                            .account_iter()
                            .next()
                            .unwrap()
                            .to_account_shared_data()
                    })
                    .unwrap_or_default();
                // add some accounts to each storage so we can make partial progress
                let mut data_size = 450;
                // random # of extra accounts here
                let total_accounts_per_storage =
                    thread_rng().gen_range(0, total_accounts_per_storage);
                let _pubkeys_and_accounts = storages
                    .iter()
                    .map(|storage| {
                        (0..(total_accounts_per_storage - 1))
                            .map(|_| {
                                let pk = solana_sdk::pubkey::new_rand();
                                let mut account = account_template.clone();
                                account.set_data((0..data_size).map(|x| (x % 256) as u8).collect());
                                data_size += 1;
                                append_single_account_with_default_hash(
                                    storage,
                                    &pk,
                                    &account,
                                    0,
                                    true,
                                    Some(&db.accounts_index),
                                );
                                (pk, account)
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();

                let original_results = storages
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();

                let slots_vec = slots.collect::<Vec<_>>();
                let accounts_to_combine = original_results
                    .iter()
                    .zip(slots_vec.iter().cloned())
                    .map(|(accounts, slot)| AliveAccounts {
                        accounts: accounts.stored_accounts.iter().collect::<Vec<_>>(),
                        bytes: accounts
                            .stored_accounts
                            .iter()
                            .map(|account| aligned_stored_size(account.data().len()))
                            .sum(),
                        slot,
                    })
                    .collect::<Vec<_>>();

                let result = PackedAncientStorage::pack(
                    accounts_to_combine.iter(),
                    NonZeroU64::new(ideal_size).unwrap(),
                );

                let largest_account_size = aligned_stored_size(data_size) as u64;
                // all packed storages should be close to ideal size
                result.iter().enumerate().for_each(|(i, packed)| {
                    if i + 1 < result.len() && ideal_size > largest_account_size {
                        // cannot assert this on the last packed storage - it may be small
                        // cannot assert this when the ideal size is too small to hold the largest account size
                        assert!(
                            packed.bytes >= ideal_size - largest_account_size,
                            "packed size too small: bytes: {}, ideal: {}, largest: {}",
                            packed.bytes,
                            ideal_size,
                            largest_account_size
                        );
                    }
                    assert!(
                        packed.bytes > 0,
                        "packed size of zero"
                    );
                    assert!(
                        packed.bytes <= ideal_size || packed.accounts.iter().map(|(_slot, accounts)| accounts.len()).sum::<usize>() == 1,
                        "packed size too large: bytes: {}, ideal_size: {}, data_size: {}, num_slots: {}, # accounts: {}",
                        packed.bytes,
                        ideal_size,
                        data_size,
                        num_slots,
                        packed.accounts.len()
                    );
                });
                result.iter().for_each(|packed| {
                    assert_eq!(
                        packed.bytes,
                        packed
                            .accounts
                            .iter()
                            .map(|(_slot, accounts)| accounts
                                .iter()
                                .map(|account| aligned_stored_size(account.data().len()) as u64)
                                .sum::<u64>())
                            .sum::<u64>()
                    );
                });

                compare_all_accounts(
                    &packed_to_compare(&result)[..],
                    &unique_to_compare(&original_results)[..],
                );
            }
        }
    }

    fn unique_to_compare(unique: &[GetUniqueAccountsResult]) -> Vec<(Pubkey, AccountSharedData)> {
        unique
            .iter()
            .flat_map(|unique| {
                unique.stored_accounts.iter().map(|stored_meta| {
                    (*stored_meta.pubkey(), stored_meta.to_account_shared_data())
                })
            })
            .collect::<Vec<_>>()
    }

    #[derive(EnumIter, Debug, PartialEq, Eq)]
    enum TestWriteMultipleRefs {
        MultipleRefs,
        PackedStorages,
    }

    #[test]
    fn test_finish_combine_ancient_slots_packed_internal() {
        // n storages
        // 1 account each
        // all accounts have 1 ref
        // nothing shrunk, so all storages and roots should be removed
        // or all slots shrunk so no roots or storages should be removed
        for all_slots_shrunk in [false, true] {
            for num_slots in 0..3 {
                let (db, storages, slots, infos) = get_sample_storages(num_slots, None);
                let accounts_per_storage = infos
                    .iter()
                    .zip(
                        storages
                            .iter()
                            .map(|store| db.get_unique_accounts_from_storage(store)),
                    )
                    .collect::<Vec<_>>();

                let accounts_to_combine = db.calc_accounts_to_combine(&accounts_per_storage);
                let mut stats = ShrinkStatsSub::default();
                let mut write_ancient_accounts = WriteAncientAccounts::default();

                slots.clone().for_each(|slot| {
                    db.add_root(slot);
                    assert!(db.storage.get_slot_storage_entry(slot).is_some());
                });

                let roots = db
                    .accounts_index
                    .roots_tracker
                    .read()
                    .unwrap()
                    .alive_roots
                    .get_all();
                assert_eq!(roots, slots.clone().collect::<Vec<_>>());

                if all_slots_shrunk {
                    // make it look like each of the slots was shrunk
                    slots.clone().for_each(|slot| {
                        write_ancient_accounts
                            .shrinks_in_progress
                            .insert(slot, db.get_store_for_shrink(slot, 1));
                    });
                }

                db.finish_combine_ancient_slots_packed_internal(
                    accounts_to_combine,
                    write_ancient_accounts,
                    &mut stats,
                );
                let roots_after = db
                    .accounts_index
                    .roots_tracker
                    .read()
                    .unwrap()
                    .alive_roots
                    .get_all();

                assert_eq!(
                    roots_after,
                    if all_slots_shrunk {
                        slots.clone().collect::<Vec<_>>()
                    } else {
                        vec![]
                    },
                    "all_slots_shrunk: {all_slots_shrunk}"
                );
                slots.for_each(|slot| {
                    let storage = db.storage.get_slot_storage_entry(slot);
                    if all_slots_shrunk {
                        assert!(storage.is_some());
                    } else {
                        assert!(storage.is_none());
                    }
                });
            }
        }
    }

    #[test]
    fn test_calc_accounts_to_combine_simple() {
        // n storages
        // 1 account each
        // all accounts have 1 ref or all accounts have 2 refs
        for method in TestWriteMultipleRefs::iter() {
            for num_slots in 0..3 {
                for two_refs in [false, true] {
                    let (db, storages, slots, infos) = get_sample_storages(num_slots, None);
                    let original_results = storages
                        .iter()
                        .map(|store| db.get_unique_accounts_from_storage(store))
                        .collect::<Vec<_>>();
                    if two_refs {
                        original_results.iter().for_each(|results| {
                            results.stored_accounts.iter().for_each(|account| {
                                let entry = db
                                    .accounts_index
                                    .get_account_read_entry(account.pubkey())
                                    .unwrap();
                                entry.addref();
                            })
                        });
                    }
                    let accounts_per_storage = infos
                        .iter()
                        .zip(original_results.into_iter())
                        .collect::<Vec<_>>();

                    let accounts_to_combine = db.calc_accounts_to_combine(&accounts_per_storage);
                    let slots_vec = slots.collect::<Vec<_>>();
                    assert_eq!(accounts_to_combine.accounts_to_combine.len(), num_slots);
                    if two_refs {
                        // all accounts should be in many_refs
                        let mut accounts_keep = accounts_to_combine
                            .accounts_keep_slots
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>();
                        accounts_keep.sort_unstable();
                        assert_eq!(accounts_keep, slots_vec);
                        assert!(accounts_to_combine.target_slots_sorted.is_empty());
                        assert_eq!(accounts_to_combine.accounts_keep_slots.len(), num_slots);
                        assert!(accounts_to_combine.accounts_to_combine.iter().all(
                            |shrink_collect| shrink_collect
                                .alive_accounts
                                .one_ref
                                .accounts
                                .is_empty()
                        ));
                        assert!(accounts_to_combine.accounts_to_combine.iter().all(
                            |shrink_collect| shrink_collect
                                .alive_accounts
                                .many_refs
                                .accounts
                                .is_empty()
                        ));
                    } else {
                        // all accounts should be in one_ref and all slots are available as target slots
                        assert_eq!(accounts_to_combine.target_slots_sorted, slots_vec);
                        assert!(accounts_to_combine.accounts_keep_slots.is_empty());
                        assert!(accounts_to_combine.accounts_to_combine.iter().all(
                            |shrink_collect| !shrink_collect
                                .alive_accounts
                                .one_ref
                                .accounts
                                .is_empty()
                        ));
                        assert!(accounts_to_combine.accounts_to_combine.iter().all(
                            |shrink_collect| shrink_collect
                                .alive_accounts
                                .many_refs
                                .accounts
                                .is_empty()
                        ));
                    }

                    // test write_ancient_accounts_to_same_slot_multiple_refs since we built interesting 'AccountsToCombine'
                    let write_ancient_accounts = match method {
                        TestWriteMultipleRefs::MultipleRefs => {
                            let mut write_ancient_accounts = WriteAncientAccounts::default();
                            db.write_ancient_accounts_to_same_slot_multiple_refs(
                                accounts_to_combine.accounts_keep_slots.values(),
                                &mut write_ancient_accounts,
                            );
                            write_ancient_accounts
                        }
                        TestWriteMultipleRefs::PackedStorages => {
                            let packed_contents = Vec::default();
                            db.write_packed_storages(&accounts_to_combine, packed_contents)
                        }
                    };
                    if two_refs {
                        assert_eq!(write_ancient_accounts.shrinks_in_progress.len(), num_slots);
                        let mut shrinks_in_progress = write_ancient_accounts
                            .shrinks_in_progress
                            .iter()
                            .collect::<Vec<_>>();
                        shrinks_in_progress.sort_unstable_by(|a, b| a.0.cmp(b.0));
                        assert_eq!(
                            shrinks_in_progress
                                .iter()
                                .map(|(slot, _)| **slot)
                                .collect::<Vec<_>>(),
                            slots_vec
                        );
                        assert_eq!(
                            shrinks_in_progress
                                .iter()
                                .map(|(_, shrink_in_progress)| shrink_in_progress
                                    .old_storage()
                                    .append_vec_id())
                                .collect::<Vec<_>>(),
                            storages
                                .iter()
                                .map(|storage| storage.append_vec_id())
                                .collect::<Vec<_>>()
                        );
                    } else {
                        assert!(write_ancient_accounts.shrinks_in_progress.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn test_calc_accounts_to_combine_opposite() {
        // 1 storage
        // 2 accounts
        // 1 with 1 ref
        // 1 with 2 refs
        for method in TestWriteMultipleRefs::iter() {
            let num_slots = 1;
            let (db, storages, slots, infos) = get_sample_storages(num_slots, None);
            let original_results = storages
                .iter()
                .map(|store| db.get_unique_accounts_from_storage(store))
                .collect::<Vec<_>>();
            let storage = storages.first().unwrap().clone();
            let pk_with_1_ref = solana_sdk::pubkey::new_rand();
            let slot1 = slots.start;
            let account_with_2_refs = original_results
                .first()
                .unwrap()
                .stored_accounts
                .first()
                .unwrap();
            let pk_with_2_refs = account_with_2_refs.pubkey();
            let mut account_with_1_ref = account_with_2_refs.to_account_shared_data();
            _ = account_with_1_ref.checked_add_lamports(1);
            append_single_account_with_default_hash(
                &storage,
                &pk_with_1_ref,
                &account_with_1_ref,
                0,
                true,
                Some(&db.accounts_index),
            );
            original_results.iter().for_each(|results| {
                results.stored_accounts.iter().for_each(|account| {
                    let entry = db
                        .accounts_index
                        .get_account_read_entry(account.pubkey())
                        .unwrap();
                    entry.addref();
                })
            });

            // update to get both accounts in the storage
            let original_results = storages
                .iter()
                .map(|store| db.get_unique_accounts_from_storage(store))
                .collect::<Vec<_>>();
            assert_eq!(original_results.first().unwrap().stored_accounts.len(), 2);
            let accounts_per_storage = infos
                .iter()
                .zip(original_results.into_iter())
                .collect::<Vec<_>>();

            let accounts_to_combine = db.calc_accounts_to_combine(&accounts_per_storage);
            let slots_vec = slots.collect::<Vec<_>>();
            assert_eq!(accounts_to_combine.accounts_to_combine.len(), num_slots);
            // all accounts should be in many_refs
            let mut accounts_keep = accounts_to_combine
                .accounts_keep_slots
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            accounts_keep.sort_unstable();
            assert_eq!(accounts_keep, slots_vec);
            assert!(accounts_to_combine.target_slots_sorted.is_empty());
            assert_eq!(accounts_to_combine.accounts_keep_slots.len(), num_slots);
            assert_eq!(
                accounts_to_combine
                    .accounts_keep_slots
                    .get(&slot1)
                    .unwrap()
                    .accounts
                    .iter()
                    .map(|meta| meta.pubkey())
                    .collect::<Vec<_>>(),
                vec![pk_with_2_refs]
            );
            assert_eq!(accounts_to_combine.accounts_to_combine.len(), 1);
            let one_ref_accounts = &accounts_to_combine
                .accounts_to_combine
                .first()
                .unwrap()
                .alive_accounts
                .one_ref
                .accounts;
            assert_eq!(
                one_ref_accounts
                    .iter()
                    .map(|meta| meta.pubkey())
                    .collect::<Vec<_>>(),
                vec![&pk_with_1_ref]
            );
            assert_eq!(
                one_ref_accounts
                    .iter()
                    .map(|meta| meta.to_account_shared_data())
                    .collect::<Vec<_>>(),
                vec![account_with_1_ref]
            );
            assert!(accounts_to_combine
                .accounts_to_combine
                .iter()
                .all(|shrink_collect| shrink_collect.alive_accounts.many_refs.accounts.is_empty()));

            // test write_ancient_accounts_to_same_slot_multiple_refs since we built interesting 'AccountsToCombine'
            let write_ancient_accounts = match method {
                TestWriteMultipleRefs::MultipleRefs => {
                    let mut write_ancient_accounts = WriteAncientAccounts::default();
                    db.write_ancient_accounts_to_same_slot_multiple_refs(
                        accounts_to_combine.accounts_keep_slots.values(),
                        &mut write_ancient_accounts,
                    );
                    write_ancient_accounts
                }
                TestWriteMultipleRefs::PackedStorages => {
                    let packed_contents = Vec::default();
                    db.write_packed_storages(&accounts_to_combine, packed_contents)
                }
            };
            assert_eq!(write_ancient_accounts.shrinks_in_progress.len(), num_slots);
            let mut shrinks_in_progress = write_ancient_accounts
                .shrinks_in_progress
                .iter()
                .collect::<Vec<_>>();
            shrinks_in_progress.sort_unstable_by(|a, b| a.0.cmp(b.0));
            assert_eq!(
                shrinks_in_progress
                    .iter()
                    .map(|(slot, _)| **slot)
                    .collect::<Vec<_>>(),
                slots_vec
            );
            assert_eq!(
                shrinks_in_progress
                    .iter()
                    .map(|(_, shrink_in_progress)| shrink_in_progress.old_storage().append_vec_id())
                    .collect::<Vec<_>>(),
                storages
                    .iter()
                    .map(|storage| storage.append_vec_id())
                    .collect::<Vec<_>>()
            );
            // assert that we wrote the 2_ref account to the newly shrunk append vec
            let shrink_in_progress = shrinks_in_progress.first().unwrap().1;
            let accounts_shrunk_same_slot = shrink_in_progress.new_storage().accounts.accounts(0);
            assert_eq!(accounts_shrunk_same_slot.len(), 1);
            assert_eq!(
                accounts_shrunk_same_slot.first().unwrap().pubkey(),
                pk_with_2_refs
            );
            assert_eq!(
                accounts_shrunk_same_slot
                    .first()
                    .unwrap()
                    .to_account_shared_data(),
                account_with_2_refs.to_account_shared_data()
            );
        }
    }

    #[test]
    fn test_get_unique_accounts_from_storage_for_combining_ancient_slots() {
        for num_slots in 0..3 {
            for reverse in [false, true] {
                let (db, storages, slots, mut infos) = get_sample_storages(num_slots, None);
                let original_results = storages
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();
                if reverse {
                    // reverse the contents for further testing
                    infos = infos.into_iter().rev().collect();
                }
                let results =
                    db.get_unique_accounts_from_storage_for_combining_ancient_slots(&infos);

                let all_accounts = get_all_accounts(&db, slots);
                assert_eq!(all_accounts.len(), num_slots);

                compare_all_vec_accounts(
                    original_results.iter(),
                    results.iter().map(|(_, accounts)| accounts),
                );
                compare_all_accounts(
                    &all_accounts,
                    &unique_to_accounts(results.iter().map(|(_, accounts)| accounts)),
                );

                let map = |info: &SlotInfo| {
                    (
                        info.storage.append_vec_id(),
                        info.slot,
                        info.capacity,
                        info.alive_bytes,
                        info.should_shrink,
                    )
                };
                assert_eq!(
                    infos.iter().map(map).collect::<Vec<_>>(),
                    results
                        .into_iter()
                        .map(|(info, _)| map(info))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn test_accounts_to_store_simple() {
        let map = vec![];
        let slot = 1;
        let accounts_to_store = AccountsToStore::new(0, &map, 0, slot);
        for selector in [StorageSelector::Primary, StorageSelector::Overflow] {
            let accounts = accounts_to_store.get(selector);
            assert!(accounts.is_empty());
        }
        assert!(!accounts_to_store.has_overflow());
    }

    #[test]
    fn test_accounts_to_store_more() {
        let pubkey = Pubkey::from([1; 32]);
        let account_size = 3;

        let account = AccountSharedData::default();

        let account_meta = AccountMeta {
            lamports: 1,
            owner: Pubkey::from([2; 32]),
            executable: false,
            rent_epoch: 0,
        };
        let offset = 3;
        let hash = Hash::new(&[2; 32]);
        let stored_meta = StoredMeta {
            /// global write version
            write_version_obsolete: 0,
            /// key for the account
            pubkey,
            data_len: 43,
        };
        let account = StoredAccountMeta::AppendVec(AppendVecAccountMeta {
            meta: &stored_meta,
            /// account data
            account_meta: &account_meta,
            data: account.data(),
            offset,
            stored_size: account_size,
            hash: &hash,
        });
        let map = vec![&account];
        for (selector, available_bytes) in [
            (StorageSelector::Primary, account_size),
            (StorageSelector::Overflow, account_size - 1),
        ] {
            let slot = 1;
            let alive_total_bytes = account_size;
            let accounts_to_store =
                AccountsToStore::new(available_bytes as u64, &map, alive_total_bytes, slot);
            let accounts = accounts_to_store.get(selector);
            assert_eq!(
                accounts.iter().collect::<Vec<_>>(),
                map.iter().collect::<Vec<_>>(),
                "mismatch"
            );
            let accounts = accounts_to_store.get(get_opposite(&selector));
            assert_eq!(
                selector == StorageSelector::Overflow,
                accounts_to_store.has_overflow()
            );
            assert!(accounts.is_empty());
        }
    }
    fn get_opposite(selector: &StorageSelector) -> StorageSelector {
        match selector {
            StorageSelector::Overflow => StorageSelector::Primary,
            StorageSelector::Primary => StorageSelector::Overflow,
        }
    }

    #[test]
    fn test_get_ancient_append_vec_capacity() {
        assert_eq!(
            get_ancient_append_vec_capacity(),
            crate::append_vec::MAXIMUM_APPEND_VEC_FILE_SIZE / 10 - 2048
        );
    }

    #[test]
    fn test_is_ancient() {
        for (size, expected_ancient) in [
            (get_ancient_append_vec_capacity() + 1, true),
            (get_ancient_append_vec_capacity(), true),
            (get_ancient_append_vec_capacity() - 1, false),
        ] {
            let tf = crate::append_vec::test_utils::get_append_vec_path("test_is_ancient");
            let (_temp_dirs, _paths) = get_temp_accounts_paths(1).unwrap();
            let av = AccountsFile::AppendVec(AppendVec::new(&tf.path, true, size as usize));

            assert_eq!(expected_ancient, is_ancient(&av));
        }
    }

    fn get_one_packed_ancient_append_vec_and_others(
        alive: bool,
        num_normal_slots: usize,
    ) -> (AccountsDb, Slot) {
        let (db, slot1) = create_db_with_storages_and_index(alive, num_normal_slots + 1, None);
        let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
        let created_accounts = db.get_unique_accounts_from_storage(&storage);

        db.combine_ancient_slots_packed(vec![slot1], CAN_RANDOMLY_SHRINK_FALSE);
        assert!(db.storage.get_slot_storage_entry(slot1).is_some());
        let after_store = db.storage.get_slot_storage_entry(slot1).unwrap();
        let GetUniqueAccountsResult {
            stored_accounts: after_stored_accounts,
            capacity: after_capacity,
        } = db.get_unique_accounts_from_storage(&after_store);
        assert_eq!(created_accounts.capacity, after_capacity);
        assert_eq!(created_accounts.stored_accounts.len(), 1);
        // always 1 account: either we leave the append vec alone if it is all dead
        // or we create a new one and copy into it if account is alive
        assert_eq!(after_stored_accounts.len(), 1);
        (db, slot1)
    }

    fn assert_storage_info(info: &SlotInfo, storage: &AccountStorageEntry, should_shrink: bool) {
        assert_eq!(storage.append_vec_id(), info.storage.append_vec_id());
        assert_eq!(storage.slot(), info.slot);
        assert_eq!(storage.capacity(), info.capacity);
        assert_eq!(storage.alive_bytes(), info.alive_bytes as usize);
        assert_eq!(should_shrink, info.should_shrink);
    }

    #[derive(EnumIter, Debug, PartialEq, Eq)]
    enum TestCollectInfo {
        CollectSortFilterInfo,
        CalcAncientSlotInfo,
        Add,
    }

    #[test]
    fn test_calc_ancient_slot_info_one_alive_only() {
        let can_randomly_shrink = false;
        let alive = true;
        let slots = 1;
        for method in TestCollectInfo::iter() {
            // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
            for data_size in [None, Some(1_040_000)] {
                let (db, slot1) = create_db_with_storages_and_index(alive, slots, data_size);
                let mut infos = AncientSlotInfos::default();
                let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
                let alive_bytes_expected = storage.alive_bytes();
                match method {
                    TestCollectInfo::Add => {
                        // test lower level 'add'
                        infos.add(slot1, Arc::clone(&storage), can_randomly_shrink);
                    }
                    TestCollectInfo::CalcAncientSlotInfo => {
                        infos = db.calc_ancient_slot_info(vec![slot1], can_randomly_shrink);
                    }
                    TestCollectInfo::CollectSortFilterInfo => {
                        let tuning = PackedAncientStorageTuning {
                            percent_of_alive_shrunk_data: 100,
                            max_ancient_slots: 0,
                            // irrelevant
                            ideal_storage_size: NonZeroU64::new(1).unwrap(),
                            can_randomly_shrink,
                        };
                        infos = db.collect_sort_filter_ancient_slots(vec![slot1], &tuning);
                    }
                }
                assert_eq!(infos.all_infos.len(), 1);
                let should_shrink = data_size.is_none();
                assert_storage_info(infos.all_infos.first().unwrap(), &storage, should_shrink);
                if should_shrink {
                    // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                    assert_eq!(
                        infos.shrink_indexes,
                        if !matches!(method, TestCollectInfo::CollectSortFilterInfo) {
                            vec![0]
                        } else {
                            Vec::default()
                        }
                    );
                    assert_eq!(infos.total_alive_bytes, alive_bytes_expected as u64);
                    assert_eq!(infos.total_alive_bytes_shrink, alive_bytes_expected as u64);
                } else {
                    assert!(infos.shrink_indexes.is_empty());
                    assert_eq!(infos.total_alive_bytes, alive_bytes_expected as u64);
                    assert_eq!(infos.total_alive_bytes_shrink, 0);
                }
            }
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_one_dead() {
        let can_randomly_shrink = false;
        let alive = false;
        let slots = 1;
        for call_add in [false, true] {
            let (db, slot1) = create_db_with_storages_and_index(alive, slots, None);
            let mut infos = AncientSlotInfos::default();
            let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
            if call_add {
                infos.add(slot1, Arc::clone(&storage), can_randomly_shrink);
            } else {
                infos = db.calc_ancient_slot_info(vec![slot1], can_randomly_shrink);
            }
            assert!(infos.all_infos.is_empty());
            assert!(infos.shrink_indexes.is_empty());
            assert_eq!(infos.total_alive_bytes, 0);
            assert_eq!(infos.total_alive_bytes_shrink, 0);
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_several() {
        let can_randomly_shrink = false;
        for alive in [true, false] {
            for slots in 0..4 {
                // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
                for data_size in [None, Some(1_040_000)] {
                    let (db, slot1) = create_db_with_storages_and_index(alive, slots, data_size);
                    let slot_vec = (slot1..(slot1 + slots as Slot)).collect::<Vec<_>>();
                    let storages = slot_vec
                        .iter()
                        .map(|slot| db.storage.get_slot_storage_entry(*slot).unwrap())
                        .collect::<Vec<_>>();
                    let alive_bytes_expected = storages
                        .iter()
                        .map(|storage| storage.alive_bytes() as u64)
                        .sum::<u64>();
                    let infos = db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink);
                    if !alive {
                        assert!(infos.all_infos.is_empty());
                        assert!(infos.shrink_indexes.is_empty());
                        assert_eq!(infos.total_alive_bytes, 0);
                        assert_eq!(infos.total_alive_bytes_shrink, 0);
                    } else {
                        assert_eq!(infos.all_infos.len(), slots);
                        let should_shrink = data_size.is_none();
                        storages
                            .iter()
                            .zip(infos.all_infos.iter())
                            .for_each(|(storage, info)| {
                                assert_storage_info(info, storage, should_shrink);
                            });
                        if should_shrink {
                            // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                            assert_eq!(
                                infos.shrink_indexes,
                                slot_vec
                                    .iter()
                                    .enumerate()
                                    .map(|(i, _)| i)
                                    .collect::<Vec<_>>()
                            );
                            assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                            assert_eq!(infos.total_alive_bytes_shrink, alive_bytes_expected);
                        } else {
                            assert!(infos.shrink_indexes.is_empty());
                            assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                            assert_eq!(infos.total_alive_bytes_shrink, 0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_one_alive_one_dead() {
        let can_randomly_shrink = false;
        for method in TestCollectInfo::iter() {
            for slot1_is_alive in [false, true] {
                let alives = vec![false /*dummy*/, slot1_is_alive, !slot1_is_alive];
                let slots = 2;
                // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
                for data_size in [None, Some(1_040_000)] {
                    let (db, slot1) =
                        create_db_with_storages_and_index(true /*alive*/, slots, data_size);
                    assert_eq!(slot1, 1); // make sure index into alives will be correct
                    assert_eq!(alives[slot1 as usize], slot1_is_alive);
                    let slot_vec = (slot1..(slot1 + slots as Slot)).collect::<Vec<_>>();
                    let storages = slot_vec
                        .iter()
                        .map(|slot| db.storage.get_slot_storage_entry(*slot).unwrap())
                        .collect::<Vec<_>>();
                    storages.iter().for_each(|storage| {
                        let slot = storage.slot();
                        let alive = alives[slot as usize];
                        if !alive {
                            // make this storage not alive
                            remove_account_for_tests(
                                storage,
                                storage.written_bytes() as usize,
                                false,
                            );
                        }
                    });
                    let alive_storages = storages
                        .iter()
                        .filter_map(|storage| alives[storage.slot() as usize].then_some(storage))
                        .collect::<Vec<_>>();
                    let alive_bytes_expected = alive_storages
                        .iter()
                        .map(|storage| storage.alive_bytes() as u64)
                        .sum::<u64>();

                    let infos = match method {
                        TestCollectInfo::CalcAncientSlotInfo => {
                            db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink)
                        }
                        TestCollectInfo::Add => {
                            continue; // unsupportable
                        }
                        TestCollectInfo::CollectSortFilterInfo => {
                            let tuning = PackedAncientStorageTuning {
                                percent_of_alive_shrunk_data: 100,
                                max_ancient_slots: 0,
                                // irrelevant
                                ideal_storage_size: NonZeroU64::new(1).unwrap(),
                                can_randomly_shrink,
                            };
                            db.collect_sort_filter_ancient_slots(slot_vec.clone(), &tuning)
                        }
                    };
                    assert_eq!(infos.all_infos.len(), 1, "method: {method:?}");
                    let should_shrink = data_size.is_none();
                    alive_storages.iter().zip(infos.all_infos.iter()).for_each(
                        |(storage, info)| {
                            assert_storage_info(info, storage, should_shrink);
                        },
                    );
                    if should_shrink {
                        // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                        assert_eq!(
                            infos.shrink_indexes,
                            if !matches!(method, TestCollectInfo::CollectSortFilterInfo) {
                                vec![0]
                            } else {
                                Vec::default()
                            }
                        );
                        assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                        assert_eq!(infos.total_alive_bytes_shrink, alive_bytes_expected);
                    } else {
                        assert!(infos.shrink_indexes.is_empty());
                        assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                        assert_eq!(infos.total_alive_bytes_shrink, 0);
                    }
                }
            }
        }
    }

    fn create_test_infos(count: usize) -> AncientSlotInfos {
        let (db, slot1) = create_db_with_storages_and_index(true /*alive*/, 1, None);
        let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
        AncientSlotInfos {
            all_infos: (0..count)
                .map(|index| SlotInfo {
                    storage: Arc::clone(&storage),
                    slot: index as Slot,
                    capacity: 1,
                    alive_bytes: 1,
                    should_shrink: false,
                })
                .collect(),
            shrink_indexes: (0..count).collect(),
            ..AncientSlotInfos::default()
        }
    }

    #[derive(EnumIter, Debug, PartialEq, Eq)]
    enum TestSmallestCapacity {
        FilterAncientSlots,
        FilterBySmallestCapacity,
    }

    #[test]
    fn test_filter_by_smallest_capacity_empty() {
        for method in TestSmallestCapacity::iter() {
            for max_storages in 1..3 {
                // requesting N max storage, has 1 storage, N >= 1 so nothing to do
                let ideal_storage_size_large = get_ancient_append_vec_capacity();
                let mut infos = create_test_infos(1);
                match method {
                    TestSmallestCapacity::FilterAncientSlots => {
                        let tuning = PackedAncientStorageTuning {
                            max_ancient_slots: max_storages,
                            ideal_storage_size: NonZeroU64::new(ideal_storage_size_large).unwrap(),
                            // irrelevant since we clear 'shrink_indexes'
                            percent_of_alive_shrunk_data: 0,
                            can_randomly_shrink: false,
                        };
                        infos.shrink_indexes.clear();
                        infos.filter_ancient_slots(&tuning);
                    }
                    TestSmallestCapacity::FilterBySmallestCapacity => {
                        infos.filter_by_smallest_capacity(
                            max_storages,
                            NonZeroU64::new(ideal_storage_size_large).unwrap(),
                        );
                    }
                }
                assert!(infos.all_infos.is_empty());
            }
        }
    }

    #[test]
    fn test_filter_by_smaller_capacity_sort() {
        // max is 3
        // 4 storages
        // storage[3] is big enough to cause us to need another storage
        // so, storage[0] and [1] can be combined into 1, resulting in 3 remaining storages, which is
        // the goal, so we only have to combine the first 2 to hit the goal
        for method in TestSmallestCapacity::iter() {
            let ideal_storage_size_large = get_ancient_append_vec_capacity();
            for reorder in [false, true] {
                let mut infos = create_test_infos(4);
                infos
                    .all_infos
                    .iter_mut()
                    .enumerate()
                    .for_each(|(i, info)| info.capacity = 1 + i as u64);
                if reorder {
                    infos.all_infos[3].capacity = 0; // sort to beginning
                }
                infos.all_infos[3].alive_bytes = ideal_storage_size_large;
                let max_storages = 3;
                match method {
                    TestSmallestCapacity::FilterBySmallestCapacity => {
                        infos.filter_by_smallest_capacity(
                            max_storages,
                            NonZeroU64::new(ideal_storage_size_large).unwrap(),
                        );
                    }
                    TestSmallestCapacity::FilterAncientSlots => {
                        let tuning = PackedAncientStorageTuning {
                            max_ancient_slots: max_storages,
                            ideal_storage_size: NonZeroU64::new(ideal_storage_size_large).unwrap(),
                            // irrelevant since we clear 'shrink_indexes'
                            percent_of_alive_shrunk_data: 0,
                            can_randomly_shrink: false,
                        };
                        infos.shrink_indexes.clear();
                        infos.filter_ancient_slots(&tuning);
                    }
                }
                assert_eq!(
                    infos
                        .all_infos
                        .iter()
                        .map(|info| info.slot)
                        .collect::<Vec<_>>(),
                    if reorder { vec![3, 0, 1] } else { vec![0, 1] },
                    "reorder: {reorder}"
                );
            }
        }
    }

    #[test]
    fn test_truncate_to_max_storages() {
        for filter in [false, true] {
            let test = |infos: &mut AncientSlotInfos, max_storages, ideal_storage_size| {
                if filter {
                    infos.filter_by_smallest_capacity(max_storages, ideal_storage_size);
                } else {
                    infos.truncate_to_max_storages(max_storages, ideal_storage_size);
                }
            };
            let ideal_storage_size_large = get_ancient_append_vec_capacity();
            let mut infos = create_test_infos(1);
            let max_storages = 1;
            // 1 storage, 1 max, but 1 storage does not fill the entire new combined storage, so truncate nothing
            test(
                &mut infos,
                max_storages,
                NonZeroU64::new(ideal_storage_size_large).unwrap(),
            );
            assert_eq!(infos.all_infos.len(), usize::from(!filter));

            let mut infos = create_test_infos(1);
            let max_storages = 1;
            infos.all_infos[0].alive_bytes = ideal_storage_size_large + 1; // too big for 1 ideal storage
                                                                           // 1 storage, 1 max, but 1 overflows the entire new combined storage, so truncate nothing
            test(
                &mut infos,
                max_storages,
                NonZeroU64::new(ideal_storage_size_large).unwrap(),
            );
            assert_eq!(infos.all_infos.len(), usize::from(!filter));

            let mut infos = create_test_infos(1);
            let max_storages = 2;
            // all truncated because these infos will fit into the # storages
            test(
                &mut infos,
                max_storages,
                NonZeroU64::new(ideal_storage_size_large).unwrap(),
            );
            assert!(infos.all_infos.is_empty());

            let mut infos = create_test_infos(1);
            infos.all_infos[0].alive_bytes = ideal_storage_size_large + 1;
            let max_storages = 2;
            // none truncated because the one storage calculates to be larger than 1 ideal storage, so we need to
            // combine
            test(
                &mut infos,
                max_storages,
                NonZeroU64::new(ideal_storage_size_large).unwrap(),
            );
            assert_eq!(
                infos
                    .all_infos
                    .iter()
                    .map(|info| info.slot)
                    .collect::<Vec<_>>(),
                if filter { Vec::default() } else { vec![0] }
            );

            // both need to be combined to reach '1'
            let max_storages = 1;
            for ideal_storage_size in [1, 2] {
                let mut infos = create_test_infos(2);
                test(
                    &mut infos,
                    max_storages,
                    NonZeroU64::new(ideal_storage_size).unwrap(),
                );
                assert_eq!(infos.all_infos.len(), 2);
            }

            // max is 3
            // 4 storages
            // storage[3] is big enough to cause us to need another storage
            // so, storage[0] and [1] can be combined into 1, resulting in 3 remaining storages, which is
            // the goal, so we only have to combine the first 2 to hit the goal
            let mut infos = create_test_infos(4);
            infos.all_infos[3].alive_bytes = ideal_storage_size_large;
            let max_storages = 3;
            test(
                &mut infos,
                max_storages,
                NonZeroU64::new(ideal_storage_size_large).unwrap(),
            );
            assert_eq!(
                infos
                    .all_infos
                    .iter()
                    .map(|info| info.slot)
                    .collect::<Vec<_>>(),
                vec![0, 1]
            );
        }
    }

    #[test]
    fn test_calc_ancient_slot_info_one_shrink_one_not() {
        let can_randomly_shrink = false;
        for method in TestCollectInfo::iter() {
            for slot1_shrink in [false, true] {
                let shrinks = vec![false /*dummy*/, slot1_shrink, !slot1_shrink];
                let slots = 2;
                // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
                let data_sizes = shrinks
                    .iter()
                    .map(|shrink| (!shrink).then_some(1_040_000))
                    .collect::<Vec<_>>();
                let (db, slot1) =
                    create_db_with_storages_and_index(true /*alive*/, 1, data_sizes[1]);
                let dead_bytes = 184; // constant based on None data size
                create_storages_and_update_index(
                    &db,
                    None,
                    slot1 + 1,
                    1,
                    true,
                    data_sizes[(slot1 + 1) as usize],
                );

                assert_eq!(slot1, 1); // make sure index into shrinks will be correct
                assert_eq!(shrinks[slot1 as usize], slot1_shrink);
                let slot_vec = (slot1..(slot1 + slots as Slot)).collect::<Vec<_>>();
                let storages = slot_vec
                    .iter()
                    .map(|slot| {
                        let storage = db.storage.get_slot_storage_entry(*slot).unwrap();
                        assert_eq!(*slot, storage.slot());
                        storage
                    })
                    .collect::<Vec<_>>();
                let alive_bytes_expected = storages
                    .iter()
                    .map(|storage| storage.alive_bytes() as u64)
                    .sum::<u64>();
                let infos = match method {
                    TestCollectInfo::CalcAncientSlotInfo => {
                        db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink)
                    }
                    TestCollectInfo::Add => {
                        continue; // unsupportable
                    }
                    TestCollectInfo::CollectSortFilterInfo => {
                        let tuning = PackedAncientStorageTuning {
                            percent_of_alive_shrunk_data: 100,
                            max_ancient_slots: 0,
                            // irrelevant
                            ideal_storage_size: NonZeroU64::new(1).unwrap(),
                            can_randomly_shrink,
                        };
                        // note this can sort infos.all_infos
                        db.collect_sort_filter_ancient_slots(slot_vec.clone(), &tuning)
                    }
                };

                assert_eq!(infos.all_infos.len(), 2);
                storages.iter().for_each(|storage| {
                    assert!(infos
                        .all_infos
                        .iter()
                        .any(|info| info.slot == storage.slot()));
                });
                // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                assert_eq!(
                    infos.shrink_indexes,
                    if matches!(method, TestCollectInfo::CollectSortFilterInfo) {
                        Vec::default()
                    } else {
                        shrinks
                            .iter()
                            .skip(1)
                            .enumerate()
                            .filter_map(|(i, shrink)| shrink.then_some(i))
                            .collect::<Vec<_>>()
                    }
                );
                assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
                assert_eq!(infos.total_alive_bytes_shrink, dead_bytes);
            }
        }
    }

    #[test]
    fn test_clear_should_shrink_after_cutoff_empty() {
        let mut infos = create_test_infos(2);
        for count in 0..2 {
            for i in 0..count {
                infos.all_infos[i].should_shrink = true;
            }
        }
        infos.clear_should_shrink_after_cutoff(100);
        assert_eq!(
            0,
            infos
                .all_infos
                .iter()
                .filter_map(|info| info.should_shrink.then_some(()))
                .count()
        );
    }

    #[derive(EnumIter, Debug, PartialEq, Eq)]
    enum TestWriteAncient {
        OnePackedStorage,
        AncientAccounts,
        PackedStorages,
    }

    #[test]
    fn test_write_ancient_accounts() {
        for data_size in [None, Some(10_000_000)] {
            for method in TestWriteAncient::iter() {
                for num_slots in 0..4 {
                    for combine_into in 0..=num_slots {
                        if combine_into == num_slots && num_slots > 0 {
                            // invalid combination when num_slots > 0, but required to hit num_slots=0, combine_into=0
                            continue;
                        }
                        let (db, storages, slots, _infos) =
                            get_sample_storages(num_slots, data_size);

                        let initial_accounts = get_all_accounts(&db, slots.clone());

                        let accounts_vecs = storages
                            .iter()
                            .map(|storage| (storage.slot(), storage.accounts.accounts(0)))
                            .collect::<Vec<_>>();
                        // reshape the data
                        let accounts_vecs2 = accounts_vecs
                            .iter()
                            .map(|(slot, accounts)| (*slot, accounts.iter().collect::<Vec<_>>()))
                            .collect::<Vec<_>>();
                        let accounts = accounts_vecs2
                            .iter()
                            .map(|(slot, accounts)| (*slot, &accounts[..]))
                            .collect::<Vec<_>>();

                        let target_slot = slots.clone().nth(combine_into).unwrap_or(slots.start);
                        let accounts_to_write = StorableAccountsBySlot::new(
                            target_slot,
                            &accounts,
                            INCLUDE_SLOT_IN_HASH_TESTS,
                        );

                        let bytes = storages
                            .iter()
                            .map(|storage| storage.written_bytes())
                            .sum::<u64>();
                        assert_eq!(
                            bytes,
                            initial_accounts
                                .iter()
                                .map(|(_, account)| aligned_stored_size(account.data().len()) as u64)
                                .sum::<u64>()
                        );

                        if num_slots > 0 {
                            let mut write_ancient_accounts = WriteAncientAccounts::default();

                            match method {
                                TestWriteAncient::AncientAccounts => db.write_ancient_accounts(
                                    bytes,
                                    accounts_to_write,
                                    &mut write_ancient_accounts,
                                ),

                                TestWriteAncient::OnePackedStorage => {
                                    let packed = PackedAncientStorage { accounts, bytes };
                                    db.write_one_packed_storage(
                                        &packed,
                                        target_slot,
                                        &mut write_ancient_accounts,
                                    );
                                }
                                TestWriteAncient::PackedStorages => {
                                    let packed = PackedAncientStorage { accounts, bytes };

                                    let accounts_to_combine = AccountsToCombine {
                                        // target slots are supposed to be read in reverse order, so test that
                                        target_slots_sorted: vec![
                                            Slot::MAX, // this asserts if it gets used
                                            Slot::MAX,
                                            target_slot,
                                        ],
                                        ..AccountsToCombine::default()
                                    };

                                    write_ancient_accounts = db
                                        .write_packed_storages(&accounts_to_combine, vec![packed]);
                                }
                            };
                            let mut result = write_ancient_accounts.shrinks_in_progress;
                            let one = result.drain().collect::<Vec<_>>();
                            assert_eq!(1, one.len());
                            assert_eq!(target_slot, one.first().unwrap().0);
                            assert_eq!(
                                one.first().unwrap().1.old_storage().append_vec_id(),
                                storages[combine_into].append_vec_id()
                            );
                            // make sure the single new append vec contains all the same accounts
                            let accounts_in_new_storage =
                                one.first().unwrap().1.new_storage().accounts.accounts(0);
                            compare_all_accounts(
                                &initial_accounts,
                                &accounts_in_new_storage
                                    .into_iter()
                                    .map(|meta| (*meta.pubkey(), meta.to_account_shared_data()))
                                    .collect::<Vec<_>>()[..],
                            );
                        }
                        let all_accounts = get_all_accounts(&db, target_slot..(target_slot + 1));

                        compare_all_accounts(&initial_accounts, &all_accounts);
                    }
                }
            }
        }
    }

    #[derive(EnumIter, Debug, PartialEq, Eq)]
    enum TestShouldShrink {
        FilterAncientSlots,
        ClearShouldShrink,
        ChooseStoragesToShrink,
    }

    #[test]
    fn test_clear_should_shrink_after_cutoff_simple() {
        for swap in [false, true] {
            for method in TestShouldShrink::iter() {
                for (percent_of_alive_shrunk_data, mut expected_infos) in
                    [(0, 0), (9, 1), (10, 1), (89, 2), (90, 2), (91, 2), (100, 2)]
                {
                    let mut infos = create_test_infos(2);
                    infos
                        .all_infos
                        .iter_mut()
                        .enumerate()
                        .for_each(|(i, info)| {
                            info.should_shrink = true;
                            info.capacity = ((i + 1) * 1000) as u64;
                        });
                    infos.all_infos[0].alive_bytes = 100;
                    infos.all_infos[1].alive_bytes = 900;
                    if swap {
                        infos.all_infos = infos.all_infos.into_iter().rev().collect();
                    }
                    infos.total_alive_bytes_shrink =
                        infos.all_infos.iter().map(|info| info.alive_bytes).sum();
                    match method {
                        TestShouldShrink::FilterAncientSlots => {
                            let tuning = PackedAncientStorageTuning {
                                percent_of_alive_shrunk_data,
                                // 0 so that we combine everything with regard to the overall # of slots limit
                                max_ancient_slots: 0,
                                // this is irrelevant since the limit is artificially 0
                                ideal_storage_size: NonZeroU64::new(1).unwrap(),
                                can_randomly_shrink: false,
                            };
                            infos.filter_ancient_slots(&tuning);
                        }
                        TestShouldShrink::ClearShouldShrink => {
                            infos.clear_should_shrink_after_cutoff(percent_of_alive_shrunk_data);
                        }
                        TestShouldShrink::ChooseStoragesToShrink => {
                            infos.choose_storages_to_shrink(percent_of_alive_shrunk_data);
                        }
                    }

                    if expected_infos == 2 {
                        let modify = if method == TestShouldShrink::FilterAncientSlots {
                            // filter_ancient_slots modifies in several ways and doesn't retain the values to compare
                            percent_of_alive_shrunk_data == 89 || percent_of_alive_shrunk_data == 90
                        } else {
                            infos.all_infos[infos.shrink_indexes[0]].alive_bytes
                                >= infos.total_alive_bytes_shrink * percent_of_alive_shrunk_data
                                    / 100
                        };
                        if modify {
                            // if the sorting ends up putting the bigger alive_bytes storage first, then only 1 will be shrunk due to 'should_shrink'
                            expected_infos = 1;
                        }
                    }
                    let count = infos
                        .all_infos
                        .iter()
                        .filter_map(|info| info.should_shrink.then_some(()))
                        .count();
                    assert_eq!(
                        expected_infos,
                        count,
                        "percent_of_alive_shrunk_data: {percent_of_alive_shrunk_data}, infos: {expected_infos}, method: {method:?}, swap: {swap}, data: {:?}",
                        infos.all_infos.iter().map(|info| (info.slot, info.capacity, info.alive_bytes)).collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    #[test]
    fn test_sort_shrink_indexes_by_bytes_saved() {
        let (db, slot1) = create_db_with_storages_and_index(true /*alive*/, 1, None);
        let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
        // ignored
        let slot = 0;

        // info1 is first, equal, last
        for info1_capacity in [0, 1, 2] {
            let info1 = SlotInfo {
                storage: storage.clone(),
                slot,
                capacity: info1_capacity,
                alive_bytes: 0,
                should_shrink: false,
            };
            let info2 = SlotInfo {
                storage: storage.clone(),
                slot,
                capacity: 2,
                alive_bytes: 1,
                should_shrink: false,
            };
            let mut infos = AncientSlotInfos {
                all_infos: vec![info1, info2],
                shrink_indexes: vec![0, 1],
                ..AncientSlotInfos::default()
            };
            infos.sort_shrink_indexes_by_bytes_saved();
            let first = &infos.all_infos[infos.shrink_indexes[0]];
            let second = &infos.all_infos[infos.shrink_indexes[1]];
            let first_capacity = first.capacity - first.alive_bytes;
            let second_capacity = second.capacity - second.alive_bytes;
            assert!(first_capacity >= second_capacity);
        }
    }

    #[test]
    fn test_combine_ancient_slots_packed_internal() {
        let can_randomly_shrink = false;
        let alive = true;
        for num_slots in 0..4 {
            for max_ancient_slots in 0..4 {
                let (db, slot1) = create_db_with_storages_and_index(alive, num_slots, None);
                let original_stores = (0..num_slots)
                    .filter_map(|slot| db.storage.get_slot_storage_entry((slot as Slot) + slot1))
                    .collect::<Vec<_>>();
                let original_results = original_stores
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();

                let tuning = PackedAncientStorageTuning {
                    percent_of_alive_shrunk_data: 0,
                    max_ancient_slots,
                    can_randomly_shrink,
                    ideal_storage_size: NonZeroU64::new(get_ancient_append_vec_capacity()).unwrap(),
                };
                db.combine_ancient_slots_packed_internal(
                    (0..num_slots).map(|slot| (slot as Slot) + slot1).collect(),
                    tuning,
                    &mut ShrinkStatsSub::default(),
                );
                let storage = db.storage.get_slot_storage_entry(slot1);
                if num_slots == 0 {
                    assert!(storage.is_none());
                    continue;
                }
                // any of the several slots could have been chosen to be re-used
                let active_slots = (0..num_slots)
                    .filter_map(|slot| db.storage.get_slot_storage_entry((slot as Slot) + slot1))
                    .count();
                let mut expected_slots = max_ancient_slots.min(num_slots);
                if max_ancient_slots == 0 {
                    expected_slots = 1;
                }
                assert_eq!(
                    active_slots, expected_slots,
                    "slots: {num_slots}, max_ancient_slots: {max_ancient_slots}, alive: {alive}"
                );
                assert_eq!(
                    expected_slots,
                    db.storage.all_slots().len(),
                    "slots: {num_slots}, max_ancient_slots: {max_ancient_slots}"
                );

                let stores = (0..num_slots)
                    .filter_map(|slot| db.storage.get_slot_storage_entry((slot as Slot) + slot1))
                    .collect::<Vec<_>>();
                let results = stores
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();
                let all_accounts = get_all_accounts(&db, slot1..(slot1 + num_slots as Slot));
                compare_all_accounts(&vec_unique_to_accounts(&original_results), &all_accounts);
                compare_all_accounts(
                    &vec_unique_to_accounts(&results),
                    &get_all_accounts(&db, slot1..(slot1 + num_slots as Slot)),
                );
            }
        }
    }

    fn vec_unique_to_accounts(one: &[GetUniqueAccountsResult]) -> Vec<(Pubkey, AccountSharedData)> {
        one.iter()
            .flat_map(|result| {
                result
                    .stored_accounts
                    .iter()
                    .map(|result| (*result.pubkey(), result.to_account_shared_data()))
            })
            .collect()
    }

    #[test]
    fn test_combine_packed_ancient_slots_simple() {
        for alive in [false, true] {
            _ = get_one_packed_ancient_append_vec_and_others(alive, 0);
        }
    }

    #[test]
    fn test_shrink_packed_ancient() {
        solana_logger::setup();

        let num_normal_slots = 1;
        // build an ancient append vec at slot 'ancient_slot'
        let (db, ancient_slot) =
            get_one_packed_ancient_append_vec_and_others(true, num_normal_slots);

        let max_slot_inclusive = ancient_slot + (num_normal_slots as Slot);
        let initial_accounts = get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1));
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // combine normal append vec(s) into existing ancient append vec
        db.combine_ancient_slots_packed(
            (ancient_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        // create a 2nd ancient append vec at 'next_slot'
        let next_slot = max_slot_inclusive + 1;
        create_storages_and_update_index(&db, None, next_slot, num_normal_slots, true, None);
        let max_slot_inclusive = next_slot + (num_normal_slots as Slot);

        let initial_accounts = get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1));
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        db.combine_ancient_slots_packed(
            (next_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );
    }
}
