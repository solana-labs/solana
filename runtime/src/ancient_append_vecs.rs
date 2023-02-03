//! helpers for squashing append vecs into ancient append vecs
//! an ancient append vec is:
//! 1. a slot that is older than an epoch old
//! 2. multiple 'slots' squashed into a single older (ie. ancient) slot for convenience and performance
//! Otherwise, an ancient append vec is the same as any other append vec
use {
    crate::{
        account_storage::ShrinkInProgress,
        accounts_db::{
            AccountStorageEntry, AccountsDb, AliveAccounts, GetUniqueAccountsResult, ShrinkCollect,
            ShrinkCollectAliveSeparatedByRefs, ShrinkStatsSub, StoreReclaims,
            INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
        },
        accounts_index::ZeroLamport,
        active_stats::ActiveStatItem,
        append_vec::{AppendVec, StoredAccountMeta},
        storable_accounts::{StorableAccounts, StorableAccountsBySlot},
    },
    rand::{thread_rng, Rng},
    solana_measure::{measure, measure::Measure},
    solana_sdk::{account::ReadableAccount, clock::Slot, hash::Hash, saturating_add_assign},
    std::{
        collections::{HashMap, HashSet},
        sync::{atomic::Ordering, Arc},
    },
};

/// determine packing algorithm tuning per pass
struct PackedAncientStorageTuning {
    // shrink enough of these ancient append vecs to realize this% of the total dead data that needs to be shrunk
    // Doing too much burns too much time and disk i/o.
    // Doing too little could cause us to never catch up and have old data accumulate.
    percent_of_shrunk_data: u64,
    // number of ancient slots we should aim to have. If we have more than this, combine further.
    max_ancient_slots: usize,
    can_randomly_shrink: bool,
}

#[derive(Default, Debug)]
struct PackedAncientStorage<'a> {
    bytes: u64,
    accounts: Vec<(Slot, &'a [&'a StoredAccountMeta<'a>])>,
}

/// info about a storage eligible to be combined into an ancient append vec.
/// Useful to help sort vecs of storages.
#[derive(Debug)]
struct StorageInfo {
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
    all_infos: Vec<StorageInfo>,
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
                use log::*;
                error!(
                    "alive ratio: {}, alive bytes: {alive_bytes}, capacity: {capacity}",
                    alive_ratio
                );
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
            self.all_infos.push(StorageInfo {
                slot,
                capacity,
                storage,
                alive_bytes,
                should_shrink,
            });
            self.total_alive_bytes += alive_bytes;
        }
    }


    /// modify 'self' to contain only the slot infos for the slots that should be combined (and in the process effectively shrunk)
    fn sort_and_filter_ancient_slots(
        &mut self,
        tuning: PackedAncientStorageTuning,
    ) {
        // figure out which slots to combine
        /*
        should_shrink: largest bytes saved above some cutoff of ratio
        smallest files so we get the largest number of files to compact
        - certainly ones with ALL accounts with 1 refcount
        later storages with widest range of pubkeys
        */

        let min_bytes_to_shrink = get_ancient_append_vec_capacity();
        let min_slots_to_combine = self
            .all_infos
            .len()
            .saturating_sub(tuning.max_ancient_slots);

        if self.total_alive_bytes < min_bytes_to_shrink && min_slots_to_combine == 0 {
            // too few bytes to combine, and not too many files, so nothing to do
            *self = AncientSlotInfos::default();
            return;
        }

        self.mark_storages_to_shrink(tuning.percent_of_shrunk_data);
        use log::*;
        //error!("infos after ratio: {self:?}");
        self.filter_by_smallest_capacity(tuning.max_ancient_slots);
        use log::*;
        //error!("infos after capacity: {self:?}");
    }  

    fn sort_shrink_indexes_by_bytes_saved(&mut self) {
        self.shrink_indexes.sort_by(|l, r| {
            let sort_value = |index: &usize| {
                let item = &self.all_infos[*index];
                item.capacity - item.alive_bytes
            };
            // so sort by most bytes saved, highest to lowest
            sort_value(r).cmp(&sort_value(l))
        });
    }

    fn clear_should_shrink_after_cutoff(&mut self, percent_of_shrunk_data: u64) {
        let mut bytes_to_shrink_due_to_ratio = 0;
        // shrink enough slots to save 'percent_of_shrunk_data'% of the data we can save
        let threhold_bytes = self.total_alive_bytes_shrink * percent_of_shrunk_data / 100;
        for info_index in &self.shrink_indexes {
            let info = &mut self.all_infos[*info_index];
            if bytes_to_shrink_due_to_ratio >= threhold_bytes {
                // we exceeded the amount to shrink due to alive ratio, so don't shrink this one just due to ratio
                // It MAY be shrunk based on total capacity still.
                // Mark it as false for 'ratio_shrink' so it gets evaluated solely based on # of files.
                info.should_shrink = false;
            } else {
                saturating_add_assign!(bytes_to_shrink_due_to_ratio, info.alive_bytes);
            }
        }
    }

    fn mark_storages_to_shrink(&mut self, percent_of_shrunk_data: u64) {
        // sort the shrink_ratio_slots by most bytes saved to fewest
        // most bytes saved is more valuable to shrink
        self.sort_shrink_indexes_by_bytes_saved();

        self.clear_should_shrink_after_cutoff(percent_of_shrunk_data);
    }

    fn truncate_to_max_storages(&mut self, max_storages: usize) {
        let total_storages = self.all_infos.len();
        let ideal_size = get_ancient_append_vec_capacity();
        let mut cumulative_bytes = 0u64;
        for (i, info) in self.all_infos.iter().enumerate() {
            saturating_add_assign!(cumulative_bytes, info.alive_bytes);
            let ancient_storages_required = (cumulative_bytes / ideal_size + 1) as usize;
            let storages_remaining = total_storages - i;
            if storages_remaining + ancient_storages_required <= max_storages {
                use log::*;
                error!("truncate to: {i}, len: {}", total_storages);
                self.all_infos.truncate(i);
                break;
            }
        }
    }

    fn filter_by_smallest_capacity(&mut self, max_storages: usize) {
        let total_storages = self.all_infos.len();
        if total_storages <= max_storages {
            // currently fewer storages than max, so nothing to shrink
            self.all_infos.clear();
            return;
        }

        // sort by ratio shrink then smallest to largest to keep the total # of ancient append vecs <= 'max_storages'
        self.all_infos.sort_by(|l, r| {
            l.should_shrink
                .cmp(&r.should_shrink)
                .reverse()
                .then_with(|| l.capacity.cmp(&r.capacity))
        });

        self.truncate_to_max_storages(max_storages);
    }
}

/// hold all alive accounts to be shrunk and/or combined
struct AccountsToCombine<'a> {
    /// slots and alive accounts that must remain in the slot they are currently in
    /// because the account exists in more than 1 slot in accounts db
    /// This hashmap contains an entry for each slot that contains at least one account with ref_count > 1.
    /// The value of the entry is all alive accounts in that slot whose ref_count > 1.
    /// Any OTHER accounts in that slot whose ref_count = 1 are in 'accounts_to_combine' because they can be moved
    /// to any slot.
    /// We want to keep the ref_count > 1 accounts by themselves, expecting the multiple ref_counts will be resolved
    /// soon and we can clean the duplicates up (which maybe THIS one).
    accounts_keep_slots: HashMap<Slot, (AliveAccounts<'a>, &'a StorageInfo)>,
    /// all the rest of alive accounts that can move slots and should be combined
    /// This includes all accounts with ref_count = 1 from the slots in 'accounts_keep_slots'
    accounts_to_combine: Vec<ShrinkCollect<'a, ShrinkCollectAliveSeparatedByRefs<'a>>>,
    /// slots that contain alive accounts that can move into ANY other ancient slot
    /// these slots will NOT be in 'accounts_keep_slots'
    /// Some of these slots will have ancient append vecs created at them to contain everything in 'accounts_to_combine'
    /// The rest will become dead slots with no accounts in them.
    target_slots: Vec<Slot>,
}

/// result of writing ancient accounts with a single refcount
struct WriteAncientAccountsOneRef<'a> {
    write_ancient_accounts: WriteAncientAccounts<'a>,
    dropped_roots: HashSet<Slot>,
}

/// result of writing ancient accounts
#[derive(Debug, Default)]
struct WriteAncientAccounts<'a> {
    /// 'ShrinkInProgress' instances created by starting a shrink operation
    shrinks_in_progress: HashMap<Slot, ShrinkInProgress<'a>>,

    metrics: ShrinkStatsSub,
}

impl<'a> WriteAncientAccounts<'a> {
    pub(crate) fn accumulate(&mut self, mut other: Self) {
        self.metrics.accumulate(&other.metrics);
        other.shrinks_in_progress.drain().for_each(|(k, v)| {
            self.shrinks_in_progress.insert(k, v);
        });
    }
}

impl AccountsDb {
    pub(crate) fn combine_ancient_slots_new(&self, slots: Vec<Slot>, can_randomly_shrink: bool) {
        let tuning = PackedAncientStorageTuning {
            // only allow 10k slots old enough to be ancient
            max_ancient_slots: 10_000,
            // re-combine/shrink 55% of the data savings this pass
            percent_of_shrunk_data: 55,
            can_randomly_shrink,
        };
        self.combine_ancient_slots_packed(slots, tuning);
    }

    /// calculate all storage info for the storages in slots
    /// prune the ones to ignore given tuning parameters
    fn collect_sort_filter_info(
        &self,
        slots: Vec<Slot>,
        tuning: PackedAncientStorageTuning,
    ) -> AncientSlotInfos {
        let mut ancient_slot_infos = self.calc_ancient_slot_info(slots, tuning.can_randomly_shrink);

        self.sort_and_filter_ancient_slots(&mut ancient_slot_infos, tuning);
        //error!("infos2: {ancient_slot_infos:?}");
        ancient_slot_infos
    }

    /// Combine all account data from storages in 'slots' into ancient append vecs.
    /// This keeps us from accumulating append vecs for each slot older than an epoch.
    /// Ater this function the number of alive roots is <= # alive roots when it was called.
    /// In practice, the # of alive roots will be significantly less than # alive roots when called.
    /// Trying to reduce # roots and append vecs (one per root) required to store all the data in ancient slots
    #[allow(dead_code)]
    fn combine_ancient_slots_packed(&self, slots: Vec<Slot>, tuning: PackedAncientStorageTuning) {
        if slots.is_empty() {
            return;
        }
        let mut total = Measure::start("combine_ancient_slots");
        let _guard = self.active_stats.activate(ActiveStatItem::SquashAncient);

        let ancient_slot_infos = self.collect_sort_filter_info(slots, tuning);

        use log::*;
        error!("combining: {}", ancient_slot_infos.all_infos.len());
        let mut stats_sub = ShrinkStatsSub::default();

        if !ancient_slot_infos.all_infos.is_empty() {
            let mut storage_accounts_to_combine =
                Vec::with_capacity(ancient_slot_infos.all_infos.len());
            self.get_unique_accounts_from_storage_for_combining_ancient_slots(
                &ancient_slot_infos.all_infos[..],
                &mut storage_accounts_to_combine,
            );

            let mut accounts_to_combine =
                self.calc_accounts_to_combine(&storage_accounts_to_combine);

            // write accounts which have ref count > 1. These must be written IN their same slot.
            let mut write_ancient_accounts =
                self.write_ancient_accounts_multiple_refs(&accounts_to_combine);
            stats_sub.accumulate(&write_ancient_accounts.metrics);

            // combine accounts that can move (ie. ref_count=1) into as few ideally sized larger append vecs as possible
            accounts_to_combine.target_slots.sort();
            let mut shrink_in_progress_all_one_ref = self
                .write_ancient_accounts_one_ref(&storage_accounts_to_combine, &accounts_to_combine);
            stats_sub.accumulate(
                &shrink_in_progress_all_one_ref
                    .write_ancient_accounts
                    .metrics,
            );

            let dropped_roots = std::mem::take(&mut shrink_in_progress_all_one_ref.dropped_roots);

            for shrink_collect in accounts_to_combine.accounts_to_combine {
                let slot = shrink_collect.slot;
                if dropped_roots.contains(&slot) {
                    self.remove_old_stores_shrink(
                        &shrink_collect,
                        &self.shrink_ancient_stats.shrink_stats,
                        None,
                    );
                } else {
                    let shrink_in_progress = write_ancient_accounts
                        .shrinks_in_progress
                        .remove(&slot)
                        .or_else(|| {
                            shrink_in_progress_all_one_ref
                                .write_ancient_accounts
                                .shrinks_in_progress
                                .remove(&slot)
                        })
                        .unwrap();
                    self.remove_old_stores_shrink(
                        &shrink_collect,
                        &self.shrink_ancient_stats.shrink_stats,
                        Some(shrink_in_progress),
                    );
                }
            }

            // these can be be dropped after all accounts have been moved and the index has been updated
            // Importantly, these drop ShrinkInProgress instances, which removes the shrunk append vecs
            drop(write_ancient_accounts);
            drop(shrink_in_progress_all_one_ref);

            self.handle_dropped_roots_for_ancient(dropped_roots.into_iter());
        }

        let find_alive_elapsed = Measure::start("");
        Self::update_shrink_stats(
            &self.shrink_ancient_stats.shrink_stats,
            find_alive_elapsed.as_us(),
            stats_sub,
        );
        total.stop();
        self.shrink_ancient_stats
            .total_us
            .fetch_add(total.as_us(), Ordering::Relaxed);

        // only log when we've spent 1s total
        // results will continue to accumulate otherwise
        if self.shrink_ancient_stats.total_us.load(Ordering::Relaxed) > 1_000_000 {
            self.shrink_ancient_stats.report();
        }
    }

    /// go through all slots and populate 'StorageInfo', per slot
    /// This provides the list of possible ancient slots to sort, filter, and then combine.
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

    /// todo doc
    fn get_unique_accounts_from_storage_for_combining_ancient_slots<'a>(
        &self,
        ancient_slots: &'a [StorageInfo],
        accounts_to_combine: &mut Vec<(&'a StorageInfo, GetUniqueAccountsResult<'a>)>,
    ) {
        for info in ancient_slots {
            let unique_accounts = self.get_unique_accounts_from_storage_for_shrink(
                &info.storage,
                &self.shrink_ancient_stats.shrink_stats,
            );
            accounts_to_combine.push((info, unique_accounts));
        }
    }

    /// given all accounts per ancient slot, in slots that we want to combine together:
    /// look up each pubkey in the index, separate, by slot, into:
    /// 1. pubkeys with refcount = 1. This means this pubkey exists NOWHERE else in accounts db.
    /// 2. pubkeys with refcount > 1
    fn calc_accounts_to_combine<'a>(
        &self,
        stored_accounts_all: &'a Vec<(&'a StorageInfo, GetUniqueAccountsResult<'a>)>,
    ) -> AccountsToCombine<'a> {
        let mut accounts_keep_slots = HashMap::default(); //<Slot, (&ShrinkCollect<ShrinkCollectAliveSeparatedByRefs>, &StorageInfo)>::default();
        let mut target_slots = Vec::default();

        let mut accounts_to_combine = Vec::default();
        for (info, unique_accounts) in stored_accounts_all {
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
                accounts_keep_slots.insert(info.slot, (std::mem::take(many_refs), *info));
            } else {
                // No alive accounts in this slot have a ref_count > 1. So, ALL alive accounts in this slot can be written to any other slot
                // we find convenient. There is NO other instance of this account to conflict with.
                target_slots.push(info.slot);
            }
            accounts_to_combine.push(shrink_collect);
        }
        AccountsToCombine {
            accounts_to_combine,
            accounts_keep_slots,
            target_slots,
        }
    }

    /// create append vec of size 'bytes'
    /// write 'accounts_to_write' into it
    /// return some metrics and shrink_in_progress
    fn write_ancient_accounts<'a, 'b: 'a, T: ReadableAccount + Sync + ZeroLamport + 'a>(
        &'b self,
        bytes: u64,
        accounts_to_write: impl StorableAccounts<'a, T>,
    ) -> WriteAncientAccounts<'b> {
        let target_slot = accounts_to_write.target_slot();
        let (shrink_in_progress, create) = measure!(self.get_store_for_shrink(target_slot, bytes));
        let (store_accounts_timing, rewrite_elapsed) = measure!(self.store_accounts_frozen(
            accounts_to_write,
            None::<Vec<Hash>>,
            Some(shrink_in_progress.new_storage()),
            None,
            StoreReclaims::Ignore,
        ));
        WriteAncientAccounts {
            shrinks_in_progress: HashMap::from([(target_slot, shrink_in_progress)]),
            metrics: ShrinkStatsSub {
                store_accounts_timing,
                rewrite_elapsed_us: rewrite_elapsed.as_us(),
                create_and_insert_store_elapsed_us: create.as_us(),
            },
        }
    }

    /// 'accounts_to_combine'.'accounts_keep_slots' contains the slot and alive accounts with ref_count > 1.
    /// these accounts need to be rewritten in their same slot, with no other accounts in the slot, where other accounts
    /// would have ref_count = 1. ref_count = 1 accounts would be combined together into other slots into larger append vecs.
    /// for each slot in 'accounts_to_combine'.'accounts_keep_slots':
    /// create a new append vec, write only the accounts with ref_count > 1
    fn write_ancient_accounts_multiple_refs<'a, 'b: 'a>(
        &'b self,
        accounts_to_combine: &'a AccountsToCombine<'a>,
    ) -> WriteAncientAccounts<'b> {
        // todo: if we're just rewriting the same append vec contents as are already there, skip this
        // but only if there are no ref_count == 1 accounts in the existing append vec
        let mut write_ancient_accounts = WriteAncientAccounts::default();
        use log::*;
        error!(
            "write_ancient_accounts_multiple_refs, {:?}",
            accounts_to_combine.accounts_keep_slots.len()
        );

        for (target_slot, alive_accounts) in &accounts_to_combine.accounts_keep_slots {
            let accounts_to_write = (
                *target_slot,
                &alive_accounts.0.accounts[..],
                INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
            );
            write_ancient_accounts.accumulate(
                self.write_ancient_accounts(alive_accounts.0.bytes as u64, accounts_to_write),
            );
        }
        write_ancient_accounts
    }

    fn pack_ancient_storages<'a>(
        stored_accounts_all: &Vec<(&'a StorageInfo, GetUniqueAccountsResult<'a>)>,
        accounts_to_combine: &'a AccountsToCombine<'a>,
    ) -> Vec<PackedAncientStorage<'a>> {
        let mut accounts_index = 0;
        let mut result = Vec::default();
        let ideal_size = get_ancient_append_vec_capacity();
        for _ in accounts_to_combine.target_slots.iter() {
            // fill up a new append vec at 'slot'
            let mut bytes_total = 0usize;
            let mut accounts_to_write = Vec::default();
            while accounts_index < stored_accounts_all.len() {
                let info = &stored_accounts_all[accounts_index];
                let shrink_collect = &accounts_to_combine.accounts_to_combine[accounts_index];
                let bytes_this_slot = shrink_collect.alive_accounts.one_ref.bytes;
                if bytes_this_slot >= ideal_size as usize && accounts_to_write.is_empty() {
                    // We encountered an append vec larger than our ideal size
                    // and we already had accounts from another append vec ready to write.
                    // So, stop here with what we already had.
                    // Then, rewrite the large append vec into its own shrunk append vec.
                    break;
                }
                saturating_add_assign!(bytes_total, bytes_this_slot);
                accounts_to_write.push((
                    info.0.slot,
                    &shrink_collect.alive_accounts.one_ref.accounts[..],
                ));
                if bytes_total > ideal_size as usize {
                    break;
                }
                accounts_index += 1;
            }
            if !accounts_to_write.is_empty() {
                result.push(PackedAncientStorage {
                    bytes: bytes_total as u64,
                    accounts: accounts_to_write,
                });
            }
        }
        result
    }

    /// 'accounts_to_combine'.'accounts_to_combine' contains alive accounts with ref_count = 1.
    /// these accounts can be combined in any order in any slot, with accounts from any other slot.
    /// for each slot in 'accounts_to_combine'.'target_slots':
    /// create a new append vec, write as many accounts as we ideally fit into the new append vec until we have
    /// run out of accounts to write
    fn write_ancient_accounts_one_ref<'a, 'b: 'a>(
        &'b self,
        stored_accounts_all: &Vec<(&'a StorageInfo, GetUniqueAccountsResult<'a>)>,
        accounts_to_combine: &'a AccountsToCombine<'a>,
    ) -> WriteAncientAccountsOneRef<'b> {
        let mut write_ancient_accounts = WriteAncientAccounts::default();
        let mut dropped_roots = HashSet::default();
        let mut accounts_to_write =
            Self::pack_ancient_storages(stored_accounts_all, accounts_to_combine);
        // rev is to try to pick the highest slot # we can since it doesn't matter where we put these movable accounts
        // this keeps the range of alive roots as small as possible, which helps the alive_roots.is_root implementation.
        use log::*;
        /*
        error!(
            "write_ancient_accounts_one_ref, slots: {:?}, accounts_to_write: {:?}, target slots: {:?}",
            accounts_to_combine.target_slots, accounts_to_write, accounts_to_combine.target_slots,
        );*/
        for target_slot in accounts_to_combine.target_slots.iter().rev() {
            let PackedAncientStorage {
                bytes: bytes_total,
                accounts: accounts_to_write,
            } = accounts_to_write.pop().unwrap_or_default();

            if bytes_total == 0 {
                error!("write_ancient_accounts_one_ref: dropping slot: {target_slot}, bytes: {bytes_total}");
                dropped_roots.insert(*target_slot);
                continue;
            }

            error!("write_ancient_accounts_one_ref: slot: {target_slot}, bytes: {bytes_total}");

            let accounts_to_write = StorableAccountsBySlot::new(
                *target_slot,
                &accounts_to_write[..],
                INCLUDE_SLOT_IN_HASH_IRRELEVANT_APPEND_VEC_OPERATION,
            );
            let write_ancient_accounts_one =
                self.write_ancient_accounts(bytes_total, accounts_to_write);
            /*error!(
                "written: slot: {}, {write_ancient_accounts_one:?}",
                target_slot
            );
            error!(
                "slot: {:?}",
                self.storage
                    .get_slot_storage_entry_shrinking_in_progress_ok(*target_slot)
                    .unwrap()
            );*/
            write_ancient_accounts.accumulate(write_ancient_accounts_one);
        }

        error!("roots:{:?}", dropped_roots);

        WriteAncientAccountsOneRef {
            write_ancient_accounts,
            dropped_roots,
        }
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
/// The slice arithmetic accross both hashes and account data gets messy. So, this struct abstracts that.
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
                let account_size = account.stored_size as u64;
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
pub fn is_ancient(storage: &AppendVec) -> bool {
    storage.capacity() >= get_ancient_append_vec_capacity()
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            accounts_db::{
                get_temp_accounts_paths,
                tests::{
                    compare_all_accounts, create_db_with_storages_and_index,
                    create_storages_and_update_index, get_all_accounts, CAN_RANDOMLY_SHRINK_FALSE,
                },
            },
            append_vec::{AccountMeta, StoredAccountMeta, StoredMeta},
        },
        solana_sdk::{
            account::{AccountSharedData, ReadableAccount},
            hash::Hash,
            pubkey::Pubkey,
        },
    };

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
        let account = StoredAccountMeta {
            meta: &stored_meta,
            /// account data
            account_meta: &account_meta,
            data: account.data(),
            offset,
            stored_size: account_size,
            hash: &hash,
        };
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
            let av = AppendVec::new(&tf.path, true, size as usize);

            assert_eq!(expected_ancient, is_ancient(&av));
        }
    }

    fn get_one_packed_ancient_append_vec_and_others(
        alive: bool,
        num_normal_slots: usize,
    ) -> (AccountsDb, Slot) {
        let (db, slot1) = create_db_with_storages_and_index(alive, num_normal_slots + 1, None);
        let storage = db.get_storage_for_slot(slot1).unwrap();
        let created_accounts = db.get_unique_accounts_from_storage(&storage);

        db.combine_ancient_slots_new(vec![slot1], CAN_RANDOMLY_SHRINK_FALSE);
        assert!(db.storage.get_slot_storage_entry(slot1).is_some());
        let after_store = db.get_storage_for_slot(slot1).unwrap();
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

    fn assert_storage_info(
        info: &StorageInfo,
        storage: &Arc<AccountStorageEntry>,
        should_shrink: bool,
    ) {
        assert_eq!(storage.append_vec_id(), info.storage.append_vec_id());
        assert_eq!(storage.slot(), info.slot);
        assert_eq!(storage.capacity(), info.capacity);
        assert_eq!(storage.alive_bytes(), info.alive_bytes as usize);
        assert_eq!(should_shrink, info.should_shrink);
    }

    #[test]
    fn test_calc_ancient_slot_info_one_alive() {
        let can_randomly_shrink = false;
        let alive = true;
        let slots = 1;
        for call_add in [false, true] {
            // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
            for data_size in [None, Some(1_040_000)] {
                let (db, slot1) = create_db_with_storages_and_index(alive, slots, data_size);
                let mut infos = AncientSlotInfos::default();
                let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
                let alive_bytes_expected = storage.alive_bytes();
                if call_add {
                    // test lower level 'add'
                    infos.add(slot1, Arc::clone(&storage), can_randomly_shrink);
                } else {
                    infos = db.calc_ancient_slot_info(vec![slot1], can_randomly_shrink);
                }
                assert_eq!(infos.all_infos.len(), 1);
                let should_shrink = data_size.is_none();
                assert_storage_info(infos.all_infos.first().unwrap(), &storage, should_shrink);
                if should_shrink {
                    // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                    assert_eq!(infos.shrink_indexes, vec![0]);
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
            // 1_040_000 is big enough relative to page size to cause shrink ratio to be triggered
            let (db, slot1) = create_db_with_storages_and_index(alive, slots, None);
            let mut infos = AncientSlotInfos::default();
            let storage = db.storage.get_slot_storage_entry(slot1).unwrap();
            let alive_bytes_expected = storage.alive_bytes();
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
            for slots in 2..4 {
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
                                assert_storage_info(&info, storage, should_shrink);
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
                        storage.remove_account(storage.written_bytes() as usize, false);
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
                let infos = db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink);
                assert_eq!(infos.all_infos.len(), 1);
                let should_shrink = data_size.is_none();
                alive_storages
                    .iter()
                    .zip(infos.all_infos.iter())
                    .for_each(|(storage, info)| {
                        assert_storage_info(&info, storage, should_shrink);
                    });
                if should_shrink {
                    // data size is so small compared to min aligned file size that the storage is marked as should_shrink
                    assert_eq!(infos.shrink_indexes, vec![0]);
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

    #[test]
    fn test_calc_ancient_slot_info_one_shrink_one_not() {
        let can_randomly_shrink = false;
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
            let infos = db.calc_ancient_slot_info(slot_vec.clone(), can_randomly_shrink);
            assert_eq!(infos.all_infos.len(), 2);
            use log::*;
            error!(
                "shrinks {shrinks:?}, all infos: {:?}",
                infos
                    .all_infos
                    .iter()
                    .map(|r| r.storage.slot())
                    .collect::<Vec<_>>()
            );
            storages
                .iter()
                .zip(infos.all_infos.iter())
                .for_each(|(storage, info)| {
                    assert_storage_info(&info, storage, shrinks[storage.slot() as usize]);
                });
            // data size is so small compared to min aligned file size that the storage is marked as should_shrink
            assert_eq!(
                infos.shrink_indexes,
                shrinks
                    .iter()
                    .skip(1)
                    .enumerate()
                    .filter_map(|(i, shrink)| shrink.then_some(i))
                    .collect::<Vec<_>>()
            );
            assert_eq!(infos.total_alive_bytes, alive_bytes_expected);
            assert_eq!(infos.total_alive_bytes_shrink, dead_bytes);
        }
    }

    #[test]
    fn test_combine_ancient_slots_packed() {
        let can_randomly_shrink = false;
        let alive = true;
        for slots in 0..4 {
            for max_ancient_slots in 0..4 {
                let (db, slot1) = create_db_with_storages_and_index(alive, slots, None);
                let original_stores = (0..slots)
                    .filter_map(|slot| db.storage.get_slot_storage_entry((slot as Slot) + slot1))
                    .collect::<Vec<_>>();
                let original_results = original_stores
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();

                let tuning = PackedAncientStorageTuning {
                    percent_of_shrunk_data: 0,
                    max_ancient_slots,
                    can_randomly_shrink,
                };
                use log::*;
                error!("slots: {slots}, max_ancient_slots: {max_ancient_slots}");
                db.combine_ancient_slots_packed(
                    (0..slots).map(|slot| (slot as Slot) + slot1).collect(),
                    tuning,
                );
                let storage = db.storage.get_slot_storage_entry(slot1);
                if slots == 0 {
                    assert!(storage.is_none());
                    continue;
                }
                // any of the several slots could have been chosen to be re-used
                let active_slots = (0..slots)
                    .map(|slot| {
                        if db
                            .storage
                            .get_slot_storage_entry((slot as Slot) + slot1)
                            .is_some()
                        {
                            error!("slot exists: {}", (slot as Slot) + slot1);
                            1
                        } else {
                            error!("slot is empty: {}", (slot as Slot) + slot1);
                            0
                        }
                    })
                    .sum::<usize>();
                let mut expected_slots = max_ancient_slots.min(slots);
                if max_ancient_slots == 0 {
                    expected_slots = 1;
                }
                assert_eq!(
                    active_slots, expected_slots,
                    "slots: {slots}, max_ancient_slots: {max_ancient_slots}"
                );
                assert_eq!(
                    expected_slots,
                    db.storage.all_slots().len(),
                    "slots: {slots}, max_ancient_slots: {max_ancient_slots}"
                );

                let stores = (0..slots)
                    .filter_map(|slot| db.storage.get_slot_storage_entry((slot as Slot) + slot1))
                    .collect::<Vec<_>>();
                let results = stores
                    .iter()
                    .map(|store| db.get_unique_accounts_from_storage(store))
                    .collect::<Vec<_>>();

                compare_all_vec_accounts(&results, &original_results);
            }
        }
    }

    fn vec_unique_to_accounts(one: &[GetUniqueAccountsResult]) -> Vec<(Pubkey, AccountSharedData)> {
        one.iter()
            .map(|result| {
                result
                    .stored_accounts
                    .iter()
                    .map(|result| (*result.pubkey(), result.to_account_shared_data()))
            })
            .flatten()
            .collect()
    }

    pub(crate) fn compare_all_vec_accounts(
        one: &[GetUniqueAccountsResult],
        two: &[GetUniqueAccountsResult],
    ) {
        compare_all_accounts(&vec_unique_to_accounts(one), &vec_unique_to_accounts(two));
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
        db.combine_ancient_slots_new(
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

        db.combine_ancient_slots_new(
            (next_slot..=max_slot_inclusive).collect(),
            CAN_RANDOMLY_SHRINK_FALSE,
        );

        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );

        /*
        // now, shrink the second ancient append vec into the first one
        let mut current_ancient = CurrentAncientAppendVec::new(
            ancient_slot,
            db.get_storage_for_slot(ancient_slot).unwrap(),
        );
        let mut dropped_roots = Vec::default();
        db.combine_one_store_into_ancient(
            next_slot,
            &db.get_storage_for_slot(next_slot).unwrap(),
            &mut current_ancient,
            &mut AncientSlotPubkeys::default(),
            &mut dropped_roots,
        );
        assert!(db.storage.is_empty_entry(next_slot));
        // this removes the storages entry completely from the hashmap for 'next_slot'.
        // Otherwise, we have a zero length vec in that hashmap
        db.handle_dropped_roots_for_ancient(dropped_roots);
        assert!(db.storage.get_slot_storage_entry(next_slot).is_none());

        // include all the slots we put into the ancient append vec - they should contain nothing
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(max_slot_inclusive + 1)),
        );
        // look at just the ancient append vec
        compare_all_accounts(
            &initial_accounts,
            &get_all_accounts(&db, ancient_slot..(ancient_slot + 1)),
        );
        // make sure there is only 1 ancient append vec at the ancient slot
        assert!(db.storage.get_slot_storage_entry(ancient_slot).is_some());
        assert!(is_ancient(
            &db.storage
                .get_slot_storage_entry(ancient_slot)
                .unwrap()
                .accounts
        ));
        ((ancient_slot + 1)..=max_slot_inclusive)
            .for_each(|slot| assert!(db.storage.get_slot_storage_entry(slot).is_none()));
            */
    }
}
