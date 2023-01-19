use {
    crate::{
        accounts_index_storage::{AccountsIndexStorage, Startup},
        ancestors::Ancestors,
        bucket_map_holder::{Age, BucketMapHolder},
        contains::Contains,
        in_mem_accounts_index::InMemAccountsIndex,
        inline_spl_token::{self, GenericTokenAccount},
        inline_spl_token_2022,
        pubkey_bins::PubkeyBinCalculator24,
        rent_paying_accounts_by_partition::RentPayingAccountsByPartition,
        rolling_bit_field::RollingBitField,
        secondary_index::*,
    },
    log::*,
    once_cell::sync::OnceCell,
    ouroboros::self_referencing,
    rand::{thread_rng, Rng},
    rayon::{
        iter::{IntoParallelIterator, ParallelIterator},
        ThreadPool,
    },
    solana_measure::measure::Measure,
    solana_sdk::{
        account::ReadableAccount,
        clock::{BankId, Slot},
        pubkey::Pubkey,
    },
    std::{
        collections::{btree_map::BTreeMap, HashSet},
        fmt::Debug,
        ops::{
            Bound,
            Bound::{Excluded, Included, Unbounded},
            Range, RangeBounds,
        },
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
            Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard,
        },
    },
    thiserror::Error,
};

pub const ITER_BATCH_SIZE: usize = 1000;
pub const BINS_DEFAULT: usize = 8192;
pub const BINS_FOR_TESTING: usize = 2; // we want > 1, but each bin is a few disk files with a disk based index, so fewer is better
pub const BINS_FOR_BENCHMARKS: usize = 8192;
pub const FLUSH_THREADS_TESTING: usize = 1;
pub const ACCOUNTS_INDEX_CONFIG_FOR_TESTING: AccountsIndexConfig = AccountsIndexConfig {
    bins: Some(BINS_FOR_TESTING),
    flush_threads: Some(FLUSH_THREADS_TESTING),
    drives: None,
    index_limit_mb: IndexLimitMb::Unspecified,
    ages_to_stay_in_cache: None,
    scan_results_limit_bytes: None,
    started_from_validator: false,
};
pub const ACCOUNTS_INDEX_CONFIG_FOR_BENCHMARKS: AccountsIndexConfig = AccountsIndexConfig {
    bins: Some(BINS_FOR_BENCHMARKS),
    flush_threads: Some(FLUSH_THREADS_TESTING),
    drives: None,
    index_limit_mb: IndexLimitMb::Unspecified,
    ages_to_stay_in_cache: None,
    scan_results_limit_bytes: None,
    started_from_validator: false,
};
pub type ScanResult<T> = Result<T, ScanError>;
pub type SlotList<T> = Vec<(Slot, T)>;
pub type SlotSlice<'s, T> = &'s [(Slot, T)];
pub type RefCount = u64;
pub type AccountMap<V> = Arc<InMemAccountsIndex<V>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// how accounts index 'upsert' should handle reclaims
pub enum UpsertReclaim {
    /// previous entry for this slot in the index is expected to be cached, so irrelevant to reclaims
    PreviousSlotEntryWasCached,
    /// previous entry for this slot in the index may need to be reclaimed, so return it.
    /// reclaims is the only output of upsert, requiring a synchronous execution
    PopulateReclaims,
    /// overwrite existing data in the same slot and do not return in 'reclaims'
    IgnoreReclaims,
}

#[derive(Debug, Default)]
pub struct ScanConfig {
    /// checked by the scan. When true, abort scan.
    pub abort: Option<Arc<AtomicBool>>,

    /// true to allow return of all matching items and allow them to be unsorted.
    /// This is more efficient.
    pub collect_all_unsorted: bool,
}

impl ScanConfig {
    pub fn new(collect_all_unsorted: bool) -> Self {
        Self {
            collect_all_unsorted,
            ..ScanConfig::default()
        }
    }

    /// mark the scan as aborted
    pub fn abort(&self) {
        if let Some(abort) = self.abort.as_ref() {
            abort.store(true, Ordering::Relaxed)
        }
    }

    /// use existing 'abort' if available, otherwise allocate one
    pub fn recreate_with_abort(&self) -> Self {
        ScanConfig {
            abort: Some(self.abort.as_ref().map(Arc::clone).unwrap_or_default()),
            collect_all_unsorted: self.collect_all_unsorted,
        }
    }

    /// true if scan should abort
    pub fn is_aborted(&self) -> bool {
        if let Some(abort) = self.abort.as_ref() {
            abort.load(Ordering::Relaxed)
        } else {
            false
        }
    }
}

pub(crate) type AccountMapEntry<T> = Arc<AccountMapEntryInner<T>>;

pub trait IsCached {
    fn is_cached(&self) -> bool;
}

pub trait IndexValue:
    'static + IsCached + Clone + Debug + PartialEq + ZeroLamport + Copy + Default + Sync + Send
{
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ScanError {
    #[error("Node detected it replayed bad version of slot {slot:?} with id {bank_id:?}, thus the scan on said slot was aborted")]
    SlotRemoved { slot: Slot, bank_id: BankId },
    #[error("scan aborted: {0}")]
    Aborted(String),
}

enum ScanTypes<R: RangeBounds<Pubkey>> {
    Unindexed(Option<R>),
    Indexed(IndexKey),
}

#[derive(Debug, Clone, Copy)]
pub enum IndexKey {
    ProgramId(Pubkey),
    SplTokenMint(Pubkey),
    SplTokenOwner(Pubkey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AccountIndex {
    ProgramId,
    SplTokenMint,
    SplTokenOwner,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AccountSecondaryIndexesIncludeExclude {
    pub exclude: bool,
    pub keys: HashSet<Pubkey>,
}

/// specification of how much memory in-mem portion of account index can use
#[derive(Debug, Clone)]
pub enum IndexLimitMb {
    /// nothing explicit specified, so default
    Unspecified,
    /// limit was specified, use disk index for rest
    Limit(usize),
    /// in-mem-only was specified, no disk index
    InMemOnly,
}

impl Default for IndexLimitMb {
    fn default() -> Self {
        Self::Unspecified
    }
}

#[derive(Debug, Default, Clone)]
pub struct AccountsIndexConfig {
    pub bins: Option<usize>,
    pub flush_threads: Option<usize>,
    pub drives: Option<Vec<PathBuf>>,
    pub index_limit_mb: IndexLimitMb,
    pub ages_to_stay_in_cache: Option<Age>,
    pub scan_results_limit_bytes: Option<usize>,
    /// true if the accounts index is being created as a result of being started as a validator (as opposed to test, etc.)
    pub started_from_validator: bool,
}

#[derive(Debug, Default, Clone)]
pub struct AccountSecondaryIndexes {
    pub keys: Option<AccountSecondaryIndexesIncludeExclude>,
    pub indexes: HashSet<AccountIndex>,
}

impl AccountSecondaryIndexes {
    pub fn is_empty(&self) -> bool {
        self.indexes.is_empty()
    }
    pub fn contains(&self, index: &AccountIndex) -> bool {
        self.indexes.contains(index)
    }
    pub fn include_key(&self, key: &Pubkey) -> bool {
        match &self.keys {
            Some(options) => options.exclude ^ options.keys.contains(key),
            None => true, // include all keys
        }
    }
}

#[derive(Debug, Default)]
/// data per entry in in-mem accounts index
/// used to keep track of consistency with disk index
pub struct AccountMapEntryMeta {
    /// true if entry in in-mem idx has changes and needs to be written to disk
    pub dirty: AtomicBool,
    /// 'age' at which this entry should be purged from the cache (implements lru)
    pub age: AtomicU8,
}

impl AccountMapEntryMeta {
    pub fn new_dirty<T: IndexValue>(storage: &Arc<BucketMapHolder<T>>, is_cached: bool) -> Self {
        AccountMapEntryMeta {
            dirty: AtomicBool::new(true),
            age: AtomicU8::new(storage.future_age_to_flush(is_cached)),
        }
    }
    pub fn new_clean<T: IndexValue>(storage: &Arc<BucketMapHolder<T>>) -> Self {
        AccountMapEntryMeta {
            dirty: AtomicBool::new(false),
            age: AtomicU8::new(storage.future_age_to_flush(false)),
        }
    }
}

#[derive(Debug, Default)]
/// one entry in the in-mem accounts index
/// Represents the value for an account key in the in-memory accounts index
pub struct AccountMapEntryInner<T> {
    /// number of alive slots that contain >= 1 instances of account data for this pubkey
    /// where alive represents a slot that has not yet been removed by clean via AccountsDB::clean_stored_dead_slots() for containing no up to date account information
    ref_count: AtomicU64,
    /// list of slots in which this pubkey was updated
    /// Note that 'clean' removes outdated entries (ie. older roots) from this slot_list
    /// purge_slot() also removes non-rooted slots from this list
    pub slot_list: RwLock<SlotList<T>>,
    /// synchronization metadata for in-memory state since last flush to disk accounts index
    pub meta: AccountMapEntryMeta,
}

impl<T: IndexValue> AccountMapEntryInner<T> {
    pub fn new(slot_list: SlotList<T>, ref_count: RefCount, meta: AccountMapEntryMeta) -> Self {
        Self {
            slot_list: RwLock::new(slot_list),
            ref_count: AtomicU64::new(ref_count),
            meta,
        }
    }
    pub fn ref_count(&self) -> RefCount {
        self.ref_count.load(Ordering::Acquire)
    }

    pub fn addref(&self) {
        self.ref_count.fetch_add(1, Ordering::Release);
        self.set_dirty(true);
    }

    /// decrement the ref count
    /// return true if the old refcount was already 0. This indicates an under refcounting error in the system.
    pub fn unref(&self) -> bool {
        let previous = self.ref_count.fetch_sub(1, Ordering::Release);
        self.set_dirty(true);
        if previous == 0 {
            inc_new_counter_info!("accounts_index-deref_from_0", 1);
        }
        previous == 0
    }

    pub fn dirty(&self) -> bool {
        self.meta.dirty.load(Ordering::Acquire)
    }

    pub fn set_dirty(&self, value: bool) {
        self.meta.dirty.store(value, Ordering::Release)
    }

    /// set dirty to false, return true if was dirty
    pub fn clear_dirty(&self) -> bool {
        self.meta
            .dirty
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    pub fn age(&self) -> Age {
        self.meta.age.load(Ordering::Acquire)
    }

    pub fn set_age(&self, value: Age) {
        self.meta.age.store(value, Ordering::Release)
    }

    /// set age to 'next_age' if 'self.age' is 'expected_age'
    pub fn try_exchange_age(&self, next_age: Age, expected_age: Age) {
        let _ = self.meta.age.compare_exchange(
            expected_age,
            next_age,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }
}

pub enum AccountIndexGetResult<T: IndexValue> {
    /// (index entry, index in slot list)
    Found(ReadAccountMapEntry<T>, usize),
    NotFound,
}

#[self_referencing]
pub struct ReadAccountMapEntry<T: IndexValue> {
    owned_entry: AccountMapEntry<T>,
    #[borrows(owned_entry)]
    #[covariant]
    slot_list_guard: RwLockReadGuard<'this, SlotList<T>>,
}

impl<T: IndexValue> Debug for ReadAccountMapEntry<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.borrow_owned_entry())
    }
}

impl<T: IndexValue> ReadAccountMapEntry<T> {
    pub fn from_account_map_entry(account_map_entry: AccountMapEntry<T>) -> Self {
        ReadAccountMapEntryBuilder {
            owned_entry: account_map_entry,
            slot_list_guard_builder: |lock| lock.slot_list.read().unwrap(),
        }
        .build()
    }

    pub fn slot_list(&self) -> &SlotList<T> {
        self.borrow_slot_list_guard()
    }

    pub fn ref_count(&self) -> RefCount {
        self.borrow_owned_entry().ref_count()
    }

    pub fn addref(&self) {
        self.borrow_owned_entry().addref();
    }
}

/// can be used to pre-allocate structures for insertion into accounts index outside of lock
pub enum PreAllocatedAccountMapEntry<T: IndexValue> {
    Entry(AccountMapEntry<T>),
    Raw((Slot, T)),
}

impl<T: IndexValue> ZeroLamport for PreAllocatedAccountMapEntry<T> {
    fn is_zero_lamport(&self) -> bool {
        match self {
            PreAllocatedAccountMapEntry::Entry(entry) => {
                entry.slot_list.read().unwrap()[0].1.is_zero_lamport()
            }
            PreAllocatedAccountMapEntry::Raw(raw) => raw.1.is_zero_lamport(),
        }
    }
}

impl<T: IndexValue> From<PreAllocatedAccountMapEntry<T>> for (Slot, T) {
    fn from(source: PreAllocatedAccountMapEntry<T>) -> (Slot, T) {
        match source {
            PreAllocatedAccountMapEntry::Entry(entry) => entry.slot_list.read().unwrap()[0],
            PreAllocatedAccountMapEntry::Raw(raw) => raw,
        }
    }
}

impl<T: IndexValue> PreAllocatedAccountMapEntry<T> {
    /// create an entry that is equivalent to this process:
    /// 1. new empty (refcount=0, slot_list={})
    /// 2. update(slot, account_info)
    /// This code is called when the first entry [ie. (slot,account_info)] for a pubkey is inserted into the index.
    pub fn new(
        slot: Slot,
        account_info: T,
        storage: &Arc<BucketMapHolder<T>>,
        store_raw: bool,
    ) -> PreAllocatedAccountMapEntry<T> {
        if store_raw {
            Self::Raw((slot, account_info))
        } else {
            Self::Entry(Self::allocate(slot, account_info, storage))
        }
    }

    fn allocate(
        slot: Slot,
        account_info: T,
        storage: &Arc<BucketMapHolder<T>>,
    ) -> AccountMapEntry<T> {
        let is_cached = account_info.is_cached();
        let ref_count = u64::from(!is_cached);
        let meta = AccountMapEntryMeta::new_dirty(storage, is_cached);
        Arc::new(AccountMapEntryInner::new(
            vec![(slot, account_info)],
            ref_count,
            meta,
        ))
    }

    pub fn into_account_map_entry(self, storage: &Arc<BucketMapHolder<T>>) -> AccountMapEntry<T> {
        match self {
            Self::Entry(entry) => entry,
            Self::Raw((slot, account_info)) => Self::allocate(slot, account_info, storage),
        }
    }
}

#[derive(Debug)]
pub struct RootsTracker {
    /// Current roots where appendvecs or write cache has account data.
    /// Constructed during load from snapshots.
    /// Updated every time we add a new root or clean/shrink an append vec into irrelevancy.
    /// Range is approximately the last N slots where N is # slots per epoch.
    pub(crate) alive_roots: RollingBitField,
    /// Set of roots that are roots now or were roots at one point in time.
    /// Range is approximately the last N slots where N is # slots per epoch.
    /// A root could remain here if all entries in the append vec at that root are cleaned/shrunk and there are no
    /// more entries for that slot. 'alive_roots' will no longer contain such roots.
    /// This is a superset of 'alive_roots'
    pub(crate) historical_roots: RollingBitField,
    uncleaned_roots: HashSet<Slot>,
    previous_uncleaned_roots: HashSet<Slot>,
}

impl Default for RootsTracker {
    fn default() -> Self {
        // we expect to keep a rolling set of 400k slots around at a time
        // 4M gives us plenty of extra(?!) room to handle a width 10x what we should need.
        // cost is 4M bits of memory, which is .5MB
        RootsTracker::new(4194304)
    }
}

impl RootsTracker {
    pub fn new(max_width: u64) -> Self {
        Self {
            alive_roots: RollingBitField::new(max_width),
            historical_roots: RollingBitField::new(max_width),
            uncleaned_roots: HashSet::new(),
            previous_uncleaned_roots: HashSet::new(),
        }
    }

    pub fn min_alive_root(&self) -> Option<Slot> {
        self.alive_roots.min()
    }
}

#[derive(Debug, Default)]
pub struct AccountsIndexRootsStats {
    pub roots_len: Option<usize>,
    pub uncleaned_roots_len: Option<usize>,
    pub previous_uncleaned_roots_len: Option<usize>,
    pub roots_range: Option<u64>,
    pub historical_roots_len: Option<usize>,
    pub rooted_cleaned_count: usize,
    pub unrooted_cleaned_count: usize,
    pub clean_unref_from_storage_us: u64,
    pub clean_dead_slot_us: u64,
}

pub struct AccountsIndexIterator<'a, T: IndexValue> {
    account_maps: &'a LockMapTypeSlice<T>,
    bin_calculator: &'a PubkeyBinCalculator24,
    start_bound: Bound<Pubkey>,
    end_bound: Bound<Pubkey>,
    is_finished: bool,
    collect_all_unsorted: bool,
}

impl<'a, T: IndexValue> AccountsIndexIterator<'a, T> {
    fn range<R>(
        map: &AccountMaps<T>,
        range: R,
        collect_all_unsorted: bool,
    ) -> Vec<(Pubkey, AccountMapEntry<T>)>
    where
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        let mut result = map.items(&range);
        if !collect_all_unsorted {
            result.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        }
        result
    }

    fn clone_bound(bound: Bound<&Pubkey>) -> Bound<Pubkey> {
        match bound {
            Unbounded => Unbounded,
            Included(k) => Included(*k),
            Excluded(k) => Excluded(*k),
        }
    }

    fn bin_from_bound(&self, bound: &Bound<Pubkey>, unbounded_bin: usize) -> usize {
        match bound {
            Bound::Included(bound) | Bound::Excluded(bound) => {
                self.bin_calculator.bin_from_pubkey(bound)
            }
            Bound::Unbounded => unbounded_bin,
        }
    }

    fn start_bin(&self) -> usize {
        // start in bin where 'start_bound' would exist
        self.bin_from_bound(&self.start_bound, 0)
    }

    fn end_bin_inclusive(&self) -> usize {
        // end in bin where 'end_bound' would exist
        self.bin_from_bound(&self.end_bound, usize::MAX)
    }

    fn bin_start_and_range(&self) -> (usize, usize) {
        let start_bin = self.start_bin();
        // calculate the max range of bins to look in
        let end_bin_inclusive = self.end_bin_inclusive();
        let bin_range = if start_bin > end_bin_inclusive {
            0 // empty range
        } else if end_bin_inclusive == usize::MAX {
            usize::MAX
        } else {
            // the range is end_inclusive + 1 - start
            // end_inclusive could be usize::MAX already if no bound was specified
            end_bin_inclusive.saturating_add(1) - start_bin
        };
        (start_bin, bin_range)
    }

    pub fn new<R>(
        index: &'a AccountsIndex<T>,
        range: Option<&R>,
        collect_all_unsorted: bool,
    ) -> Self
    where
        R: RangeBounds<Pubkey>,
    {
        Self {
            start_bound: range
                .as_ref()
                .map(|r| Self::clone_bound(r.start_bound()))
                .unwrap_or(Unbounded),
            end_bound: range
                .as_ref()
                .map(|r| Self::clone_bound(r.end_bound()))
                .unwrap_or(Unbounded),
            account_maps: &index.account_maps,
            is_finished: false,
            bin_calculator: &index.bin_calculator,
            collect_all_unsorted,
        }
    }

    pub fn hold_range_in_memory<R>(&self, range: &R, start_holding: bool, thread_pool: &ThreadPool)
    where
        R: RangeBounds<Pubkey> + Debug + Sync,
    {
        // forward this hold request ONLY to the bins which contain keys in the specified range
        let (start_bin, bin_range) = self.bin_start_and_range();
        // the idea is this range shouldn't be more than a few buckets, but the process of loading from disk buckets is very slow
        // so, parallelize the bucket loads
        thread_pool.install(|| {
            (0..bin_range).into_par_iter().for_each(|idx| {
                let map = &self.account_maps[idx + start_bin];
                map.hold_range_in_memory(range, start_holding);
            });
        });
    }
}

impl<'a, T: IndexValue> Iterator for AccountsIndexIterator<'a, T> {
    type Item = Vec<(Pubkey, AccountMapEntry<T>)>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_finished {
            return None;
        }
        let (start_bin, bin_range) = self.bin_start_and_range();
        let mut chunk = Vec::with_capacity(ITER_BATCH_SIZE);
        'outer: for i in self.account_maps.iter().skip(start_bin).take(bin_range) {
            for (pubkey, account_map_entry) in Self::range(
                &i,
                (self.start_bound, self.end_bound),
                self.collect_all_unsorted,
            ) {
                if chunk.len() >= ITER_BATCH_SIZE && !self.collect_all_unsorted {
                    break 'outer;
                }
                let item = (pubkey, account_map_entry);
                chunk.push(item);
            }
        }

        if chunk.is_empty() {
            self.is_finished = true;
            return None;
        } else if self.collect_all_unsorted {
            self.is_finished = true;
        }

        self.start_bound = Excluded(chunk.last().unwrap().0);
        Some(chunk)
    }
}

pub trait ZeroLamport {
    fn is_zero_lamport(&self) -> bool;
}

type MapType<T> = AccountMap<T>;
type LockMapType<T> = Vec<MapType<T>>;
type LockMapTypeSlice<T> = [MapType<T>];
type AccountMaps<'a, T> = &'a MapType<T>;

#[derive(Debug, Default)]
pub struct ScanSlotTracker {
    is_removed: bool,
}

impl ScanSlotTracker {
    pub fn is_removed(&self) -> bool {
        self.is_removed
    }

    pub fn mark_removed(&mut self) {
        self.is_removed = true;
    }
}

#[derive(Copy, Clone)]
pub enum AccountsIndexScanResult {
    /// if the entry is not in the in-memory index, do not add it, make no modifications to it
    None,
    /// keep the entry in the in-memory index
    KeepInMemory,
    /// reduce refcount by 1
    Unref,
}

#[derive(Debug)]
pub struct AccountsIndex<T: IndexValue> {
    pub account_maps: LockMapType<T>,
    pub bin_calculator: PubkeyBinCalculator24,
    program_id_index: SecondaryIndex<DashMapSecondaryIndexEntry>,
    spl_token_mint_index: SecondaryIndex<DashMapSecondaryIndexEntry>,
    spl_token_owner_index: SecondaryIndex<RwLockSecondaryIndexEntry>,
    pub(crate) roots_tracker: RwLock<RootsTracker>,
    ongoing_scan_roots: RwLock<BTreeMap<Slot, u64>>,
    // Each scan has some latest slot `S` that is the tip of the fork the scan
    // is iterating over. The unique id of that slot `S` is recorded here (note we don't use
    // `S` as the id because there can be more than one version of a slot `S`). If a fork
    // is abandoned, all of the slots on that fork up to `S` will be removed via
    // `AccountsDb::remove_unrooted_slots()`. When the scan finishes, it'll realize that the
    // results of the scan may have been corrupted by `remove_unrooted_slots` and abort its results.
    //
    // `removed_bank_ids` tracks all the slot ids that were removed via `remove_unrooted_slots()` so any attempted scans
    // on any of these slots fails. This is safe to purge once the associated Bank is dropped and
    // scanning the fork with that Bank at the tip is no longer possible.
    pub removed_bank_ids: Mutex<HashSet<BankId>>,

    storage: AccountsIndexStorage<T>,

    /// when a scan's accumulated data exceeds this limit, abort the scan
    pub scan_results_limit_bytes: Option<usize>,

    /// # roots added since last check
    pub roots_added: AtomicUsize,
    /// # roots removed since last check
    pub roots_removed: AtomicUsize,
    /// # scans active currently
    pub active_scans: AtomicUsize,
    /// # of slots between latest max and latest scan
    pub max_distance_to_min_scan_slot: AtomicU64,

    /// populated at generate_index time - accounts that could possibly be rent paying
    pub rent_paying_accounts_by_partition: OnceCell<RentPayingAccountsByPartition>,
}

impl<T: IndexValue> AccountsIndex<T> {
    pub fn default_for_tests() -> Self {
        Self::new(Some(ACCOUNTS_INDEX_CONFIG_FOR_TESTING), &Arc::default())
    }

    pub fn new(config: Option<AccountsIndexConfig>, exit: &Arc<AtomicBool>) -> Self {
        let scan_results_limit_bytes = config
            .as_ref()
            .and_then(|config| config.scan_results_limit_bytes);
        let (account_maps, bin_calculator, storage) = Self::allocate_accounts_index(config, exit);
        Self {
            account_maps,
            bin_calculator,
            program_id_index: SecondaryIndex::<DashMapSecondaryIndexEntry>::new(
                "program_id_index_stats",
            ),
            spl_token_mint_index: SecondaryIndex::<DashMapSecondaryIndexEntry>::new(
                "spl_token_mint_index_stats",
            ),
            spl_token_owner_index: SecondaryIndex::<RwLockSecondaryIndexEntry>::new(
                "spl_token_owner_index_stats",
            ),
            roots_tracker: RwLock::<RootsTracker>::default(),
            ongoing_scan_roots: RwLock::<BTreeMap<Slot, u64>>::default(),
            removed_bank_ids: Mutex::<HashSet<BankId>>::default(),
            storage,
            scan_results_limit_bytes,
            roots_added: AtomicUsize::default(),
            roots_removed: AtomicUsize::default(),
            active_scans: AtomicUsize::default(),
            max_distance_to_min_scan_slot: AtomicU64::default(),
            rent_paying_accounts_by_partition: OnceCell::default(),
        }
    }

    fn allocate_accounts_index(
        config: Option<AccountsIndexConfig>,
        exit: &Arc<AtomicBool>,
    ) -> (
        LockMapType<T>,
        PubkeyBinCalculator24,
        AccountsIndexStorage<T>,
    ) {
        let bins = config
            .as_ref()
            .and_then(|config| config.bins)
            .unwrap_or(BINS_DEFAULT);
        // create bin_calculator early to verify # bins is reasonable
        let bin_calculator = PubkeyBinCalculator24::new(bins);
        let storage = AccountsIndexStorage::new(bins, &config, exit);
        let account_maps = (0..bins)
            .map(|bin| Arc::clone(&storage.in_mem[bin]))
            .collect::<Vec<_>>();
        (account_maps, bin_calculator, storage)
    }

    fn iter<R>(&self, range: Option<&R>, collect_all_unsorted: bool) -> AccountsIndexIterator<T>
    where
        R: RangeBounds<Pubkey>,
    {
        AccountsIndexIterator::new(self, range, collect_all_unsorted)
    }

    /// is the accounts index using disk as a backing store
    pub fn is_disk_index_enabled(&self) -> bool {
        self.storage.storage.is_disk_index_enabled()
    }

    fn min_ongoing_scan_root_from_btree(ongoing_scan_roots: &BTreeMap<Slot, u64>) -> Option<Slot> {
        ongoing_scan_roots.keys().next().cloned()
    }

    fn do_checked_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        scan_bank_id: BankId,
        func: F,
        scan_type: ScanTypes<R>,
        config: &ScanConfig,
    ) -> Result<(), ScanError>
    where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        {
            let locked_removed_bank_ids = self.removed_bank_ids.lock().unwrap();
            if locked_removed_bank_ids.contains(&scan_bank_id) {
                return Err(ScanError::SlotRemoved {
                    slot: ancestors.max_slot(),
                    bank_id: scan_bank_id,
                });
            }
        }

        self.active_scans.fetch_add(1, Ordering::Relaxed);
        let max_root = {
            let mut w_ongoing_scan_roots = self
                // This lock is also grabbed by clean_accounts(), so clean
                // has at most cleaned up to the current `max_root` (since
                // clean only happens *after* BankForks::set_root() which sets
                // the `max_root`)
                .ongoing_scan_roots
                .write()
                .unwrap();
            // `max_root()` grabs a lock while
            // the `ongoing_scan_roots` lock is held,
            // make sure inverse doesn't happen to avoid
            // deadlock
            let max_root_inclusive = self.max_root_inclusive();
            if let Some(min_ongoing_scan_root) =
                Self::min_ongoing_scan_root_from_btree(&w_ongoing_scan_roots)
            {
                if min_ongoing_scan_root < max_root_inclusive {
                    let current = max_root_inclusive - min_ongoing_scan_root;
                    self.max_distance_to_min_scan_slot
                        .fetch_max(current, Ordering::Relaxed);
                }
            }
            *w_ongoing_scan_roots.entry(max_root_inclusive).or_default() += 1;

            max_root_inclusive
        };

        // First we show that for any bank `B` that is a descendant of
        // the current `max_root`, it must be true that and `B.ancestors.contains(max_root)`,
        // regardless of the pattern of `squash()` behavior, where `ancestors` is the set
        // of ancestors that is tracked in each bank.
        //
        // Proof: At startup, if starting from a snapshot, generate_index() adds all banks
        // in the snapshot to the index via `add_root()` and so `max_root` will be the
        // greatest of these. Thus, so the claim holds at startup since there are no
        // descendants of `max_root`.
        //
        // Now we proceed by induction on each `BankForks::set_root()`.
        // Assume the claim holds when the `max_root` is `R`. Call the set of
        // descendants of `R` present in BankForks `R_descendants`.
        //
        // Then for any banks `B` in `R_descendants`, it must be that `B.ancestors.contains(S)`,
        // where `S` is any ancestor of `B` such that `S >= R`.
        //
        // For example:
        //          `R` -> `A` -> `C` -> `B`
        // Then `B.ancestors == {R, A, C}`
        //
        // Next we call `BankForks::set_root()` at some descendant of `R`, `R_new`,
        // where `R_new > R`.
        //
        // When we squash `R_new`, `max_root` in the AccountsIndex here is now set to `R_new`,
        // and all nondescendants of `R_new` are pruned.
        //
        // Now consider any outstanding references to banks in the system that are descended from
        // `max_root == R_new`. Take any one of these references and call it `B`. Because `B` is
        // a descendant of `R_new`, this means `B` was also a descendant of `R`. Thus `B`
        // must be a member of `R_descendants` because `B` was constructed and added to
        // BankForks before the `set_root`.
        //
        // This means by the guarantees of `R_descendants` described above, because
        // `R_new` is an ancestor of `B`, and `R < R_new < B`, then `B.ancestors.contains(R_new)`.
        //
        // Now until the next `set_root`, any new banks constructed from `new_from_parent` will
        // also have `max_root == R_new` in their ancestor set, so the claim holds for those descendants
        // as well. Once the next `set_root` happens, we once again update `max_root` and the same
        // inductive argument can be applied again to show the claim holds.

        // Check that the `max_root` is present in `ancestors`. From the proof above, if
        // `max_root` is not present in `ancestors`, this means the bank `B` with the
        // given `ancestors` is not descended from `max_root, which means
        // either:
        // 1) `B` is on a different fork or
        // 2) `B` is an ancestor of `max_root`.
        // In both cases we can ignore the given ancestors and instead just rely on the roots
        // present as `max_root` indicates the roots present in the index are more up to date
        // than the ancestors given.
        let empty = Ancestors::default();
        let ancestors = if ancestors.contains_key(&max_root) {
            ancestors
        } else {
            /*
            This takes of edge cases like:

            Diagram 1:

                        slot 0
                          |
                        slot 1
                      /        \
                 slot 2         |
                    |       slot 3 (max root)
            slot 4 (scan)

            By the time the scan on slot 4 is called, slot 2 may already have been
            cleaned by a clean on slot 3, but slot 4 may not have been cleaned.
            The state in slot 2 would have been purged and is not saved in any roots.
            In this case, a scan on slot 4 wouldn't accurately reflect the state when bank 4
            was frozen. In cases like this, we default to a scan on the latest roots by
            removing all `ancestors`.
            */
            &empty
        };

        /*
        Now there are two cases, either `ancestors` is empty or nonempty:

        1) If ancestors is empty, then this is the same as a scan on a rooted bank,
        and `ongoing_scan_roots` provides protection against cleanup of roots necessary
        for the scan, and  passing `Some(max_root)` to `do_scan_accounts()` ensures newer
        roots don't appear in the scan.

        2) If ancestors is non-empty, then from the `ancestors_contains(&max_root)` above, we know
        that the fork structure must look something like:

        Diagram 2:

                Build fork structure:
                        slot 0
                          |
                    slot 1 (max_root)
                    /            \
             slot 2              |
                |            slot 3 (potential newer max root)
              slot 4
                |
             slot 5 (scan)

        Consider both types of ancestors, ancestor <= `max_root` and
        ancestor > `max_root`, where `max_root == 1` as illustrated above.

        a) The set of `ancestors <= max_root` are all rooted, which means their state
        is protected by the same guarantees as 1).

        b) As for the `ancestors > max_root`, those banks have at least one reference discoverable
        through the chain of `Bank::BankRc::parent` starting from the calling bank. For instance
        bank 5's parent reference keeps bank 4 alive, which will prevent the `Bank::drop()` from
        running and cleaning up bank 4. Furthermore, no cleans can happen past the saved max_root == 1,
        so a potential newer max root at 3 will not clean up any of the ancestors > 1, so slot 4
        will not be cleaned in the middle of the scan either. (NOTE similar reasoning is employed for
        assert!() justification in AccountsDb::retry_to_get_account_accessor)
        */
        match scan_type {
            ScanTypes::Unindexed(range) => {
                // Pass "" not to log metrics, so RPC doesn't get spammy
                self.do_scan_accounts(metric_name, ancestors, func, range, Some(max_root), config);
            }
            ScanTypes::Indexed(IndexKey::ProgramId(program_id)) => {
                self.do_scan_secondary_index(
                    ancestors,
                    func,
                    &self.program_id_index,
                    &program_id,
                    Some(max_root),
                    config,
                );
            }
            ScanTypes::Indexed(IndexKey::SplTokenMint(mint_key)) => {
                self.do_scan_secondary_index(
                    ancestors,
                    func,
                    &self.spl_token_mint_index,
                    &mint_key,
                    Some(max_root),
                    config,
                );
            }
            ScanTypes::Indexed(IndexKey::SplTokenOwner(owner_key)) => {
                self.do_scan_secondary_index(
                    ancestors,
                    func,
                    &self.spl_token_owner_index,
                    &owner_key,
                    Some(max_root),
                    config,
                );
            }
        }

        {
            self.active_scans.fetch_sub(1, Ordering::Relaxed);
            let mut ongoing_scan_roots = self.ongoing_scan_roots.write().unwrap();
            let count = ongoing_scan_roots.get_mut(&max_root).unwrap();
            *count -= 1;
            if *count == 0 {
                ongoing_scan_roots.remove(&max_root);
            }
        }

        // If the fork with tip at bank `scan_bank_id` was removed during our scan, then the scan
        // may have been corrupted, so abort the results.
        let was_scan_corrupted = self
            .removed_bank_ids
            .lock()
            .unwrap()
            .contains(&scan_bank_id);

        if was_scan_corrupted {
            Err(ScanError::SlotRemoved {
                slot: ancestors.max_slot(),
                bank_id: scan_bank_id,
            })
        } else {
            Ok(())
        }
    }

    fn do_unchecked_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        func: F,
        range: Option<R>,
        config: &ScanConfig,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        self.do_scan_accounts(metric_name, ancestors, func, range, None, config);
    }

    // Scan accounts and return latest version of each account that is either:
    // 1) rooted or
    // 2) present in ancestors
    fn do_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        mut func: F,
        range: Option<R>,
        max_root: Option<Slot>,
        config: &ScanConfig,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        // TODO: expand to use mint index to find the `pubkey_list` below more efficiently
        // instead of scanning the entire range
        let mut total_elapsed_timer = Measure::start("total");
        let mut num_keys_iterated = 0;
        let mut latest_slot_elapsed = 0;
        let mut load_account_elapsed = 0;
        let mut read_lock_elapsed = 0;
        let mut iterator_elapsed = 0;
        let mut iterator_timer = Measure::start("iterator_elapsed");
        for pubkey_list in self.iter(range.as_ref(), config.collect_all_unsorted) {
            iterator_timer.stop();
            iterator_elapsed += iterator_timer.as_us();
            for (pubkey, list) in pubkey_list {
                num_keys_iterated += 1;
                let mut read_lock_timer = Measure::start("read_lock");
                let list_r = &list.slot_list.read().unwrap();
                read_lock_timer.stop();
                read_lock_elapsed += read_lock_timer.as_us();
                let mut latest_slot_timer = Measure::start("latest_slot");
                if let Some(index) = self.latest_slot(Some(ancestors), list_r, max_root) {
                    latest_slot_timer.stop();
                    latest_slot_elapsed += latest_slot_timer.as_us();
                    let mut load_account_timer = Measure::start("load_account");
                    func(&pubkey, (&list_r[index].1, list_r[index].0));
                    load_account_timer.stop();
                    load_account_elapsed += load_account_timer.as_us();
                }
                if config.is_aborted() {
                    return;
                }
            }
            iterator_timer = Measure::start("iterator_elapsed");
        }

        total_elapsed_timer.stop();
        if !metric_name.is_empty() {
            datapoint_info!(
                metric_name,
                ("total_elapsed", total_elapsed_timer.as_us(), i64),
                ("latest_slot_elapsed", latest_slot_elapsed, i64),
                ("read_lock_elapsed", read_lock_elapsed, i64),
                ("load_account_elapsed", load_account_elapsed, i64),
                ("iterator_elapsed", iterator_elapsed, i64),
                ("num_keys_iterated", num_keys_iterated, i64),
            )
        }
    }

    fn do_scan_secondary_index<
        F,
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        &self,
        ancestors: &Ancestors,
        mut func: F,
        index: &SecondaryIndex<SecondaryIndexEntryType>,
        index_key: &Pubkey,
        max_root: Option<Slot>,
        config: &ScanConfig,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        for pubkey in index.get(index_key) {
            // Maybe these reads from the AccountsIndex can be batched every time it
            // grabs the read lock as well...
            if let AccountIndexGetResult::Found(list_r, index) =
                self.get(&pubkey, Some(ancestors), max_root)
            {
                let entry = &list_r.slot_list()[index];
                func(&pubkey, (&entry.1, entry.0));
            }
            if config.is_aborted() {
                break;
            }
        }
    }

    pub fn get_account_read_entry(&self, pubkey: &Pubkey) -> Option<ReadAccountMapEntry<T>> {
        let lock = self.get_bin(pubkey);
        self.get_account_read_entry_with_lock(pubkey, &lock)
    }

    pub fn get_account_read_entry_with_lock(
        &self,
        pubkey: &Pubkey,
        lock: &AccountMaps<'_, T>,
    ) -> Option<ReadAccountMapEntry<T>> {
        lock.get(pubkey)
            .map(ReadAccountMapEntry::from_account_map_entry)
    }

    fn slot_list_mut<RT>(
        &self,
        pubkey: &Pubkey,
        user: impl for<'a> FnOnce(&mut RwLockWriteGuard<'a, SlotList<T>>) -> RT,
    ) -> Option<RT> {
        let read_lock = self.get_bin(pubkey);
        read_lock.slot_list_mut(pubkey, user)
    }

    /// Remove keys from the account index if the key's slot list is empty.
    /// Returns the keys that were removed from the index. These keys should not be accessed again in the current code path.
    #[must_use]
    pub fn handle_dead_keys(
        &self,
        dead_keys: &[&Pubkey],
        account_indexes: &AccountSecondaryIndexes,
    ) -> HashSet<Pubkey> {
        let mut pubkeys_removed_from_accounts_index = HashSet::default();
        if !dead_keys.is_empty() {
            for key in dead_keys.iter() {
                let w_index = self.get_bin(key);
                if w_index.remove_if_slot_list_empty(**key) {
                    pubkeys_removed_from_accounts_index.insert(**key);
                    // Note it's only safe to remove all the entries for this key
                    // because we have the lock for this key's entry in the AccountsIndex,
                    // so no other thread is also updating the index
                    self.purge_secondary_indexes_by_inner_key(key, account_indexes);
                }
            }
        }
        pubkeys_removed_from_accounts_index
    }

    /// call func with every pubkey and index visible from a given set of ancestors
    pub(crate) fn scan_accounts<F>(
        &self,
        ancestors: &Ancestors,
        scan_bank_id: BankId,
        func: F,
        config: &ScanConfig,
    ) -> Result<(), ScanError>
    where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        // Pass "" not to log metrics, so RPC doesn't get spammy
        self.do_checked_scan_accounts(
            "",
            ancestors,
            scan_bank_id,
            func,
            ScanTypes::Unindexed(None::<Range<Pubkey>>),
            config,
        )
    }

    pub(crate) fn unchecked_scan_accounts<F>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        func: F,
        config: &ScanConfig,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        self.do_unchecked_scan_accounts(
            metric_name,
            ancestors,
            func,
            None::<Range<Pubkey>>,
            config,
        );
    }

    /// call func with every pubkey and index visible from a given set of ancestors with range
    /// Only guaranteed to be safe when called from rent collection
    pub(crate) fn range_scan_accounts<F, R>(
        &self,
        metric_name: &'static str,
        ancestors: &Ancestors,
        range: R,
        config: &ScanConfig,
        func: F,
    ) where
        F: FnMut(&Pubkey, (&T, Slot)),
        R: RangeBounds<Pubkey> + std::fmt::Debug,
    {
        // Only the rent logic should be calling this, which doesn't need the safety checks
        self.do_unchecked_scan_accounts(metric_name, ancestors, func, Some(range), config);
    }

    /// call func with every pubkey and index visible from a given set of ancestors
    pub(crate) fn index_scan_accounts<F>(
        &self,
        ancestors: &Ancestors,
        scan_bank_id: BankId,
        index_key: IndexKey,
        func: F,
        config: &ScanConfig,
    ) -> Result<(), ScanError>
    where
        F: FnMut(&Pubkey, (&T, Slot)),
    {
        // Pass "" not to log metrics, so RPC doesn't get spammy
        self.do_checked_scan_accounts(
            "",
            ancestors,
            scan_bank_id,
            func,
            ScanTypes::<Range<Pubkey>>::Indexed(index_key),
            config,
        )
    }

    pub fn get_rooted_entries(
        &self,
        slice: SlotSlice<T>,
        max_inclusive: Option<Slot>,
    ) -> SlotList<T> {
        let max_inclusive = max_inclusive.unwrap_or(Slot::MAX);
        let lock = &self.roots_tracker.read().unwrap().alive_roots;
        slice
            .iter()
            .filter(|(slot, _)| *slot <= max_inclusive && lock.contains(slot))
            .cloned()
            .collect()
    }

    pub fn purge_exact<'a, C>(
        &'a self,
        pubkey: &Pubkey,
        slots_to_purge: &'a C,
        reclaims: &mut SlotList<T>,
    ) -> bool
    where
        C: Contains<'a, Slot>,
    {
        self.slot_list_mut(pubkey, |slot_list| {
            slot_list.retain(|(slot, item)| {
                let should_purge = slots_to_purge.contains(slot);
                if should_purge {
                    reclaims.push((*slot, *item));
                    false
                } else {
                    true
                }
            });
            slot_list.is_empty()
        })
        .unwrap_or(true)
    }

    pub fn min_ongoing_scan_root(&self) -> Option<Slot> {
        Self::min_ongoing_scan_root_from_btree(&self.ongoing_scan_roots.read().unwrap())
    }

    // Given a SlotSlice `L`, a list of ancestors and a maximum slot, find the latest element
    // in `L`, where the slot `S` is an ancestor or root, and if `S` is a root, then `S <= max_root`
    pub(crate) fn latest_slot(
        &self,
        ancestors: Option<&Ancestors>,
        slice: SlotSlice<T>,
        max_root_inclusive: Option<Slot>,
    ) -> Option<usize> {
        let mut current_max = 0;
        let mut rv = None;
        if let Some(ancestors) = ancestors {
            if !ancestors.is_empty() {
                for (i, (slot, _t)) in slice.iter().rev().enumerate() {
                    if (rv.is_none() || *slot > current_max) && ancestors.contains_key(slot) {
                        rv = Some(i);
                        current_max = *slot;
                    }
                }
            }
        }

        let max_root_inclusive = max_root_inclusive.unwrap_or(Slot::MAX);
        let mut tracker = None;

        for (i, (slot, _t)) in slice.iter().rev().enumerate() {
            if (rv.is_none() || *slot > current_max) && *slot <= max_root_inclusive {
                let lock = match tracker {
                    Some(inner) => inner,
                    None => self.roots_tracker.read().unwrap(),
                };
                if lock.alive_roots.contains(slot) {
                    rv = Some(i);
                    current_max = *slot;
                }
                tracker = Some(lock);
            }
        }

        rv.map(|index| slice.len() - 1 - index)
    }

    pub fn hold_range_in_memory<R>(&self, range: &R, start_holding: bool, thread_pool: &ThreadPool)
    where
        R: RangeBounds<Pubkey> + Debug + Sync,
    {
        let iter = self.iter(Some(range), true);
        iter.hold_range_in_memory(range, start_holding, thread_pool);
    }

    pub fn set_startup(&self, value: Startup) {
        self.storage.set_startup(value);
    }

    pub fn get_startup_remaining_items_to_flush_estimate(&self) -> usize {
        self.storage.get_startup_remaining_items_to_flush_estimate()
    }

    /// For each pubkey, find the slot list in the accounts index
    ///   apply 'avoid_callback_result' if specified.
    ///   otherwise, call `callback`
    pub(crate) fn scan<'a, F, I>(
        &'a self,
        pubkeys: I,
        mut callback: F,
        avoid_callback_result: Option<AccountsIndexScanResult>,
    ) where
        // params:
        //  pubkey looked up
        //  slots_refs is Option<(slot_list, ref_count)>
        //    None if 'pubkey' is not in accounts index.
        //   slot_list: comes from accounts index for 'pubkey'
        //   ref_count: refcount of entry in index
        // if 'avoid_callback_result' is Some(_), then callback is NOT called
        //  and _ is returned as if callback were called.
        F: FnMut(&'a Pubkey, Option<(&SlotList<T>, RefCount)>) -> AccountsIndexScanResult,
        I: Iterator<Item = &'a Pubkey>,
    {
        let mut lock = None;
        let mut last_bin = self.bins(); // too big, won't match
        pubkeys.into_iter().for_each(|pubkey| {
            let bin = self.bin_calculator.bin_from_pubkey(pubkey);
            if bin != last_bin {
                // cannot re-use lock since next pubkey is in a different bin than previous one
                lock = Some(&self.account_maps[bin]);
                last_bin = bin;
            }
            lock.as_ref().unwrap().get_internal(pubkey, |entry| {
                let mut cache = false;
                match entry {
                    Some(locked_entry) => {
                        let result = if let Some(result) = avoid_callback_result.as_ref() {
                            *result
                        } else {
                            let slot_list = &locked_entry.slot_list.read().unwrap();
                            callback(pubkey, Some((slot_list, locked_entry.ref_count())))
                        };
                        cache = match result {
                            AccountsIndexScanResult::Unref => {
                                if locked_entry.unref() {
                                    info!("scan: refcount of item already at 0: {pubkey}");
                                }
                                true
                            }
                            AccountsIndexScanResult::KeepInMemory => true,
                            AccountsIndexScanResult::None => false,
                        };
                    }
                    None => {
                        avoid_callback_result.unwrap_or_else(|| callback(pubkey, None));
                    }
                }
                (cache, ())
            });
        });
    }

    /// Get an account
    /// The latest account that appears in `ancestors` or `roots` is returned.
    pub(crate) fn get(
        &self,
        pubkey: &Pubkey,
        ancestors: Option<&Ancestors>,
        max_root: Option<Slot>,
    ) -> AccountIndexGetResult<T> {
        let read_lock = self.get_bin(pubkey);
        let account = read_lock
            .get(pubkey)
            .map(ReadAccountMapEntry::from_account_map_entry);

        match account {
            Some(locked_entry) => {
                let slot_list = locked_entry.slot_list();
                let found_index = self.latest_slot(ancestors, slot_list, max_root);
                match found_index {
                    Some(found_index) => AccountIndexGetResult::Found(locked_entry, found_index),
                    None => AccountIndexGetResult::NotFound,
                }
            }
            None => AccountIndexGetResult::NotFound,
        }
    }

    // Get the maximum root <= `max_allowed_root` from the given `slice`
    fn get_newest_root_in_slot_list(
        alive_roots: &RollingBitField,
        slice: SlotSlice<T>,
        max_allowed_root_inclusive: Option<Slot>,
    ) -> Slot {
        let mut max_root = 0;
        for (slot, _) in slice.iter() {
            if let Some(max_allowed_root_inclusive) = max_allowed_root_inclusive {
                if *slot > max_allowed_root_inclusive {
                    continue;
                }
            }
            if *slot > max_root && alive_roots.contains(slot) {
                max_root = *slot;
            }
        }
        max_root
    }

    fn update_spl_token_secondary_indexes<G: GenericTokenAccount>(
        &self,
        token_id: &Pubkey,
        pubkey: &Pubkey,
        account_owner: &Pubkey,
        account_data: &[u8],
        account_indexes: &AccountSecondaryIndexes,
    ) {
        if *account_owner == *token_id {
            if account_indexes.contains(&AccountIndex::SplTokenOwner) {
                if let Some(owner_key) = G::unpack_account_owner(account_data) {
                    if account_indexes.include_key(owner_key) {
                        self.spl_token_owner_index.insert(owner_key, pubkey);
                    }
                }
            }

            if account_indexes.contains(&AccountIndex::SplTokenMint) {
                if let Some(mint_key) = G::unpack_account_mint(account_data) {
                    if account_indexes.include_key(mint_key) {
                        self.spl_token_mint_index.insert(mint_key, pubkey);
                    }
                }
            }
        }
    }

    pub fn get_index_key_size(&self, index: &AccountIndex, index_key: &Pubkey) -> Option<usize> {
        match index {
            AccountIndex::ProgramId => self.program_id_index.index.get(index_key).map(|x| x.len()),
            AccountIndex::SplTokenOwner => self
                .spl_token_owner_index
                .index
                .get(index_key)
                .map(|x| x.len()),
            AccountIndex::SplTokenMint => self
                .spl_token_mint_index
                .index
                .get(index_key)
                .map(|x| x.len()),
        }
    }

    /// log any secondary index counts, if non-zero
    pub(crate) fn log_secondary_indexes(&self) {
        if !self.program_id_index.index.is_empty() {
            info!("secondary index: {:?}", AccountIndex::ProgramId);
            self.program_id_index.log_contents();
        }
        if !self.spl_token_mint_index.index.is_empty() {
            info!("secondary index: {:?}", AccountIndex::SplTokenMint);
            self.spl_token_mint_index.log_contents();
        }
        if !self.spl_token_owner_index.index.is_empty() {
            info!("secondary index: {:?}", AccountIndex::SplTokenOwner);
            self.spl_token_owner_index.log_contents();
        }
    }

    pub(crate) fn update_secondary_indexes(
        &self,
        pubkey: &Pubkey,
        account: &impl ReadableAccount,
        account_indexes: &AccountSecondaryIndexes,
    ) {
        if account_indexes.is_empty() {
            return;
        }

        let account_owner = account.owner();
        let account_data = account.data();

        if account_indexes.contains(&AccountIndex::ProgramId)
            && account_indexes.include_key(account_owner)
        {
            self.program_id_index.insert(account_owner, pubkey);
        }
        // Note because of the below check below on the account data length, when an
        // account hits zero lamports and is reset to AccountSharedData::Default, then we skip
        // the below updates to the secondary indexes.
        //
        // Skipping means not updating secondary index to mark the account as missing.
        // This doesn't introduce false positives during a scan because the caller to scan
        // provides the ancestors to check. So even if a zero-lamport account is not yet
        // removed from the secondary index, the scan function will:
        // 1) consult the primary index via `get(&pubkey, Some(ancestors), max_root)`
        // and find the zero-lamport version
        // 2) When the fetch from storage occurs, it will return AccountSharedData::Default
        // (as persisted tombstone for snapshots). This will then ultimately be
        // filtered out by post-scan filters, like in `get_filtered_spl_token_accounts_by_owner()`.

        self.update_spl_token_secondary_indexes::<inline_spl_token::Account>(
            &inline_spl_token::id(),
            pubkey,
            account_owner,
            account_data,
            account_indexes,
        );
        self.update_spl_token_secondary_indexes::<inline_spl_token_2022::Account>(
            &inline_spl_token_2022::id(),
            pubkey,
            account_owner,
            account_data,
            account_indexes,
        );
    }

    pub(crate) fn get_bin(&self, pubkey: &Pubkey) -> AccountMaps<T> {
        &self.account_maps[self.bin_calculator.bin_from_pubkey(pubkey)]
    }

    pub fn bins(&self) -> usize {
        self.account_maps.len()
    }

    // Same functionally to upsert, but:
    // 1. operates on a batch of items
    // 2. holds the write lock for the duration of adding the items
    // Can save time when inserting lots of new keys.
    // But, does NOT update secondary index
    // This is designed to be called at startup time.
    #[allow(clippy::needless_collect)]
    pub(crate) fn insert_new_if_missing_into_primary_index(
        &self,
        slot: Slot,
        item_len: usize,
        items: impl Iterator<Item = (Pubkey, T)>,
    ) -> (Vec<Pubkey>, u64) {
        // big enough so not likely to re-allocate, small enough to not over-allocate by too much
        // this assumes the largest bin contains twice the expected amount of the average size per bin
        let bins = self.bins();
        let expected_items_per_bin = item_len * 2 / bins;
        // offset bin 0 in the 'binned' array by a random amount.
        // This results in calls to insert_new_entry_if_missing_with_lock from different threads starting at different bins.
        let random_offset = thread_rng().gen_range(0, bins);
        let use_disk = self.storage.storage.disk.is_some();
        let mut binned = (0..bins)
            .map(|mut pubkey_bin| {
                // opposite of (pubkey_bin + random_offset) % bins
                pubkey_bin = if pubkey_bin < random_offset {
                    pubkey_bin + bins - random_offset
                } else {
                    pubkey_bin - random_offset
                };
                (pubkey_bin, Vec::with_capacity(expected_items_per_bin))
            })
            .collect::<Vec<_>>();
        let dirty_pubkeys = items
            .filter_map(|(pubkey, account_info)| {
                let pubkey_bin = self.bin_calculator.bin_from_pubkey(&pubkey);
                let binned_index = (pubkey_bin + random_offset) % bins;
                // this value is equivalent to what update() below would have created if we inserted a new item
                let is_zero_lamport = account_info.is_zero_lamport();
                let result = if is_zero_lamport { Some(pubkey) } else { None };

                binned[binned_index].1.push((pubkey, account_info));
                result
            })
            .collect::<Vec<_>>();
        binned.retain(|x| !x.1.is_empty());

        let insertion_time = AtomicU64::new(0);

        binned.into_iter().for_each(|(pubkey_bin, items)| {
            let r_account_maps = &self.account_maps[pubkey_bin];
            let mut insert_time = Measure::start("insert_into_primary_index");
            if use_disk {
                r_account_maps.startup_insert_only(slot, items.into_iter());
            } else {
                // not using disk buckets, so just write to in-mem
                // this is no longer the default case
                items.into_iter().for_each(|(pubkey, account_info)| {
                    let new_entry = PreAllocatedAccountMapEntry::new(
                        slot,
                        account_info,
                        &self.storage.storage,
                        use_disk,
                    );
                    r_account_maps.insert_new_entry_if_missing_with_lock(pubkey, new_entry);
                });
            }
            insert_time.stop();
            insertion_time.fetch_add(insert_time.as_us(), Ordering::Relaxed);
        });

        (dirty_pubkeys, insertion_time.load(Ordering::Relaxed))
    }

    /// return Vec<Vec<>> because the internal vecs are already allocated per bin
    pub fn retrieve_duplicate_keys_from_startup(&self) -> Vec<Vec<(Slot, Pubkey)>> {
        (0..self.bins())
            .map(|pubkey_bin| {
                let r_account_maps = &self.account_maps[pubkey_bin];
                r_account_maps.retrieve_duplicate_keys_from_startup()
            })
            .collect()
    }

    /// Updates the given pubkey at the given slot with the new account information.
    /// on return, the index's previous account info may be returned in 'reclaims' depending on 'previous_slot_entry_was_cached'
    pub fn upsert(
        &self,
        new_slot: Slot,
        old_slot: Slot,
        pubkey: &Pubkey,
        account: &impl ReadableAccount,
        account_indexes: &AccountSecondaryIndexes,
        account_info: T,
        reclaims: &mut SlotList<T>,
        reclaim: UpsertReclaim,
    ) {
        // vast majority of updates are to item already in accounts index, so store as raw to avoid unnecessary allocations
        let store_raw = true;

        // We don't atomically update both primary index and secondary index together.
        // This certainly creates a small time window with inconsistent state across the two indexes.
        // However, this is acceptable because:
        //
        //  - A strict consistent view at any given moment of time is not necessary, because the only
        //  use case for the secondary index is `scan`, and `scans` are only supported/require consistency
        //  on frozen banks, and this inconsistency is only possible on working banks.
        //
        //  - The secondary index is never consulted as primary source of truth for gets/stores.
        //  So, what the accounts_index sees alone is sufficient as a source of truth for other non-scan
        //  account operations.
        let new_item = PreAllocatedAccountMapEntry::new(
            new_slot,
            account_info,
            &self.storage.storage,
            store_raw,
        );
        let map = self.get_bin(pubkey);

        map.upsert(pubkey, new_item, Some(old_slot), reclaims, reclaim);
        self.update_secondary_indexes(pubkey, account, account_indexes);
    }

    pub fn ref_count_from_storage(&self, pubkey: &Pubkey) -> RefCount {
        let map = self.get_bin(pubkey);
        map.get_internal(pubkey, |entry| {
            (
                false,
                entry.map(|entry| entry.ref_count()).unwrap_or_default(),
            )
        })
    }

    fn purge_secondary_indexes_by_inner_key(
        &self,
        inner_key: &Pubkey,
        account_indexes: &AccountSecondaryIndexes,
    ) {
        if account_indexes.contains(&AccountIndex::ProgramId) {
            self.program_id_index.remove_by_inner_key(inner_key);
        }

        if account_indexes.contains(&AccountIndex::SplTokenOwner) {
            self.spl_token_owner_index.remove_by_inner_key(inner_key);
        }

        if account_indexes.contains(&AccountIndex::SplTokenMint) {
            self.spl_token_mint_index.remove_by_inner_key(inner_key);
        }
    }

    fn purge_older_root_entries(
        &self,
        slot_list: &mut SlotList<T>,
        reclaims: &mut SlotList<T>,
        max_clean_root_inclusive: Option<Slot>,
    ) {
        let newest_root_in_slot_list;
        let max_clean_root_inclusive = {
            let roots_tracker = &self.roots_tracker.read().unwrap();
            newest_root_in_slot_list = Self::get_newest_root_in_slot_list(
                &roots_tracker.alive_roots,
                slot_list,
                max_clean_root_inclusive,
            );
            max_clean_root_inclusive.unwrap_or_else(|| roots_tracker.alive_roots.max_inclusive())
        };

        slot_list.retain(|(slot, value)| {
            let should_purge = Self::can_purge_older_entries(
                // Note that we have a root that is inclusive here.
                // Calling a function that expects 'exclusive'
                // This is expected behavior for this call.
                max_clean_root_inclusive,
                newest_root_in_slot_list,
                *slot,
            ) && !value.is_cached();
            if should_purge {
                reclaims.push((*slot, *value));
            }
            !should_purge
        });
    }

    /// return true if pubkey was removed from the accounts index
    ///  or does not exist in the accounts index
    /// This means it should NOT be unref'd later.
    #[must_use]
    pub fn clean_rooted_entries(
        &self,
        pubkey: &Pubkey,
        reclaims: &mut SlotList<T>,
        max_clean_root_inclusive: Option<Slot>,
    ) -> bool {
        let mut is_slot_list_empty = false;
        let missing_in_accounts_index = self
            .slot_list_mut(pubkey, |slot_list| {
                self.purge_older_root_entries(slot_list, reclaims, max_clean_root_inclusive);
                is_slot_list_empty = slot_list.is_empty();
            })
            .is_none();

        let mut removed = false;
        // If the slot list is empty, remove the pubkey from `account_maps`. Make sure to grab the
        // lock and double check the slot list is still empty, because another writer could have
        // locked and inserted the pubkey in-between when `is_slot_list_empty=true` and the call to
        // remove() below.
        if is_slot_list_empty {
            let w_maps = self.get_bin(pubkey);
            removed = w_maps.remove_if_slot_list_empty(*pubkey);
        }
        removed || missing_in_accounts_index
    }

    /// When can an entry be purged?
    ///
    /// If we get a slot update where slot != newest_root_in_slot_list for an account where slot <
    /// max_clean_root_exclusive, then we know it's safe to delete because:
    ///
    /// a) If slot < newest_root_in_slot_list, then we know the update is outdated by a later rooted
    /// update, namely the one in newest_root_in_slot_list
    ///
    /// b) If slot > newest_root_in_slot_list, then because slot < max_clean_root_exclusive and we know there are
    /// no roots in the slot list between newest_root_in_slot_list and max_clean_root_exclusive, (otherwise there
    /// would be a bigger newest_root_in_slot_list, which is a contradiction), then we know slot must be
    /// an unrooted slot less than max_clean_root_exclusive and thus safe to clean as well.
    fn can_purge_older_entries(
        max_clean_root_exclusive: Slot,
        newest_root_in_slot_list: Slot,
        slot: Slot,
    ) -> bool {
        slot < max_clean_root_exclusive && slot != newest_root_in_slot_list
    }

    /// Given a list of slots, return a new list of only the slots that are rooted
    pub fn get_rooted_from_list<'a>(&self, slots: impl Iterator<Item = &'a Slot>) -> Vec<Slot> {
        let roots_tracker = self.roots_tracker.read().unwrap();
        slots
            .filter_map(|s| {
                if roots_tracker.alive_roots.contains(s) {
                    Some(*s)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn is_alive_root(&self, slot: Slot) -> bool {
        self.roots_tracker
            .read()
            .unwrap()
            .alive_roots
            .contains(&slot)
    }

    pub fn add_root(&self, slot: Slot) {
        self.roots_added.fetch_add(1, Ordering::Relaxed);
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        // `AccountsDb::flush_accounts_cache()` relies on roots being added in order
        assert!(slot >= w_roots_tracker.alive_roots.max_inclusive());
        // 'slot' is a root, so it is both 'root' and 'original'
        w_roots_tracker.alive_roots.insert(slot);
        w_roots_tracker.historical_roots.insert(slot);
    }

    pub fn add_uncleaned_roots<I>(&self, roots: I)
    where
        I: IntoIterator<Item = Slot>,
    {
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.uncleaned_roots.extend(roots);
    }

    pub fn max_root_inclusive(&self) -> Slot {
        self.roots_tracker
            .read()
            .unwrap()
            .alive_roots
            .max_inclusive()
    }

    /// return the lowest original root >= slot, including historical_roots and ancestors
    pub fn get_next_original_root(
        &self,
        slot: Slot,
        ancestors: Option<&Ancestors>,
    ) -> Option<Slot> {
        {
            let roots_tracker = self.roots_tracker.read().unwrap();
            for root in slot..roots_tracker.historical_roots.max_exclusive() {
                if roots_tracker.historical_roots.contains(&root) {
                    return Some(root);
                }
            }
        }
        // ancestors are higher than roots, so look for roots first
        if let Some(ancestors) = ancestors {
            let min = std::cmp::max(slot, ancestors.min_slot());
            for root in min..=ancestors.max_slot() {
                if ancestors.contains_key(&root) {
                    return Some(root);
                }
            }
        }
        None
    }

    /// roots are inserted into 'historical_roots' and 'roots' as a new root is made.
    /// roots are removed form 'roots' as all entries in the append vec become outdated.
    /// This function exists to clean older entries from 'historical_roots'.
    /// all roots < 'oldest_slot_to_keep' are removed from 'historical_roots'.
    pub fn remove_old_historical_roots(&self, oldest_slot_to_keep: Slot, keep: &HashSet<Slot>) {
        let mut roots = self
            .roots_tracker
            .read()
            .unwrap()
            .historical_roots
            .get_all_less_than(oldest_slot_to_keep);
        roots.retain(|root| !keep.contains(root));
        if !roots.is_empty() {
            let mut w_roots_tracker = self.roots_tracker.write().unwrap();
            roots.into_iter().for_each(|root| {
                w_roots_tracker.historical_roots.remove(&root);
            });
        }
    }

    /// Remove the slot when the storage for the slot is freed
    /// Accounts no longer reference this slot.
    /// return true if slot was a root
    pub fn clean_dead_slot(&self, slot: Slot, stats: &mut AccountsIndexRootsStats) -> bool {
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        let removed_from_unclean_roots = w_roots_tracker.uncleaned_roots.remove(&slot);
        let removed_from_previous_uncleaned_roots =
            w_roots_tracker.previous_uncleaned_roots.remove(&slot);
        if !w_roots_tracker.alive_roots.remove(&slot) {
            if removed_from_unclean_roots {
                error!("clean_dead_slot-removed_from_unclean_roots: {}", slot);
                inc_new_counter_error!("clean_dead_slot-removed_from_unclean_roots", 1, 1);
            }
            if removed_from_previous_uncleaned_roots {
                error!(
                    "clean_dead_slot-removed_from_previous_uncleaned_roots: {}",
                    slot
                );
                inc_new_counter_error!(
                    "clean_dead_slot-removed_from_previous_uncleaned_roots",
                    1,
                    1
                );
            }
            false
        } else {
            stats.roots_len = Some(w_roots_tracker.alive_roots.len());
            stats.uncleaned_roots_len = Some(w_roots_tracker.uncleaned_roots.len());
            stats.previous_uncleaned_roots_len =
                Some(w_roots_tracker.previous_uncleaned_roots.len());
            stats.roots_range = Some(w_roots_tracker.alive_roots.range_width());
            stats.historical_roots_len = Some(w_roots_tracker.historical_roots.len());
            drop(w_roots_tracker);
            self.roots_removed.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    pub fn min_alive_root(&self) -> Option<Slot> {
        self.roots_tracker.read().unwrap().min_alive_root()
    }

    pub fn reset_uncleaned_roots(&self, max_clean_root: Option<Slot>) -> HashSet<Slot> {
        let mut cleaned_roots = HashSet::new();
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.uncleaned_roots.retain(|root| {
            let is_cleaned = max_clean_root
                .map(|max_clean_root| *root <= max_clean_root)
                .unwrap_or(true);
            if is_cleaned {
                cleaned_roots.insert(*root);
            }
            // Only keep the slots that have yet to be cleaned
            !is_cleaned
        });
        std::mem::replace(&mut w_roots_tracker.previous_uncleaned_roots, cleaned_roots)
    }

    #[cfg(test)]
    pub fn clear_uncleaned_roots(&self, max_clean_root: Option<Slot>) -> HashSet<Slot> {
        let mut cleaned_roots = HashSet::new();
        let mut w_roots_tracker = self.roots_tracker.write().unwrap();
        w_roots_tracker.uncleaned_roots.retain(|root| {
            let is_cleaned = max_clean_root
                .map(|max_clean_root| *root <= max_clean_root)
                .unwrap_or(true);
            if is_cleaned {
                cleaned_roots.insert(*root);
            }
            // Only keep the slots that have yet to be cleaned
            !is_cleaned
        });
        cleaned_roots
    }

    pub fn is_uncleaned_root(&self, slot: Slot) -> bool {
        self.roots_tracker
            .read()
            .unwrap()
            .uncleaned_roots
            .contains(&slot)
    }

    pub fn num_alive_roots(&self) -> usize {
        self.roots_tracker.read().unwrap().alive_roots.len()
    }

    pub fn all_alive_roots(&self) -> Vec<Slot> {
        let tracker = self.roots_tracker.read().unwrap();
        tracker.alive_roots.get_all()
    }

    #[cfg(test)]
    pub fn clear_roots(&self) {
        self.roots_tracker.write().unwrap().alive_roots.clear()
    }

    pub fn clone_uncleaned_roots(&self) -> HashSet<Slot> {
        self.roots_tracker.read().unwrap().uncleaned_roots.clone()
    }

    pub fn uncleaned_roots_len(&self) -> usize {
        self.roots_tracker.read().unwrap().uncleaned_roots.len()
    }

    #[cfg(test)]
    // filter any rooted entries and return them along with a bool that indicates
    // if this account has no more entries. Note this does not update the secondary
    // indexes!
    pub fn purge_roots(&self, pubkey: &Pubkey) -> (SlotList<T>, bool) {
        self.slot_list_mut(pubkey, |slot_list| {
            let reclaims = self.get_rooted_entries(slot_list, None);
            slot_list.retain(|(slot, _)| !self.is_alive_root(*slot));
            (reclaims, slot_list.is_empty())
        })
        .unwrap()
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::inline_spl_token::*,
        solana_sdk::{
            account::{AccountSharedData, WritableAccount},
            pubkey::PUBKEY_BYTES,
        },
        std::ops::RangeInclusive,
    };

    pub enum SecondaryIndexTypes<'a> {
        RwLock(&'a SecondaryIndex<RwLockSecondaryIndexEntry>),
        DashMap(&'a SecondaryIndex<DashMapSecondaryIndexEntry>),
    }

    pub fn spl_token_mint_index_enabled() -> AccountSecondaryIndexes {
        let mut account_indexes = HashSet::new();
        account_indexes.insert(AccountIndex::SplTokenMint);
        AccountSecondaryIndexes {
            indexes: account_indexes,
            keys: None,
        }
    }

    pub fn spl_token_owner_index_enabled() -> AccountSecondaryIndexes {
        let mut account_indexes = HashSet::new();
        account_indexes.insert(AccountIndex::SplTokenOwner);
        AccountSecondaryIndexes {
            indexes: account_indexes,
            keys: None,
        }
    }

    impl<T: IndexValue> AccountIndexGetResult<T> {
        pub fn unwrap(self) -> (ReadAccountMapEntry<T>, usize) {
            match self {
                AccountIndexGetResult::Found(lock, size) => (lock, size),
                _ => {
                    panic!("trying to unwrap AccountIndexGetResult with non-Success result");
                }
            }
        }

        pub fn is_none(&self) -> bool {
            !self.is_some()
        }

        pub fn is_some(&self) -> bool {
            matches!(self, AccountIndexGetResult::Found(_lock, _size))
        }

        pub fn map<V, F: FnOnce((ReadAccountMapEntry<T>, usize)) -> V>(self, f: F) -> Option<V> {
            match self {
                AccountIndexGetResult::Found(lock, size) => Some(f((lock, size))),
                _ => None,
            }
        }
    }

    fn create_dashmap_secondary_index_state() -> (usize, usize, AccountSecondaryIndexes) {
        {
            // Check that we're actually testing the correct variant
            let index = AccountsIndex::<bool>::default_for_tests();
            let _type_check = SecondaryIndexTypes::DashMap(&index.spl_token_mint_index);
        }

        (0, PUBKEY_BYTES, spl_token_mint_index_enabled())
    }

    fn create_rwlock_secondary_index_state() -> (usize, usize, AccountSecondaryIndexes) {
        {
            // Check that we're actually testing the correct variant
            let index = AccountsIndex::<bool>::default_for_tests();
            let _type_check = SecondaryIndexTypes::RwLock(&index.spl_token_owner_index);
        }

        (
            SPL_TOKEN_ACCOUNT_OWNER_OFFSET,
            SPL_TOKEN_ACCOUNT_OWNER_OFFSET + PUBKEY_BYTES,
            spl_token_owner_index_enabled(),
        )
    }

    impl<T: IndexValue> Clone for PreAllocatedAccountMapEntry<T> {
        fn clone(&self) -> Self {
            // clone the AccountMapEntryInner into a new Arc
            match self {
                PreAllocatedAccountMapEntry::Entry(entry) => {
                    let (slot, account_info) = entry.slot_list.read().unwrap()[0];
                    let meta = AccountMapEntryMeta {
                        dirty: AtomicBool::new(entry.dirty()),
                        age: AtomicU8::new(entry.age()),
                    };
                    PreAllocatedAccountMapEntry::Entry(Arc::new(AccountMapEntryInner::new(
                        vec![(slot, account_info)],
                        entry.ref_count(),
                        meta,
                    )))
                }
                PreAllocatedAccountMapEntry::Raw(raw) => PreAllocatedAccountMapEntry::Raw(*raw),
            }
        }
    }

    impl<T: IndexValue> AccountsIndex<T> {
        /// provides the ability to refactor this function on the api without bloody changes
        pub fn get_for_tests(
            &self,
            pubkey: &Pubkey,
            ancestors: Option<&Ancestors>,
            max_root: Option<Slot>,
        ) -> AccountIndexGetResult<T> {
            self.get(pubkey, ancestors, max_root)
        }
    }

    #[test]
    fn test_get_next_original_root() {
        let ancestors = None;
        let index = AccountsIndex::<bool>::default_for_tests();
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), None);
        }
        // roots are now [1]. 0 and 1 both return 1
        index.add_root(1);
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(1));
        }
        assert_eq!(index.get_next_original_root(2, ancestors), None); // no roots after 1, so asking for root >= 2 is None

        // roots are now [1, 3]. 0 and 1 both return 1. 2 and 3 both return 3
        index.add_root(3);
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(1));
        }
        for slot in 2..4 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(3));
        }
        assert_eq!(index.get_next_original_root(4, ancestors), None); // no roots after 3, so asking for root >= 4 is None
    }

    #[test]
    fn test_get_next_original_root_ancestors() {
        let orig_ancestors = Ancestors::default();
        let ancestors = Some(&orig_ancestors);
        let index = AccountsIndex::<bool>::default_for_tests();
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), None);
        }
        // ancestors are now [1]. 0 and 1 both return 1
        let orig_ancestors = Ancestors::from(vec![1]);
        let ancestors = Some(&orig_ancestors);
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(1));
        }
        assert_eq!(index.get_next_original_root(2, ancestors), None); // no roots after 1, so asking for root >= 2 is None

        // ancestors are now [1, 3]. 0 and 1 both return 1. 2 and 3 both return 3
        let orig_ancestors = Ancestors::from(vec![1, 3]);
        let ancestors = Some(&orig_ancestors);
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(1));
        }
        for slot in 2..4 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(3));
        }
        assert_eq!(index.get_next_original_root(4, ancestors), None); // no roots after 3, so asking for root >= 4 is None
    }

    #[test]
    fn test_get_next_original_root_roots_and_ancestors() {
        let orig_ancestors = Ancestors::default();
        let ancestors = Some(&orig_ancestors);
        let index = AccountsIndex::<bool>::default_for_tests();
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), None);
        }
        // roots are now [1]. 0 and 1 both return 1
        index.add_root(1);
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(1));
        }
        assert_eq!(index.get_next_original_root(2, ancestors), None); // no roots after 1, so asking for root >= 2 is None

        // roots are now [1] and ancestors are now [3]. 0 and 1 both return 1. 2 and 3 both return 3
        let orig_ancestors = Ancestors::from(vec![3]);
        let ancestors = Some(&orig_ancestors);
        for slot in 0..2 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(1));
        }
        for slot in 2..4 {
            assert_eq!(index.get_next_original_root(slot, ancestors), Some(3));
        }
        assert_eq!(index.get_next_original_root(4, ancestors), None); // no roots after 3, so asking for root >= 4 is None
    }

    #[test]
    fn test_remove_old_historical_roots() {
        let index = AccountsIndex::<bool>::default_for_tests();
        index.add_root(1);
        index.add_root(2);
        assert_eq!(
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .get_all(),
            vec![1, 2]
        );
        let empty_hash_set = HashSet::default();
        index.remove_old_historical_roots(2, &empty_hash_set);
        assert_eq!(
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .get_all(),
            vec![2]
        );
        index.remove_old_historical_roots(3, &empty_hash_set);
        assert!(
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .is_empty(),
            "{:?}",
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .get_all()
        );

        // now use 'keep'
        let index = AccountsIndex::<bool>::default_for_tests();
        index.add_root(1);
        index.add_root(2);
        let hash_set_1 = vec![1].into_iter().collect();
        assert_eq!(
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .get_all(),
            vec![1, 2]
        );
        index.remove_old_historical_roots(2, &hash_set_1);
        assert_eq!(
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .get_all(),
            vec![1, 2]
        );
        index.remove_old_historical_roots(3, &hash_set_1);
        assert_eq!(
            index
                .roots_tracker
                .read()
                .unwrap()
                .historical_roots
                .get_all(),
            vec![1]
        );
    }

    const COLLECT_ALL_UNSORTED_FALSE: bool = false;

    #[test]
    fn test_get_empty() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let ancestors = Ancestors::default();
        let key = &key;
        assert!(index.get_for_tests(key, Some(&ancestors), None).is_none());
        assert!(index.get_for_tests(key, None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 0);
    }

    #[test]
    fn test_secondary_index_include_exclude() {
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();
        let mut index = AccountSecondaryIndexes::default();

        assert!(!index.contains(&AccountIndex::ProgramId));
        index.indexes.insert(AccountIndex::ProgramId);
        assert!(index.contains(&AccountIndex::ProgramId));
        assert!(index.include_key(&pk1));
        assert!(index.include_key(&pk2));

        let exclude = false;
        index.keys = Some(AccountSecondaryIndexesIncludeExclude {
            keys: [pk1].iter().cloned().collect::<HashSet<_>>(),
            exclude,
        });
        assert!(index.include_key(&pk1));
        assert!(!index.include_key(&pk2));

        let exclude = true;
        index.keys = Some(AccountSecondaryIndexesIncludeExclude {
            keys: [pk1].iter().cloned().collect::<HashSet<_>>(),
            exclude,
        });
        assert!(!index.include_key(&pk1));
        assert!(index.include_key(&pk2));

        let exclude = true;
        index.keys = Some(AccountSecondaryIndexesIncludeExclude {
            keys: [pk1, pk2].iter().cloned().collect::<HashSet<_>>(),
            exclude,
        });
        assert!(!index.include_key(&pk1));
        assert!(!index.include_key(&pk2));

        let exclude = false;
        index.keys = Some(AccountSecondaryIndexesIncludeExclude {
            keys: [pk1, pk2].iter().cloned().collect::<HashSet<_>>(),
            exclude,
        });
        assert!(index.include_key(&pk1));
        assert!(index.include_key(&pk2));
    }

    const UPSERT_POPULATE_RECLAIMS: UpsertReclaim = UpsertReclaim::PopulateReclaims;

    #[test]
    fn test_insert_no_ancestors() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());

        let ancestors = Ancestors::default();
        assert!(index.get_for_tests(&key, Some(&ancestors), None).is_none());
        assert!(index.get_for_tests(&key, None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 0);
    }

    type AccountInfoTest = f64;

    impl IndexValue for AccountInfoTest {}
    impl IsCached for AccountInfoTest {
        fn is_cached(&self) -> bool {
            true
        }
    }

    impl ZeroLamport for AccountInfoTest {
        fn is_zero_lamport(&self) -> bool {
            true
        }
    }
    #[test]
    fn test_insert_new_with_lock_no_ancestors() {
        let key = solana_sdk::pubkey::new_rand();
        let pubkey = &key;
        let slot = 0;

        let index = AccountsIndex::<bool>::default_for_tests();
        let account_info = true;
        let items = vec![(*pubkey, account_info)];
        index.set_startup(Startup::Startup);
        index.insert_new_if_missing_into_primary_index(slot, items.len(), items.into_iter());
        index.set_startup(Startup::Normal);

        let mut ancestors = Ancestors::default();
        assert!(index
            .get_for_tests(pubkey, Some(&ancestors), None)
            .is_none());
        assert!(index.get_for_tests(pubkey, None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 0);
        ancestors.insert(slot, 0);
        assert!(index
            .get_for_tests(pubkey, Some(&ancestors), None)
            .is_some());
        assert_eq!(index.ref_count_from_storage(pubkey), 1);
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 1);

        // not zero lamports
        let index = AccountsIndex::<AccountInfoTest>::default_for_tests();
        let account_info: AccountInfoTest = 0 as AccountInfoTest;
        let items = vec![(*pubkey, account_info)];
        index.set_startup(Startup::Startup);
        index.insert_new_if_missing_into_primary_index(slot, items.len(), items.into_iter());
        index.set_startup(Startup::Normal);

        let mut ancestors = Ancestors::default();
        assert!(index
            .get_for_tests(pubkey, Some(&ancestors), None)
            .is_none());
        assert!(index.get_for_tests(pubkey, None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 0);
        ancestors.insert(slot, 0);
        assert!(index
            .get_for_tests(pubkey, Some(&ancestors), None)
            .is_some());
        assert_eq!(index.ref_count_from_storage(pubkey), 0); // cached, so 0
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 1);
    }

    fn get_pre_allocated<T: IndexValue>(
        slot: Slot,
        account_info: T,
        storage: &Arc<BucketMapHolder<T>>,
        store_raw: bool,
        to_raw_first: bool,
    ) -> PreAllocatedAccountMapEntry<T> {
        let entry = PreAllocatedAccountMapEntry::new(slot, account_info, storage, store_raw);

        if to_raw_first {
            // convert to raw
            let (slot2, account_info2) = entry.into();
            // recreate using extracted raw
            PreAllocatedAccountMapEntry::new(slot2, account_info2, storage, store_raw)
        } else {
            entry
        }
    }

    #[test]
    fn test_new_entry() {
        for store_raw in [false, true] {
            for to_raw_first in [false, true] {
                let slot = 0;
                // account_info type that IS cached
                let account_info = AccountInfoTest::default();
                let index = AccountsIndex::default_for_tests();

                let new_entry = get_pre_allocated(
                    slot,
                    account_info,
                    &index.storage.storage,
                    store_raw,
                    to_raw_first,
                )
                .into_account_map_entry(&index.storage.storage);
                assert_eq!(new_entry.ref_count(), 0);
                assert_eq!(new_entry.slot_list.read().unwrap().capacity(), 1);
                assert_eq!(
                    new_entry.slot_list.read().unwrap().to_vec(),
                    vec![(slot, account_info)]
                );

                // account_info type that is NOT cached
                let account_info = true;
                let index = AccountsIndex::default_for_tests();

                let new_entry = get_pre_allocated(
                    slot,
                    account_info,
                    &index.storage.storage,
                    store_raw,
                    to_raw_first,
                )
                .into_account_map_entry(&index.storage.storage);
                assert_eq!(new_entry.ref_count(), 1);
                assert_eq!(new_entry.slot_list.read().unwrap().capacity(), 1);
                assert_eq!(
                    new_entry.slot_list.read().unwrap().to_vec(),
                    vec![(slot, account_info)]
                );
            }
        }
    }

    #[test]
    fn test_batch_insert() {
        let slot0 = 0;
        let key0 = solana_sdk::pubkey::new_rand();
        let key1 = solana_sdk::pubkey::new_rand();

        let index = AccountsIndex::<bool>::default_for_tests();
        let account_infos = [true, false];

        index.set_startup(Startup::Startup);
        let items = vec![(key0, account_infos[0]), (key1, account_infos[1])];
        index.insert_new_if_missing_into_primary_index(slot0, items.len(), items.into_iter());
        index.set_startup(Startup::Normal);

        for (i, key) in [key0, key1].iter().enumerate() {
            let entry = index.get_account_read_entry(key).unwrap();
            assert_eq!(entry.ref_count(), 1);
            assert_eq!(entry.slot_list().to_vec(), vec![(slot0, account_infos[i]),]);
        }
    }

    fn test_new_entry_code_paths_helper<T: IndexValue>(
        account_infos: [T; 2],
        is_cached: bool,
        upsert: bool,
        use_disk: bool,
    ) {
        if is_cached && !upsert {
            // This is an illegal combination when we are using queued lazy inserts.
            // Cached items don't ever leave the in-mem cache.
            // But the queued lazy insert code relies on there being nothing in the in-mem cache.
            return;
        }

        let slot0 = 0;
        let slot1 = 1;
        let key = solana_sdk::pubkey::new_rand();

        let mut config = ACCOUNTS_INDEX_CONFIG_FOR_TESTING;
        config.index_limit_mb = if use_disk {
            IndexLimitMb::Limit(10_000)
        } else {
            IndexLimitMb::InMemOnly // in-mem only
        };
        let index = AccountsIndex::<T>::new(Some(config), &Arc::default());
        let mut gc = Vec::new();

        if upsert {
            // insert first entry for pubkey. This will use new_entry_after_update and not call update.
            index.upsert(
                slot0,
                slot0,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                account_infos[0],
                &mut gc,
                UPSERT_POPULATE_RECLAIMS,
            );
        } else {
            let items = vec![(key, account_infos[0])];
            index.set_startup(Startup::Startup);
            index.insert_new_if_missing_into_primary_index(slot0, items.len(), items.into_iter());
            index.set_startup(Startup::Normal);
        }
        assert!(gc.is_empty());

        // verify the added entry matches expected
        {
            let entry = index.get_account_read_entry(&key).unwrap();
            assert_eq!(entry.ref_count(), u64::from(!is_cached));
            let expected = vec![(slot0, account_infos[0])];
            assert_eq!(entry.slot_list().to_vec(), expected);
            let new_entry: AccountMapEntry<_> = PreAllocatedAccountMapEntry::new(
                slot0,
                account_infos[0],
                &index.storage.storage,
                false,
            )
            .into_account_map_entry(&index.storage.storage);
            assert_eq!(
                entry.slot_list().to_vec(),
                new_entry.slot_list.read().unwrap().to_vec(),
            );
        }

        // insert second entry for pubkey. This will use update and NOT use new_entry_after_update.
        if upsert {
            index.upsert(
                slot1,
                slot1,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                account_infos[1],
                &mut gc,
                UPSERT_POPULATE_RECLAIMS,
            );
        } else {
            // this has the effect of aging out everything in the in-mem cache
            for _ in 0..5 {
                index.set_startup(Startup::Startup);
                index.set_startup(Startup::Normal);
            }

            let items = vec![(key, account_infos[1])];
            index.set_startup(Startup::Startup);
            index.insert_new_if_missing_into_primary_index(slot1, items.len(), items.into_iter());
            index.set_startup(Startup::Normal);
        }
        assert!(gc.is_empty());

        for lock in &[false, true] {
            let read_lock = if *lock {
                Some(index.get_bin(&key))
            } else {
                None
            };

            let entry = if *lock {
                index
                    .get_account_read_entry_with_lock(&key, read_lock.as_ref().unwrap())
                    .unwrap()
            } else {
                index.get_account_read_entry(&key).unwrap()
            };

            assert_eq!(entry.ref_count(), if is_cached { 0 } else { 2 });
            assert_eq!(
                entry.slot_list().to_vec(),
                vec![(slot0, account_infos[0]), (slot1, account_infos[1])]
            );

            let new_entry = PreAllocatedAccountMapEntry::new(
                slot1,
                account_infos[1],
                &index.storage.storage,
                false,
            );
            assert_eq!(entry.slot_list()[1], new_entry.into());
        }
    }

    #[test]
    fn test_new_entry_and_update_code_paths() {
        for use_disk in [false, true] {
            for is_upsert in &[false, true] {
                // account_info type that IS cached
                test_new_entry_code_paths_helper([1.0, 2.0], true, *is_upsert, use_disk);

                // account_info type that is NOT cached
                test_new_entry_code_paths_helper([true, false], false, *is_upsert, use_disk);
            }
        }
    }

    #[test]
    fn test_insert_with_lock_no_ancestors() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let slot = 0;
        let account_info = true;

        let new_entry =
            PreAllocatedAccountMapEntry::new(slot, account_info, &index.storage.storage, false);
        assert_eq!(0, account_maps_stats_len(&index));
        assert_eq!((slot, account_info), new_entry.clone().into());

        assert_eq!(0, account_maps_stats_len(&index));
        let r_account_maps = index.get_bin(&key);
        r_account_maps.upsert(
            &key,
            new_entry,
            None,
            &mut SlotList::default(),
            UPSERT_POPULATE_RECLAIMS,
        );
        assert_eq!(1, account_maps_stats_len(&index));

        let mut ancestors = Ancestors::default();
        assert!(index.get_for_tests(&key, Some(&ancestors), None).is_none());
        assert!(index.get_for_tests(&key, None, None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 0);
        ancestors.insert(slot, 0);
        assert!(index.get_for_tests(&key, Some(&ancestors), None).is_some());
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 1);
    }

    #[test]
    fn test_insert_wrong_ancestors() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());

        let ancestors = vec![(1, 1)].into_iter().collect();
        assert!(index.get_for_tests(&key, Some(&ancestors), None).is_none());

        let mut num = 0;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |_pubkey, _index| num += 1,
            &ScanConfig::default(),
        );
        assert_eq!(num, 0);
    }
    #[test]
    fn test_insert_ignore_reclaims() {
        {
            // non-cached
            let key = solana_sdk::pubkey::new_rand();
            let index = AccountsIndex::<u64>::default_for_tests();
            let mut reclaims = Vec::new();
            let slot = 0;
            let value = 1;
            assert!(!value.is_cached());
            index.upsert(
                slot,
                slot,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut reclaims,
                UpsertReclaim::PopulateReclaims,
            );
            assert!(reclaims.is_empty());
            index.upsert(
                slot,
                slot,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut reclaims,
                UpsertReclaim::PopulateReclaims,
            );
            // reclaimed
            assert!(!reclaims.is_empty());
            reclaims.clear();
            index.upsert(
                slot,
                slot,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut reclaims,
                // since IgnoreReclaims, we should expect reclaims to be empty
                UpsertReclaim::IgnoreReclaims,
            );
            // reclaims is ignored
            assert!(reclaims.is_empty());
        }
        {
            // cached
            let key = solana_sdk::pubkey::new_rand();
            let index = AccountsIndex::<AccountInfoTest>::default_for_tests();
            let mut reclaims = Vec::new();
            let slot = 0;
            let value = 1.0;
            assert!(value.is_cached());
            index.upsert(
                slot,
                slot,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut reclaims,
                UpsertReclaim::PopulateReclaims,
            );
            assert!(reclaims.is_empty());
            index.upsert(
                slot,
                slot,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut reclaims,
                UpsertReclaim::PopulateReclaims,
            );
            // reclaimed
            assert!(!reclaims.is_empty());
            reclaims.clear();
            index.upsert(
                slot,
                slot,
                &key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut reclaims,
                // since IgnoreReclaims, we should expect reclaims to be empty
                UpsertReclaim::IgnoreReclaims,
            );
            // reclaims is ignored
            assert!(reclaims.is_empty());
        }
    }

    #[test]
    fn test_insert_with_ancestors() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());

        let ancestors = vec![(0, 0)].into_iter().collect();
        let (list, idx) = index.get_for_tests(&key, Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));

        let mut num = 0;
        let mut found_key = false;
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |pubkey, _index| {
                if pubkey == &key {
                    found_key = true
                };
                num += 1
            },
            &ScanConfig::default(),
        );
        assert_eq!(num, 1);
        assert!(found_key);
    }

    fn setup_accounts_index_keys(num_pubkeys: usize) -> (AccountsIndex<bool>, Vec<Pubkey>) {
        let index = AccountsIndex::<bool>::default_for_tests();
        let root_slot = 0;

        let mut pubkeys: Vec<Pubkey> = std::iter::repeat_with(|| {
            let new_pubkey = solana_sdk::pubkey::new_rand();
            index.upsert(
                root_slot,
                root_slot,
                &new_pubkey,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                true,
                &mut vec![],
                UPSERT_POPULATE_RECLAIMS,
            );
            new_pubkey
        })
        .take(num_pubkeys.saturating_sub(1))
        .collect();

        if num_pubkeys != 0 {
            pubkeys.push(Pubkey::default());
            index.upsert(
                root_slot,
                root_slot,
                &Pubkey::default(),
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                true,
                &mut vec![],
                UPSERT_POPULATE_RECLAIMS,
            );
        }

        index.add_root(root_slot);

        (index, pubkeys)
    }

    fn run_test_range(
        index: &AccountsIndex<bool>,
        pubkeys: &[Pubkey],
        start_bound: Bound<usize>,
        end_bound: Bound<usize>,
    ) {
        // Exclusive `index_start`
        let (pubkey_start, index_start) = match start_bound {
            Unbounded => (Unbounded, 0),
            Included(i) => (Included(pubkeys[i]), i),
            Excluded(i) => (Excluded(pubkeys[i]), i + 1),
        };

        // Exclusive `index_end`
        let (pubkey_end, index_end) = match end_bound {
            Unbounded => (Unbounded, pubkeys.len()),
            Included(i) => (Included(pubkeys[i]), i + 1),
            Excluded(i) => (Excluded(pubkeys[i]), i),
        };
        let pubkey_range = (pubkey_start, pubkey_end);

        let ancestors = Ancestors::default();
        let mut scanned_keys = HashSet::new();
        index.range_scan_accounts(
            "",
            &ancestors,
            pubkey_range,
            &ScanConfig::default(),
            |pubkey, _index| {
                scanned_keys.insert(*pubkey);
            },
        );

        let mut expected_len = 0;
        for key in &pubkeys[index_start..index_end] {
            expected_len += 1;
            assert!(scanned_keys.contains(key));
        }

        assert_eq!(scanned_keys.len(), expected_len);
    }

    fn run_test_range_indexes(
        index: &AccountsIndex<bool>,
        pubkeys: &[Pubkey],
        start: Option<usize>,
        end: Option<usize>,
    ) {
        let start_options = start
            .map(|i| vec![Included(i), Excluded(i)])
            .unwrap_or_else(|| vec![Unbounded]);
        let end_options = end
            .map(|i| vec![Included(i), Excluded(i)])
            .unwrap_or_else(|| vec![Unbounded]);

        for start in &start_options {
            for end in &end_options {
                run_test_range(index, pubkeys, *start, *end);
            }
        }
    }

    #[test]
    fn test_range_scan_accounts() {
        let (index, mut pubkeys) = setup_accounts_index_keys(3 * ITER_BATCH_SIZE);
        pubkeys.sort();

        run_test_range_indexes(&index, &pubkeys, None, None);

        run_test_range_indexes(&index, &pubkeys, Some(ITER_BATCH_SIZE), None);

        run_test_range_indexes(&index, &pubkeys, None, Some(2 * ITER_BATCH_SIZE));

        run_test_range_indexes(
            &index,
            &pubkeys,
            Some(ITER_BATCH_SIZE),
            Some(2 * ITER_BATCH_SIZE),
        );

        run_test_range_indexes(
            &index,
            &pubkeys,
            Some(ITER_BATCH_SIZE),
            Some(2 * ITER_BATCH_SIZE - 1),
        );

        run_test_range_indexes(
            &index,
            &pubkeys,
            Some(ITER_BATCH_SIZE - 1_usize),
            Some(2 * ITER_BATCH_SIZE + 1),
        );
    }

    fn run_test_scan_accounts(num_pubkeys: usize) {
        let (index, _) = setup_accounts_index_keys(num_pubkeys);
        let ancestors = Ancestors::default();

        let mut scanned_keys = HashSet::new();
        index.unchecked_scan_accounts(
            "",
            &ancestors,
            |pubkey, _index| {
                scanned_keys.insert(*pubkey);
            },
            &ScanConfig::default(),
        );
        assert_eq!(scanned_keys.len(), num_pubkeys);
    }

    #[test]
    fn test_scan_accounts() {
        run_test_scan_accounts(0);
        run_test_scan_accounts(1);
        run_test_scan_accounts(ITER_BATCH_SIZE * 10);
        run_test_scan_accounts(ITER_BATCH_SIZE * 10 - 1);
        run_test_scan_accounts(ITER_BATCH_SIZE * 10 + 1);
    }

    #[test]
    fn test_accounts_iter_finished() {
        let (index, _) = setup_accounts_index_keys(0);
        let mut iter = index.iter(None::<&Range<Pubkey>>, COLLECT_ALL_UNSORTED_FALSE);
        assert!(iter.next().is_none());
        let mut gc = vec![];
        index.upsert(
            0,
            0,
            &solana_sdk::pubkey::new_rand(),
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_is_alive_root() {
        let index = AccountsIndex::<bool>::default_for_tests();
        assert!(!index.is_alive_root(0));
        index.add_root(0);
        assert!(index.is_alive_root(0));
    }

    #[test]
    fn test_insert_with_root() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());

        index.add_root(0);
        let (list, idx) = index.get_for_tests(&key, None, None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
    }

    #[test]
    fn test_clean_first() {
        let index = AccountsIndex::<bool>::default_for_tests();
        index.add_root(0);
        index.add_root(1);
        index.clean_dead_slot(0, &mut AccountsIndexRootsStats::default());
        assert!(index.is_alive_root(1));
        assert!(!index.is_alive_root(0));
    }

    #[test]
    fn test_clean_last() {
        //this behavior might be undefined, clean up should only occur on older slots
        let index = AccountsIndex::<bool>::default_for_tests();
        index.add_root(0);
        index.add_root(1);
        index.clean_dead_slot(1, &mut AccountsIndexRootsStats::default());
        assert!(!index.is_alive_root(1));
        assert!(index.is_alive_root(0));
    }

    #[test]
    fn test_clean_and_unclean_slot() {
        let index = AccountsIndex::<bool>::default_for_tests();
        assert_eq!(0, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        index.add_root(0);
        index.add_root(1);
        index.add_uncleaned_roots([0, 1].into_iter());
        assert_eq!(2, index.roots_tracker.read().unwrap().uncleaned_roots.len());

        assert_eq!(
            0,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );
        index.reset_uncleaned_roots(None);
        assert_eq!(2, index.roots_tracker.read().unwrap().alive_roots.len());
        assert_eq!(0, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            2,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );

        index.add_root(2);
        index.add_root(3);
        index.add_uncleaned_roots([2, 3].into_iter());
        assert_eq!(4, index.roots_tracker.read().unwrap().alive_roots.len());
        assert_eq!(2, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            2,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );

        index.clean_dead_slot(1, &mut AccountsIndexRootsStats::default());
        assert_eq!(3, index.roots_tracker.read().unwrap().alive_roots.len());
        assert_eq!(2, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            1,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );

        index.clean_dead_slot(2, &mut AccountsIndexRootsStats::default());
        assert_eq!(2, index.roots_tracker.read().unwrap().alive_roots.len());
        assert_eq!(1, index.roots_tracker.read().unwrap().uncleaned_roots.len());
        assert_eq!(
            1,
            index
                .roots_tracker
                .read()
                .unwrap()
                .previous_uncleaned_roots
                .len()
        );
    }

    #[test]
    fn test_update_last_wins() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());
        let (list, idx) = index.get_for_tests(&key, Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
        drop(list);

        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            false,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert_eq!(gc, vec![(0, true)]);
        let (list, idx) = index.get_for_tests(&key, Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, false));
    }

    #[test]
    fn test_update_new_slot() {
        solana_logger::setup();
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let ancestors = vec![(0, 0)].into_iter().collect();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());
        index.upsert(
            1,
            1,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            false,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());
        let (list, idx) = index.get_for_tests(&key, Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (0, true));
        let ancestors = vec![(1, 0)].into_iter().collect();
        let (list, idx) = index.get_for_tests(&key, Some(&ancestors), None).unwrap();
        assert_eq!(list.slot_list()[idx], (1, false));
    }

    #[test]
    fn test_update_gc_purged_slot() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let mut gc = Vec::new();
        index.upsert(
            0,
            0,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(gc.is_empty());
        index.upsert(
            1,
            1,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            false,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        index.upsert(
            2,
            2,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        index.upsert(
            3,
            3,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        index.add_root(0);
        index.add_root(1);
        index.add_root(3);
        index.upsert(
            4,
            4,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            true,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );

        // Updating index should not purge older roots, only purges
        // previous updates within the same slot
        assert_eq!(gc, vec![]);
        let (list, idx) = index.get_for_tests(&key, None, None).unwrap();
        assert_eq!(list.slot_list()[idx], (3, true));

        let mut num = 0;
        let mut found_key = false;
        index.unchecked_scan_accounts(
            "",
            &Ancestors::default(),
            |pubkey, _index| {
                if pubkey == &key {
                    found_key = true;
                    assert_eq!(_index, (&true, 3));
                };
                num += 1
            },
            &ScanConfig::default(),
        );
        assert_eq!(num, 1);
        assert!(found_key);
    }

    fn account_maps_stats_len<T: IndexValue>(index: &AccountsIndex<T>) -> usize {
        index.storage.storage.stats.total_count()
    }

    #[test]
    fn test_purge() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<u64>::default_for_tests();
        let mut gc = Vec::new();
        assert_eq!(0, account_maps_stats_len(&index));
        index.upsert(
            1,
            1,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            12,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert_eq!(1, account_maps_stats_len(&index));

        index.upsert(
            1,
            1,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            10,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert_eq!(1, account_maps_stats_len(&index));

        let purges = index.purge_roots(&key);
        assert_eq!(purges, (vec![], false));
        index.add_root(1);

        let purges = index.purge_roots(&key);
        assert_eq!(purges, (vec![(1, 10)], true));

        assert_eq!(1, account_maps_stats_len(&index));
        index.upsert(
            1,
            1,
            &key,
            &AccountSharedData::default(),
            &AccountSecondaryIndexes::default(),
            9,
            &mut gc,
            UPSERT_POPULATE_RECLAIMS,
        );
        assert_eq!(1, account_maps_stats_len(&index));
    }

    #[test]
    fn test_latest_slot() {
        let slot_slice = vec![(0, true), (5, true), (3, true), (7, true)];
        let index = AccountsIndex::<bool>::default_for_tests();

        // No ancestors, no root, should return None
        assert!(index.latest_slot(None, &slot_slice, None).is_none());

        // Given a root, should return the root
        index.add_root(5);
        assert_eq!(index.latest_slot(None, &slot_slice, None).unwrap(), 1);

        // Given a max_root == root, should still return the root
        assert_eq!(index.latest_slot(None, &slot_slice, Some(5)).unwrap(), 1);

        // Given a max_root < root, should filter out the root
        assert!(index.latest_slot(None, &slot_slice, Some(4)).is_none());

        // Given a max_root, should filter out roots < max_root, but specified
        // ancestors should not be affected
        let ancestors = vec![(3, 1), (7, 1)].into_iter().collect();
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(4))
                .unwrap(),
            3
        );
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, Some(7))
                .unwrap(),
            3
        );

        // Given no max_root, should just return the greatest ancestor or root
        assert_eq!(
            index
                .latest_slot(Some(&ancestors), &slot_slice, None)
                .unwrap(),
            3
        );
    }

    fn run_test_purge_exact_secondary_index<
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        index: &AccountsIndex<bool>,
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        key_start: usize,
        key_end: usize,
        secondary_indexes: &AccountSecondaryIndexes,
    ) {
        // No roots, should be no reclaims
        let slots = vec![1, 2, 5, 9];
        let index_key = Pubkey::new_unique();
        let account_key = Pubkey::new_unique();

        let mut account_data = vec![0; inline_spl_token::Account::get_packed_len()];
        account_data[key_start..key_end].clone_from_slice(&(index_key.to_bytes()));

        // Insert slots into secondary index
        for slot in &slots {
            index.upsert(
                *slot,
                *slot,
                &account_key,
                // Make sure these accounts are added to secondary index
                &AccountSharedData::create(
                    0,
                    account_data.to_vec(),
                    inline_spl_token::id(),
                    false,
                    0,
                ),
                secondary_indexes,
                true,
                &mut vec![],
                UPSERT_POPULATE_RECLAIMS,
            );
        }

        // Only one top level index entry exists
        assert_eq!(secondary_index.index.get(&index_key).unwrap().len(), 1);

        // In the reverse index, one account maps across multiple slots
        // to the same top level key
        assert_eq!(
            secondary_index
                .reverse_index
                .get(&account_key)
                .unwrap()
                .value()
                .read()
                .unwrap()
                .len(),
            1
        );

        index.purge_exact(
            &account_key,
            &slots.into_iter().collect::<HashSet<Slot>>(),
            &mut vec![],
        );

        let _ = index.handle_dead_keys(&[&account_key], secondary_indexes);
        assert!(secondary_index.index.is_empty());
        assert!(secondary_index.reverse_index.is_empty());
    }

    #[test]
    fn test_purge_exact_dashmap_secondary_index() {
        let (key_start, key_end, secondary_indexes) = create_dashmap_secondary_index_state();
        let index = AccountsIndex::<bool>::default_for_tests();
        run_test_purge_exact_secondary_index(
            &index,
            &index.spl_token_mint_index,
            key_start,
            key_end,
            &secondary_indexes,
        );
    }

    #[test]
    fn test_purge_exact_rwlock_secondary_index() {
        let (key_start, key_end, secondary_indexes) = create_rwlock_secondary_index_state();
        let index = AccountsIndex::<bool>::default_for_tests();
        run_test_purge_exact_secondary_index(
            &index,
            &index.spl_token_owner_index,
            key_start,
            key_end,
            &secondary_indexes,
        );
    }

    #[test]
    fn test_purge_older_root_entries() {
        // No roots, should be no reclaims
        let index = AccountsIndex::<bool>::default_for_tests();
        let mut slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        let mut reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, None);
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Add a later root, earlier slots should be reclaimed
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        index.add_root(1);
        // Note 2 is not a root
        index.add_root(5);
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, None);
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Add a later root that is not in the list, should not affect the outcome
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        index.add_root(6);
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, None);
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root >= than any root in the slot list, should not affect
        // outcome
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(6));
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root, earlier slots should be reclaimed
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(5));
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);

        // Pass a max root 2. This means the latest root < 2 is 1 because 2 is not a root
        // so nothing will be purged
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(2));
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Pass a max root 1. This means the latest root < 3 is 1 because 2 is not a root
        // so nothing will be purged
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(1));
        assert!(reclaims.is_empty());
        assert_eq!(slot_list, vec![(1, true), (2, true), (5, true), (9, true)]);

        // Pass a max root that doesn't exist in the list but is greater than
        // some of the roots in the list, shouldn't return those smaller roots
        slot_list = vec![(1, true), (2, true), (5, true), (9, true)];
        reclaims = vec![];
        index.purge_older_root_entries(&mut slot_list, &mut reclaims, Some(7));
        assert_eq!(reclaims, vec![(1, true), (2, true)]);
        assert_eq!(slot_list, vec![(5, true), (9, true)]);
    }

    fn check_secondary_index_mapping_correct<SecondaryIndexEntryType>(
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        secondary_index_keys: &[Pubkey],
        account_key: &Pubkey,
    ) where
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    {
        // Check secondary index has unique mapping from secondary index key
        // to the account key and slot
        for secondary_index_key in secondary_index_keys {
            assert_eq!(secondary_index.index.len(), secondary_index_keys.len());
            let account_key_map = secondary_index.get(secondary_index_key);
            assert_eq!(account_key_map.len(), 1);
            assert_eq!(account_key_map, vec![*account_key]);
        }
        // Check reverse index contains all of the `secondary_index_keys`
        let secondary_index_key_map = secondary_index.reverse_index.get(account_key).unwrap();
        assert_eq!(
            &*secondary_index_key_map.value().read().unwrap(),
            secondary_index_keys
        );
    }

    fn run_test_spl_token_secondary_indexes<
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        token_id: &Pubkey,
        index: &AccountsIndex<bool>,
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        key_start: usize,
        key_end: usize,
        secondary_indexes: &AccountSecondaryIndexes,
    ) {
        let mut secondary_indexes = secondary_indexes.clone();
        let account_key = Pubkey::new_unique();
        let index_key = Pubkey::new_unique();
        let mut account_data = vec![0; inline_spl_token::Account::get_packed_len()];
        account_data[key_start..key_end].clone_from_slice(&(index_key.to_bytes()));

        // Wrong program id
        index.upsert(
            0,
            0,
            &account_key,
            &AccountSharedData::create(0, account_data.to_vec(), Pubkey::default(), false, 0),
            &secondary_indexes,
            true,
            &mut vec![],
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(secondary_index.index.is_empty());
        assert!(secondary_index.reverse_index.is_empty());

        // Wrong account data size
        index.upsert(
            0,
            0,
            &account_key,
            &AccountSharedData::create(0, account_data[1..].to_vec(), *token_id, false, 0),
            &secondary_indexes,
            true,
            &mut vec![],
            UPSERT_POPULATE_RECLAIMS,
        );
        assert!(secondary_index.index.is_empty());
        assert!(secondary_index.reverse_index.is_empty());

        secondary_indexes.keys = None;

        // Just right. Inserting the same index multiple times should be ok
        for _ in 0..2 {
            index.update_secondary_indexes(
                &account_key,
                &AccountSharedData::create(0, account_data.to_vec(), *token_id, false, 0),
                &secondary_indexes,
            );
            check_secondary_index_mapping_correct(secondary_index, &[index_key], &account_key);
        }

        // included
        assert!(!secondary_index.index.is_empty());
        assert!(!secondary_index.reverse_index.is_empty());

        secondary_indexes.keys = Some(AccountSecondaryIndexesIncludeExclude {
            keys: [index_key].iter().cloned().collect::<HashSet<_>>(),
            exclude: false,
        });
        secondary_index.index.clear();
        secondary_index.reverse_index.clear();
        index.update_secondary_indexes(
            &account_key,
            &AccountSharedData::create(0, account_data.to_vec(), *token_id, false, 0),
            &secondary_indexes,
        );
        assert!(!secondary_index.index.is_empty());
        assert!(!secondary_index.reverse_index.is_empty());
        check_secondary_index_mapping_correct(secondary_index, &[index_key], &account_key);

        // not-excluded
        secondary_indexes.keys = Some(AccountSecondaryIndexesIncludeExclude {
            keys: [].iter().cloned().collect::<HashSet<_>>(),
            exclude: true,
        });
        secondary_index.index.clear();
        secondary_index.reverse_index.clear();
        index.update_secondary_indexes(
            &account_key,
            &AccountSharedData::create(0, account_data.to_vec(), *token_id, false, 0),
            &secondary_indexes,
        );
        assert!(!secondary_index.index.is_empty());
        assert!(!secondary_index.reverse_index.is_empty());
        check_secondary_index_mapping_correct(secondary_index, &[index_key], &account_key);

        secondary_indexes.keys = None;

        index.slot_list_mut(&account_key, |slot_list| slot_list.clear());

        // Everything should be deleted
        let _ = index.handle_dead_keys(&[&account_key], &secondary_indexes);
        assert!(secondary_index.index.is_empty());
        assert!(secondary_index.reverse_index.is_empty());
    }

    #[test]
    fn test_dashmap_secondary_index() {
        let (key_start, key_end, secondary_indexes) = create_dashmap_secondary_index_state();
        let index = AccountsIndex::<bool>::default_for_tests();
        for token_id in [inline_spl_token::id(), inline_spl_token_2022::id()] {
            run_test_spl_token_secondary_indexes(
                &token_id,
                &index,
                &index.spl_token_mint_index,
                key_start,
                key_end,
                &secondary_indexes,
            );
        }
    }

    #[test]
    fn test_rwlock_secondary_index() {
        let (key_start, key_end, secondary_indexes) = create_rwlock_secondary_index_state();
        let index = AccountsIndex::<bool>::default_for_tests();
        for token_id in [inline_spl_token::id(), inline_spl_token_2022::id()] {
            run_test_spl_token_secondary_indexes(
                &token_id,
                &index,
                &index.spl_token_owner_index,
                key_start,
                key_end,
                &secondary_indexes,
            );
        }
    }

    fn run_test_secondary_indexes_same_slot_and_forks<
        SecondaryIndexEntryType: SecondaryIndexEntry + Default + Sync + Send,
    >(
        token_id: &Pubkey,
        index: &AccountsIndex<bool>,
        secondary_index: &SecondaryIndex<SecondaryIndexEntryType>,
        index_key_start: usize,
        index_key_end: usize,
        secondary_indexes: &AccountSecondaryIndexes,
    ) {
        let account_key = Pubkey::new_unique();
        let secondary_key1 = Pubkey::new_unique();
        let secondary_key2 = Pubkey::new_unique();
        let slot = 1;
        let mut account_data1 = vec![0; inline_spl_token::Account::get_packed_len()];
        account_data1[index_key_start..index_key_end]
            .clone_from_slice(&(secondary_key1.to_bytes()));
        let mut account_data2 = vec![0; inline_spl_token::Account::get_packed_len()];
        account_data2[index_key_start..index_key_end]
            .clone_from_slice(&(secondary_key2.to_bytes()));

        // First write one mint index
        index.upsert(
            slot,
            slot,
            &account_key,
            &AccountSharedData::create(0, account_data1.to_vec(), *token_id, false, 0),
            secondary_indexes,
            true,
            &mut vec![],
            UPSERT_POPULATE_RECLAIMS,
        );

        // Now write a different mint index for the same account
        index.upsert(
            slot,
            slot,
            &account_key,
            &AccountSharedData::create(0, account_data2.to_vec(), *token_id, false, 0),
            secondary_indexes,
            true,
            &mut vec![],
            UPSERT_POPULATE_RECLAIMS,
        );

        // Both pubkeys will now be present in the index
        check_secondary_index_mapping_correct(
            secondary_index,
            &[secondary_key1, secondary_key2],
            &account_key,
        );

        // If a later slot also introduces secondary_key1, then it should still exist in the index
        let later_slot = slot + 1;
        index.upsert(
            later_slot,
            later_slot,
            &account_key,
            &AccountSharedData::create(0, account_data1.to_vec(), *token_id, false, 0),
            secondary_indexes,
            true,
            &mut vec![],
            UPSERT_POPULATE_RECLAIMS,
        );
        assert_eq!(secondary_index.get(&secondary_key1), vec![account_key]);

        // If we set a root at `later_slot`, and clean, then even though the account with secondary_key1
        // was outdated by the update in the later slot, the primary account key is still alive,
        // so both secondary keys will still be kept alive.
        index.add_root(later_slot);
        index.slot_list_mut(&account_key, |slot_list| {
            index.purge_older_root_entries(slot_list, &mut vec![], None)
        });

        check_secondary_index_mapping_correct(
            secondary_index,
            &[secondary_key1, secondary_key2],
            &account_key,
        );

        // Removing the remaining entry for this pubkey in the index should mark the
        // pubkey as dead and finally remove all the secondary indexes
        let mut reclaims = vec![];
        index.purge_exact(&account_key, &later_slot, &mut reclaims);
        let _ = index.handle_dead_keys(&[&account_key], secondary_indexes);
        assert!(secondary_index.index.is_empty());
        assert!(secondary_index.reverse_index.is_empty());
    }

    #[test]
    fn test_dashmap_secondary_index_same_slot_and_forks() {
        let (key_start, key_end, account_index) = create_dashmap_secondary_index_state();
        let index = AccountsIndex::<bool>::default_for_tests();
        for token_id in [inline_spl_token::id(), inline_spl_token_2022::id()] {
            run_test_secondary_indexes_same_slot_and_forks(
                &token_id,
                &index,
                &index.spl_token_mint_index,
                key_start,
                key_end,
                &account_index,
            );
        }
    }

    #[test]
    fn test_rwlock_secondary_index_same_slot_and_forks() {
        let (key_start, key_end, account_index) = create_rwlock_secondary_index_state();
        let index = AccountsIndex::<bool>::default_for_tests();
        for token_id in [inline_spl_token::id(), inline_spl_token_2022::id()] {
            run_test_secondary_indexes_same_slot_and_forks(
                &token_id,
                &index,
                &index.spl_token_owner_index,
                key_start,
                key_end,
                &account_index,
            );
        }
    }

    impl IndexValue for bool {}
    impl IndexValue for u64 {}
    impl IsCached for bool {
        fn is_cached(&self) -> bool {
            false
        }
    }
    impl IsCached for u64 {
        fn is_cached(&self) -> bool {
            false
        }
    }
    impl ZeroLamport for bool {
        fn is_zero_lamport(&self) -> bool {
            false
        }
    }

    impl ZeroLamport for u64 {
        fn is_zero_lamport(&self) -> bool {
            false
        }
    }

    #[test]
    fn test_bin_start_and_range() {
        let index = AccountsIndex::<bool>::default_for_tests();
        let iter = AccountsIndexIterator::new(
            &index,
            None::<&RangeInclusive<Pubkey>>,
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!((0, usize::MAX), iter.bin_start_and_range());

        let key_0 = Pubkey::new(&[0; 32]);
        let key_ff = Pubkey::new(&[0xff; 32]);

        let iter = AccountsIndexIterator::new(
            &index,
            Some(&RangeInclusive::new(key_0, key_ff)),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        let bins = index.bins();
        assert_eq!((0, bins), iter.bin_start_and_range());
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&RangeInclusive::new(key_ff, key_0)),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!((bins - 1, 0), iter.bin_start_and_range());
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&(Included(key_0), Unbounded)),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!((0, usize::MAX), iter.bin_start_and_range());
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&(Included(key_ff), Unbounded)),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!((bins - 1, usize::MAX), iter.bin_start_and_range());

        assert_eq!((0..2).skip(1).take(usize::MAX).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn test_get_newest_root_in_slot_list() {
        let index = AccountsIndex::<bool>::default_for_tests();
        let return_0 = 0;
        let slot1 = 1;
        let slot2 = 2;
        let slot99 = 99;

        // no roots, so always 0
        {
            let roots_tracker = &index.roots_tracker.read().unwrap();
            let slot_list = Vec::<(Slot, bool)>::default();
            assert_eq!(
                return_0,
                AccountsIndex::get_newest_root_in_slot_list(
                    &roots_tracker.alive_roots,
                    &slot_list,
                    Some(slot1),
                )
            );
            assert_eq!(
                return_0,
                AccountsIndex::get_newest_root_in_slot_list(
                    &roots_tracker.alive_roots,
                    &slot_list,
                    Some(slot2),
                )
            );
            assert_eq!(
                return_0,
                AccountsIndex::get_newest_root_in_slot_list(
                    &roots_tracker.alive_roots,
                    &slot_list,
                    Some(slot99),
                )
            );
        }

        index.add_root(slot2);

        {
            let roots_tracker = &index.roots_tracker.read().unwrap();
            let slot_list = vec![(slot2, true)];
            assert_eq!(
                slot2,
                AccountsIndex::get_newest_root_in_slot_list(
                    &roots_tracker.alive_roots,
                    &slot_list,
                    Some(slot2),
                )
            );
            // no newest root
            assert_eq!(
                return_0,
                AccountsIndex::get_newest_root_in_slot_list(
                    &roots_tracker.alive_roots,
                    &slot_list,
                    Some(slot1),
                )
            );
            assert_eq!(
                slot2,
                AccountsIndex::get_newest_root_in_slot_list(
                    &roots_tracker.alive_roots,
                    &slot_list,
                    Some(slot99),
                )
            );
        }
    }

    impl<T: IndexValue> AccountsIndex<T> {
        fn upsert_simple_test(&self, key: &Pubkey, slot: Slot, value: T) {
            let mut gc = Vec::new();
            self.upsert(
                slot,
                slot,
                key,
                &AccountSharedData::default(),
                &AccountSecondaryIndexes::default(),
                value,
                &mut gc,
                UPSERT_POPULATE_RECLAIMS,
            );
            assert!(gc.is_empty());
        }
    }

    #[test]
    fn test_unref() {
        let value = true;
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let slot1 = 1;

        index.upsert_simple_test(&key, slot1, value);

        let map = index.get_bin(&key);
        for expected in [false, true] {
            assert!(map.get_internal(&key, |entry| {
                // check refcount BEFORE the unref
                assert_eq!(u64::from(!expected), entry.unwrap().ref_count());
                // first time, ref count was at 1, we can unref once. Unref should return false.
                // second time, ref count was at 0, it is an error to unref. Unref should return true
                assert_eq!(expected, entry.unwrap().unref());
                // check refcount AFTER the unref
                assert_eq!(
                    if expected {
                        (0 as RefCount).wrapping_sub(1)
                    } else {
                        0
                    },
                    entry.unwrap().ref_count()
                );
                (false, true)
            }));
        }
    }

    #[test]
    fn test_clean_rooted_entries_return() {
        solana_logger::setup();
        let value = true;
        let key = solana_sdk::pubkey::new_rand();
        let key_unknown = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();
        let slot1 = 1;

        let mut gc = Vec::new();
        // return true if we don't know anything about 'key_unknown'
        // the item did not exist in the accounts index at all, so index is up to date
        assert!(index.clean_rooted_entries(&key_unknown, &mut gc, None));

        index.upsert_simple_test(&key, slot1, value);

        let slot2 = 2;
        // none for max root because we don't want to delete the entry yet
        assert!(!index.clean_rooted_entries(&key, &mut gc, None));
        // this is because of inclusive vs exclusive in the call to can_purge_older_entries
        assert!(!index.clean_rooted_entries(&key, &mut gc, Some(slot1)));
        // this will delete the entry because it is <= max_root_inclusive and NOT a root
        // note this has to be slot2 because of inclusive vs exclusive in the call to can_purge_older_entries
        {
            let mut gc = Vec::new();
            assert!(index.clean_rooted_entries(&key, &mut gc, Some(slot2)));
            assert_eq!(gc, vec![(slot1, value)]);
        }

        // re-add it
        index.upsert_simple_test(&key, slot1, value);

        index.add_root(slot1);
        assert!(!index.clean_rooted_entries(&key, &mut gc, Some(slot2)));
        index.upsert_simple_test(&key, slot2, value);

        assert_eq!(
            2,
            index
                .get_account_read_entry(&key)
                .unwrap()
                .slot_list()
                .len()
        );
        assert_eq!(
            &vec![(slot1, value), (slot2, value)],
            index.get_account_read_entry(&key).unwrap().slot_list()
        );
        assert!(!index.clean_rooted_entries(&key, &mut gc, Some(slot2)));
        assert_eq!(
            2,
            index
                .get_account_read_entry(&key)
                .unwrap()
                .slot_list()
                .len()
        );
        assert!(gc.is_empty());
        {
            {
                let roots_tracker = &index.roots_tracker.read().unwrap();
                let slot_list = vec![(slot2, value)];
                assert_eq!(
                    0,
                    AccountsIndex::get_newest_root_in_slot_list(
                        &roots_tracker.alive_roots,
                        &slot_list,
                        None,
                    )
                );
            }
            index.add_root(slot2);
            {
                let roots_tracker = &index.roots_tracker.read().unwrap();
                let slot_list = vec![(slot2, value)];
                assert_eq!(
                    slot2,
                    AccountsIndex::get_newest_root_in_slot_list(
                        &roots_tracker.alive_roots,
                        &slot_list,
                        None,
                    )
                );
                assert_eq!(
                    0,
                    AccountsIndex::get_newest_root_in_slot_list(
                        &roots_tracker.alive_roots,
                        &slot_list,
                        Some(0),
                    )
                );
            }
        }

        assert!(gc.is_empty());
        assert!(!index.clean_rooted_entries(&key, &mut gc, Some(slot2)));
        assert_eq!(gc, vec![(slot1, value)]);
        gc.clear();
        index.clean_dead_slot(slot2, &mut AccountsIndexRootsStats::default());
        let slot3 = 3;
        assert!(index.clean_rooted_entries(&key, &mut gc, Some(slot3)));
        assert_eq!(gc, vec![(slot2, value)]);
    }

    #[test]
    fn test_handle_dead_keys_return() {
        let key = solana_sdk::pubkey::new_rand();
        let index = AccountsIndex::<bool>::default_for_tests();

        assert_eq!(
            index.handle_dead_keys(&[&key], &AccountSecondaryIndexes::default()),
            vec![key].into_iter().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn test_start_end_bin() {
        let index = AccountsIndex::<bool>::default_for_tests();
        assert_eq!(index.bins(), BINS_FOR_TESTING);
        let iter = AccountsIndexIterator::new(
            &index,
            None::<&RangeInclusive<Pubkey>>,
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!(iter.start_bin(), 0); // no range, so 0
        assert_eq!(iter.end_bin_inclusive(), usize::MAX); // no range, so max

        let key = Pubkey::new(&[0; 32]);
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&RangeInclusive::new(key, key)),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!(iter.start_bin(), 0); // start at pubkey 0, so 0
        assert_eq!(iter.end_bin_inclusive(), 0); // end at pubkey 0, so 0
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&(Included(key), Excluded(key))),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!(iter.start_bin(), 0); // start at pubkey 0, so 0
        assert_eq!(iter.end_bin_inclusive(), 0); // end at pubkey 0, so 0
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&(Excluded(key), Excluded(key))),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!(iter.start_bin(), 0); // start at pubkey 0, so 0
        assert_eq!(iter.end_bin_inclusive(), 0); // end at pubkey 0, so 0

        let key = Pubkey::new(&[0xff; 32]);
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&RangeInclusive::new(key, key)),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        let bins = index.bins();
        assert_eq!(iter.start_bin(), bins - 1); // start at highest possible pubkey, so bins - 1
        assert_eq!(iter.end_bin_inclusive(), bins - 1);
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&(Included(key), Excluded(key))),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!(iter.start_bin(), bins - 1); // start at highest possible pubkey, so bins - 1
        assert_eq!(iter.end_bin_inclusive(), bins - 1);
        let iter = AccountsIndexIterator::new(
            &index,
            Some(&(Excluded(key), Excluded(key))),
            COLLECT_ALL_UNSORTED_FALSE,
        );
        assert_eq!(iter.start_bin(), bins - 1); // start at highest possible pubkey, so bins - 1
        assert_eq!(iter.end_bin_inclusive(), bins - 1);
    }

    #[test]
    #[should_panic(expected = "bins.is_power_of_two()")]
    #[allow(clippy::field_reassign_with_default)]
    fn test_illegal_bins() {
        let mut config = AccountsIndexConfig::default();
        config.bins = Some(3);
        AccountsIndex::<bool>::new(Some(config), &Arc::default());
    }

    #[test]
    fn test_scan_config() {
        for collect_all_unsorted in [false, true] {
            let config = ScanConfig::new(collect_all_unsorted);
            assert_eq!(config.collect_all_unsorted, collect_all_unsorted);
            assert!(config.abort.is_none()); // not allocated
            assert!(!config.is_aborted());
            config.abort(); // has no effect
            assert!(!config.is_aborted());
        }

        let config = ScanConfig::default();
        assert!(!config.collect_all_unsorted);
        assert!(config.abort.is_none());

        let config = config.recreate_with_abort();
        assert!(config.abort.is_some());
        assert!(!config.is_aborted());
        config.abort();
        assert!(config.is_aborted());

        let config = config.recreate_with_abort();
        assert!(config.is_aborted());
    }
}
