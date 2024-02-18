use {
    crate::{bank::Bank, compute_budget_details::GetComputeBudgetDetails, prioritization_fee::*},
    crossbeam_channel::{unbounded, Receiver, Sender},
    dashmap::DashMap,
    log::*,
    solana_measure::measure,
    solana_sdk::{
        clock::{BankId, Slot},
        pubkey::Pubkey,
        transaction::SanitizedTransaction,
    },
    std::{
        collections::{BTreeMap, HashMap},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::{Builder, JoinHandle},
    },
};

/// The maximum number of blocks to keep in `PrioritizationFeeCache`, ie.
/// the amount of history generally desired to estimate the prioritization fee needed to
/// land a transaction in the current block.
const MAX_NUM_RECENT_BLOCKS: u64 = 150;

#[derive(Debug, Default)]
struct PrioritizationFeeCacheMetrics {
    // Count of transactions that successfully updated each slot's prioritization fee cache.
    successful_transaction_update_count: AtomicU64,

    // Count of duplicated banks being purged
    purged_duplicated_bank_count: AtomicU64,

    // Accumulated time spent on tracking prioritization fee for each slot.
    total_update_elapsed_us: AtomicU64,

    // Accumulated time spent on acquiring cache write lock.
    total_cache_lock_elapsed_us: AtomicU64,

    // Accumulated time spent on updating block prioritization fees.
    total_entry_update_elapsed_us: AtomicU64,

    // Accumulated time spent on finalizing block prioritization fees.
    total_block_finalize_elapsed_us: AtomicU64,
}

impl PrioritizationFeeCacheMetrics {
    fn accumulate_successful_transaction_update_count(&self, val: u64) {
        self.successful_transaction_update_count
            .fetch_add(val, Ordering::Relaxed);
    }

    fn accumulate_total_purged_duplicated_bank_count(&self, val: u64) {
        self.purged_duplicated_bank_count
            .fetch_add(val, Ordering::Relaxed);
    }

    fn accumulate_total_update_elapsed_us(&self, val: u64) {
        self.total_update_elapsed_us
            .fetch_add(val, Ordering::Relaxed);
    }

    fn accumulate_total_cache_lock_elapsed_us(&self, val: u64) {
        self.total_cache_lock_elapsed_us
            .fetch_add(val, Ordering::Relaxed);
    }

    fn accumulate_total_entry_update_elapsed_us(&self, val: u64) {
        self.total_entry_update_elapsed_us
            .fetch_add(val, Ordering::Relaxed);
    }

    fn accumulate_total_block_finalize_elapsed_us(&self, val: u64) {
        self.total_block_finalize_elapsed_us
            .fetch_add(val, Ordering::Relaxed);
    }

    fn report(&self, slot: Slot) {
        datapoint_info!(
            "block_prioritization_fee_counters",
            ("slot", slot as i64, i64),
            (
                "successful_transaction_update_count",
                self.successful_transaction_update_count
                    .swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "purged_duplicated_bank_count",
                self.purged_duplicated_bank_count.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "total_update_elapsed_us",
                self.total_update_elapsed_us.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "total_cache_lock_elapsed_us",
                self.total_cache_lock_elapsed_us.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "total_entry_update_elapsed_us",
                self.total_entry_update_elapsed_us
                    .swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "total_block_finalize_elapsed_us",
                self.total_block_finalize_elapsed_us
                    .swap(0, Ordering::Relaxed) as i64,
                i64
            ),
        );
    }
}

#[derive(Debug)]
enum CacheServiceUpdate {
    TransactionUpdate {
        slot: Slot,
        bank_id: BankId,
        transaction_fee: u64,
        writable_accounts: Vec<Pubkey>,
    },
    BankFinalized {
        slot: Slot,
        bank_id: BankId,
    },
    Exit,
}

/// Potentially there are more than one bank that updates Prioritization Fee
/// for a slot. The updates are tracked and finalized by bank_id.
type SlotPrioritizationFee = DashMap<BankId, PrioritizationFee>;

/// Stores up to MAX_NUM_RECENT_BLOCKS recent block's prioritization fee,
/// A separate internal thread `service_thread` handles additional tasks when a bank is frozen,
/// and collecting stats and reporting metrics.
#[derive(Debug)]
pub struct PrioritizationFeeCache {
    cache: Arc<RwLock<BTreeMap<Slot, SlotPrioritizationFee>>>,
    service_thread: Option<JoinHandle<()>>,
    sender: Sender<CacheServiceUpdate>,
    metrics: Arc<PrioritizationFeeCacheMetrics>,
}

impl Default for PrioritizationFeeCache {
    fn default() -> Self {
        Self::new(MAX_NUM_RECENT_BLOCKS)
    }
}

impl Drop for PrioritizationFeeCache {
    fn drop(&mut self) {
        let _ = self.sender.send(CacheServiceUpdate::Exit);
        self.service_thread
            .take()
            .unwrap()
            .join()
            .expect("Prioritization fee cache servicing thread failed to join");
    }
}

impl PrioritizationFeeCache {
    pub fn new(capacity: u64) -> Self {
        let cache = Arc::new(RwLock::new(BTreeMap::new()));
        let (sender, receiver) = unbounded();
        let metrics = Arc::new(PrioritizationFeeCacheMetrics::default());

        let service_thread = Some(
            Builder::new()
                .name("solPrFeeCachSvc".to_string())
                .spawn({
                    let cache = cache.clone();
                    let metrics = metrics.clone();
                    move || Self::service_loop(cache, capacity as usize, receiver, metrics)
                })
                .unwrap(),
        );

        PrioritizationFeeCache {
            cache,
            service_thread,
            sender,
            metrics,
        }
    }

    /// Update with a list of non-vote transactions' compute_budget_details and account_locks; Only
    /// transactions have both valid compute_budget_details and account_locks will be used to update
    /// fee_cache asynchronously.
    pub fn update<'a>(&self, bank: &Bank, txs: impl Iterator<Item = &'a SanitizedTransaction>) {
        let (_, send_updates_time) = measure!(
            {
                for sanitized_transaction in txs {
                    // Vote transactions are not prioritized, therefore they are excluded from
                    // updating fee_cache.
                    if sanitized_transaction.is_simple_vote_transaction() {
                        continue;
                    }

                    let round_compute_unit_price_enabled = false; // TODO: bank.feture_set.is_active(round_compute_unit_price)
                    let compute_budget_details = sanitized_transaction
                        .get_compute_budget_details(round_compute_unit_price_enabled);
                    let account_locks = sanitized_transaction
                        .get_account_locks(bank.get_transaction_account_lock_limit());

                    if compute_budget_details.is_none() || account_locks.is_err() {
                        continue;
                    }
                    let compute_budget_details = compute_budget_details.unwrap();

                    // filter out any transaction that requests zero compute_unit_limit
                    // since its priority fee amount is not instructive
                    if compute_budget_details.compute_unit_limit == 0 {
                        continue;
                    }

                    let writable_accounts = account_locks
                        .unwrap()
                        .writable
                        .iter()
                        .map(|key| **key)
                        .collect::<Vec<_>>();

                    self.sender
                        .send(CacheServiceUpdate::TransactionUpdate {
                            slot: bank.slot(),
                            bank_id: bank.bank_id(),
                            transaction_fee: compute_budget_details.compute_unit_price,
                            writable_accounts,
                        })
                        .unwrap_or_else(|err| {
                            warn!(
                                "prioritization fee cache transaction updates failed: {:?}",
                                err
                            );
                        });
                }
            },
            "send_updates",
        );

        self.metrics
            .accumulate_total_update_elapsed_us(send_updates_time.as_us());
    }

    /// Finalize prioritization fee when it's bank is completely replayed from blockstore,
    /// by pruning irrelevant accounts to save space, and marking its availability for queries.
    pub fn finalize_priority_fee(&self, slot: Slot, bank_id: BankId) {
        self.sender
            .send(CacheServiceUpdate::BankFinalized { slot, bank_id })
            .unwrap_or_else(|err| {
                warn!(
                    "prioritization fee cache signalling bank frozen failed: {:?}",
                    err
                )
            });
    }

    /// Internal function is invoked by worker thread to update slot's minimum prioritization fee.
    fn update_cache(
        unfinalized: &mut BTreeMap<Slot, SlotPrioritizationFee>,
        slot: Slot,
        bank_id: BankId,
        transaction_fee: u64,
        writable_accounts: &[Pubkey],
        metrics: &PrioritizationFeeCacheMetrics,
    ) {
        let (_, entry_update_time) = measure!(
            {
                let mut block_prioritization_fee = unfinalized
                    .entry(slot)
                    .or_default()
                    .entry(bank_id)
                    .or_insert(PrioritizationFee::default());
                block_prioritization_fee.update(transaction_fee, writable_accounts)
            },
            "entry_update_time"
        );
        metrics.accumulate_total_entry_update_elapsed_us(entry_update_time.as_us());
        metrics.accumulate_successful_transaction_update_count(1);
    }

    fn finalize_slot(
        unfinalized: &mut BTreeMap<Slot, SlotPrioritizationFee>,
        cache: &RwLock<BTreeMap<Slot, SlotPrioritizationFee>>,
        cache_max_size: usize,
        slot: Slot,
        bank_id: BankId,
        metrics: &PrioritizationFeeCacheMetrics,
    ) {
        // remove unfinalized slots
        // TODO: do we need to keep slots in buffer? or they always come in order?
        loop {
            match unfinalized.keys().next().cloned() {
                Some(unfinalized_slot) if unfinalized_slot < slot - 128 => unfinalized.pop_first(),
                _ => break,
            };
        }

        let slot_prioritization_fee = match unfinalized.remove(&slot) {
            Some(slot_prioritization_fee) => slot_prioritization_fee,
            None => return,
        };

        // prune cache by evicting write account entry from prioritization fee if its fee is less
        // or equal to block's minimum transaction fee, because they are irrelevant in calculating
        // block minimum fee.
        let (_, slot_finalize_time) = measure!(
            {
                // Only retain priority fee reported from optimistically confirmed bank
                let pre_purge_bank_count = slot_prioritization_fee.len() as u64;
                slot_prioritization_fee.retain(|id, _| *id == bank_id);
                let post_purge_bank_count = slot_prioritization_fee.len() as u64;
                metrics.accumulate_total_purged_duplicated_bank_count(
                    pre_purge_bank_count.saturating_sub(post_purge_bank_count),
                );
                // It should be rare that optimistically confirmed bank had no prioritized
                // transactions, but duplicated and unconfirmed bank had.
                if pre_purge_bank_count > 0 && post_purge_bank_count == 0 {
                    warn!("Finalized bank has empty prioritization fee cache. slot {slot} bank id {bank_id}");
                }

                let mut block_prioritization_fee = slot_prioritization_fee
                    .entry(bank_id)
                    .or_insert(PrioritizationFee::default());
                if let Err(err) = block_prioritization_fee.mark_block_completed() {
                    error!(
                        "Unsuccessful finalizing slot {slot}, bank ID {bank_id}: {:?}",
                        err
                    );
                }
                block_prioritization_fee.report_metrics(slot);
            },
            "slot_finalize_time"
        );
        metrics.accumulate_total_block_finalize_elapsed_us(slot_finalize_time.as_us());

        // Create new cache entry
        let (_, cache_lock_time) = measure!(
            {
                let mut cache = cache.write().unwrap();
                while cache.len() >= cache_max_size {
                    cache.pop_first();
                }
                cache.insert(slot, slot_prioritization_fee);
            },
            "cache_lock_time"
        );
        metrics.accumulate_total_cache_lock_elapsed_us(cache_lock_time.as_us());
    }

    fn service_loop(
        cache: Arc<RwLock<BTreeMap<Slot, SlotPrioritizationFee>>>,
        cache_max_size: usize,
        receiver: Receiver<CacheServiceUpdate>,
        metrics: Arc<PrioritizationFeeCacheMetrics>,
    ) {
        let mut unfinalized = BTreeMap::<Slot, SlotPrioritizationFee>::new();

        for update in receiver.iter() {
            match update {
                CacheServiceUpdate::TransactionUpdate {
                    slot,
                    bank_id,
                    transaction_fee,
                    writable_accounts,
                } => Self::update_cache(
                    &mut unfinalized,
                    slot,
                    bank_id,
                    transaction_fee,
                    &writable_accounts,
                    &metrics,
                ),
                CacheServiceUpdate::BankFinalized { slot, bank_id } => {
                    Self::finalize_slot(
                        &mut unfinalized,
                        &cache,
                        cache_max_size,
                        slot,
                        bank_id,
                        &metrics,
                    );

                    metrics.report(slot);
                }
                CacheServiceUpdate::Exit => {
                    break;
                }
            }
        }
    }

    /// Returns number of blocks that have finalized minimum fees collection
    pub fn available_block_count(&self) -> usize {
        self.cache.read().unwrap().len()
    }

    pub fn get_prioritization_fees(&self, account_keys: &[Pubkey]) -> HashMap<Slot, u64> {
        self.cache
            .read()
            .unwrap()
            .iter()
            .filter_map(|(slot, slot_prioritization_fee)| {
                slot_prioritization_fee
                    .iter()
                    .find_map(|prioritization_fee| {
                        let mut fee = prioritization_fee
                            .get_min_transaction_fee()
                            .unwrap_or_default();
                        for account_key in account_keys {
                            if let Some(account_fee) =
                                prioritization_fee.get_writable_account_fee(account_key)
                            {
                                fee = std::cmp::max(fee, account_fee);
                            }
                        }
                        Some((*slot, fee))
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::Bank,
            bank_forks::BankForks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            message::Message,
            pubkey::Pubkey,
            system_instruction,
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    fn build_sanitized_transaction_for_test(
        compute_unit_price: u64,
        signer_account: &Pubkey,
        write_account: &Pubkey,
    ) -> SanitizedTransaction {
        let transaction = Transaction::new_unsigned(Message::new(
            &[
                system_instruction::transfer(signer_account, write_account, 1),
                ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
            ],
            Some(signer_account),
        ));

        SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap()
    }

    // update fee cache is asynchronous, this test helper blocks until update is completed.
    fn sync_update<'a>(
        prioritization_fee_cache: &PrioritizationFeeCache,
        bank: Arc<Bank>,
        txs: impl Iterator<Item = &'a SanitizedTransaction> + ExactSizeIterator,
    ) {
        let expected_update_count = prioritization_fee_cache
            .metrics
            .successful_transaction_update_count
            .load(Ordering::Relaxed)
            .saturating_add(txs.len() as u64);

        prioritization_fee_cache.update(&bank, txs);

        // wait till expected number of transaction updates have occurred...
        while prioritization_fee_cache
            .metrics
            .successful_transaction_update_count
            .load(Ordering::Relaxed)
            != expected_update_count
        {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    // finalization is asynchronous, this test helper blocks until finalization is completed.
    fn sync_finalize_priority_fee_for_test(
        prioritization_fee_cache: &PrioritizationFeeCache,
        slot: Slot,
        bank_id: BankId,
    ) {
        // mark as finalized
        prioritization_fee_cache.finalize_priority_fee(slot, bank_id);

        // wait till finalization is done
        loop {
            let cache = prioritization_fee_cache.cache.read().unwrap();
            if let Some(slot_cache) = cache.get(&slot) {
                if let Some(block_fee) = slot_cache.get(&bank_id) {
                    if block_fee.is_finalized() {
                        return;
                    }
                }
            }
            drop(cache);

            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn test_prioritization_fee_cache_update() {
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        // Set up test with 3 transactions, in format of [fee, write-accounts...],
        // Shall expect fee cache is updated in following sequence:
        // transaction                    block minimum prioritization fee cache
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [5,   a, b             ]  -->  [5,     5,         5,         nil      ]
        // [9,      b, c          ]  -->  [5,     5,         5,         9        ]
        // [2,   a,    c          ]  -->  [2,     2,         5,         2        ]
        //
        let txs = vec![
            build_sanitized_transaction_for_test(5, &write_account_a, &write_account_b),
            build_sanitized_transaction_for_test(9, &write_account_b, &write_account_c),
            build_sanitized_transaction_for_test(2, &write_account_a, &write_account_c),
        ];

        let bank = Arc::new(Bank::default_for_tests());
        let slot = bank.slot();

        let prioritization_fee_cache = PrioritizationFeeCache::default();
        sync_update(&prioritization_fee_cache, bank.clone(), txs.iter());

        // assert block minimum fee and account a, b, c fee accordingly
        {
            // Not possible to check the state in the thread
            // let lock = prioritization_fee_cache.cache.read().unwrap();
            // let fee = lock.get(&slot).unwrap();
            // let fee = fee.get(&bank.bank_id()).unwrap();
            // assert_eq!(2, fee.get_min_transaction_fee().unwrap());
            // assert_eq!(2, fee.get_writable_account_fee(&write_account_a).unwrap());
            // assert_eq!(5, fee.get_writable_account_fee(&write_account_b).unwrap());
            // assert_eq!(2, fee.get_writable_account_fee(&write_account_c).unwrap());
            // // assert unknown account d fee
            // assert!(fee
            //     .get_writable_account_fee(&Pubkey::new_unique())
            //     .is_none());
        }

        // assert after prune, account a and c should be removed from cache to save space
        {
            sync_finalize_priority_fee_for_test(&prioritization_fee_cache, slot, bank.bank_id());
            let lock = prioritization_fee_cache.cache.read().unwrap();
            let fee = lock.get(&slot).unwrap();
            let fee = fee.get(&bank.bank_id()).unwrap();
            assert_eq!(2, fee.get_min_transaction_fee().unwrap());
            assert!(fee.get_writable_account_fee(&write_account_a).is_none());
            assert_eq!(5, fee.get_writable_account_fee(&write_account_b).unwrap());
            assert!(fee.get_writable_account_fee(&write_account_c).is_none());
        }
    }

    #[test]
    fn test_available_block_count() {
        let prioritization_fee_cache = PrioritizationFeeCache::default();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank0 = Bank::new_for_benches(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank0);
        let bank = bank_forks.read().unwrap().working_bank();
        let collector = solana_sdk::pubkey::new_rand();

        let bank1 = Arc::new(Bank::new_from_parent(bank.clone(), &collector, 1));
        sync_update(
            &prioritization_fee_cache,
            bank1.clone(),
            vec![build_sanitized_transaction_for_test(
                1,
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
            )]
            .iter(),
        );
        sync_finalize_priority_fee_for_test(&prioritization_fee_cache, 1, bank1.bank_id());

        let bank2 = Arc::new(Bank::new_from_parent(bank.clone(), &collector, 2));
        sync_update(
            &prioritization_fee_cache,
            bank2.clone(),
            vec![build_sanitized_transaction_for_test(
                1,
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
            )]
            .iter(),
        );
        sync_finalize_priority_fee_for_test(&prioritization_fee_cache, 2, bank2.bank_id());

        // add slot 3 entry to cache, but not finalize it
        let bank3 = Arc::new(Bank::new_from_parent(bank.clone(), &collector, 3));
        let txs = vec![build_sanitized_transaction_for_test(
            1,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
        )];
        sync_update(&prioritization_fee_cache, bank3.clone(), txs.iter());

        // assert available block count should be 2 finalized blocks
        std::thread::sleep(std::time::Duration::from_millis(1_000));
        assert_eq!(2, prioritization_fee_cache.available_block_count());
    }

    fn hashmap_of(vec: Vec<(Slot, u64)>) -> HashMap<Slot, u64> {
        vec.into_iter().collect()
    }

    #[test]
    fn test_get_prioritization_fees() {
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank0 = Bank::new_for_benches(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank0);
        let bank = bank_forks.read().unwrap().working_bank();
        let collector = solana_sdk::pubkey::new_rand();
        let bank1 = Arc::new(Bank::new_from_parent(bank.clone(), &collector, 1));
        let bank2 = Arc::new(Bank::new_from_parent(bank.clone(), &collector, 2));
        let bank3 = Arc::new(Bank::new_from_parent(bank, &collector, 3));

        let prioritization_fee_cache = PrioritizationFeeCache::default();

        // Assert no minimum fee from empty cache
        assert!(prioritization_fee_cache
            .get_prioritization_fees(&[])
            .is_empty());
        assert!(prioritization_fee_cache
            .get_prioritization_fees(&[write_account_a])
            .is_empty());
        assert!(prioritization_fee_cache
            .get_prioritization_fees(&[write_account_b])
            .is_empty());
        assert!(prioritization_fee_cache
            .get_prioritization_fees(&[write_account_c])
            .is_empty());
        assert!(prioritization_fee_cache
            .get_prioritization_fees(&[write_account_a, write_account_b])
            .is_empty());
        assert!(prioritization_fee_cache
            .get_prioritization_fees(&[write_account_a, write_account_b, write_account_c])
            .is_empty());

        // Assert after add one transaction for slot 1
        {
            let txs = vec![
                build_sanitized_transaction_for_test(2, &write_account_a, &write_account_b),
                build_sanitized_transaction_for_test(
                    1,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            sync_update(&prioritization_fee_cache, bank1.clone(), txs.iter());
            // before block is marked as completed
            assert!(prioritization_fee_cache
                .get_prioritization_fees(&[])
                .is_empty());
            assert!(prioritization_fee_cache
                .get_prioritization_fees(&[write_account_a])
                .is_empty());
            assert!(prioritization_fee_cache
                .get_prioritization_fees(&[write_account_b])
                .is_empty());
            assert!(prioritization_fee_cache
                .get_prioritization_fees(&[write_account_c])
                .is_empty());
            assert!(prioritization_fee_cache
                .get_prioritization_fees(&[write_account_a, write_account_b])
                .is_empty());
            assert!(prioritization_fee_cache
                .get_prioritization_fees(&[write_account_a, write_account_b, write_account_c])
                .is_empty());
            // after block is completed
            sync_finalize_priority_fee_for_test(&prioritization_fee_cache, 1, bank1.bank_id());
            assert_eq!(
                hashmap_of(vec![(1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_a])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_b])
            );
            assert_eq!(
                hashmap_of(vec![(1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_c])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache
                    .get_prioritization_fees(&[write_account_a, write_account_b])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[
                    write_account_a,
                    write_account_b,
                    write_account_c
                ])
            );
        }

        // Assert after add one transaction for slot 2
        {
            let txs = vec![
                build_sanitized_transaction_for_test(4, &write_account_b, &write_account_c),
                build_sanitized_transaction_for_test(
                    3,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            sync_update(&prioritization_fee_cache, bank2.clone(), txs.iter());
            // before block is marked as completed
            assert_eq!(
                hashmap_of(vec![(1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_a])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_b])
            );
            assert_eq!(
                hashmap_of(vec![(1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_c])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache
                    .get_prioritization_fees(&[write_account_a, write_account_b])
            );
            assert_eq!(
                hashmap_of(vec![(1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[
                    write_account_a,
                    write_account_b,
                    write_account_c
                ])
            );
            // after block is completed
            sync_finalize_priority_fee_for_test(&prioritization_fee_cache, 2, bank2.bank_id());
            assert_eq!(
                hashmap_of(vec![(2, 3), (1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 3), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_a]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_b]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_c]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 2)]),
                prioritization_fee_cache
                    .get_prioritization_fees(&[write_account_a, write_account_b]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[
                    write_account_a,
                    write_account_b,
                    write_account_c,
                ]),
            );
        }

        // Assert after add one transaction for slot 3
        {
            let txs = vec![
                build_sanitized_transaction_for_test(6, &write_account_a, &write_account_c),
                build_sanitized_transaction_for_test(
                    5,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            sync_update(&prioritization_fee_cache, bank3.clone(), txs.iter());
            // before block is marked as completed
            assert_eq!(
                hashmap_of(vec![(2, 3), (1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 3), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_a]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_b]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_c]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 2)]),
                prioritization_fee_cache
                    .get_prioritization_fees(&[write_account_a, write_account_b]),
            );
            assert_eq!(
                hashmap_of(vec![(2, 4), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[
                    write_account_a,
                    write_account_b,
                    write_account_c,
                ]),
            );
            // after block is completed
            sync_finalize_priority_fee_for_test(&prioritization_fee_cache, 3, bank3.bank_id());
            assert_eq!(
                hashmap_of(vec![(3, 5), (2, 3), (1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[]),
            );
            assert_eq!(
                hashmap_of(vec![(3, 6), (2, 3), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_a]),
            );
            assert_eq!(
                hashmap_of(vec![(3, 5), (2, 4), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_b]),
            );
            assert_eq!(
                hashmap_of(vec![(3, 6), (2, 4), (1, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_c]),
            );
            assert_eq!(
                hashmap_of(vec![(3, 6), (2, 4), (1, 2)]),
                prioritization_fee_cache
                    .get_prioritization_fees(&[write_account_a, write_account_b]),
            );
            assert_eq!(
                hashmap_of(vec![(3, 6), (2, 4), (1, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[
                    write_account_a,
                    write_account_b,
                    write_account_c,
                ]),
            );
        }
    }

    #[test]
    fn test_purge_duplicated_bank() {
        // duplicated bank can exists for same slot before OC.
        // prioritization_fee_cache should only have data from OC-ed bank
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank0 = Bank::new_for_benches(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank0);
        let bank = bank_forks.read().unwrap().working_bank();
        let collector = solana_sdk::pubkey::new_rand();
        let slot: Slot = 999;
        let bank1 = Arc::new(Bank::new_from_parent(bank.clone(), &collector, slot));
        let bank2 = Arc::new(Bank::new_from_parent(bank, &collector, slot));

        let prioritization_fee_cache = PrioritizationFeeCache::default();

        // Assert after add transactions for bank1 of slot 1
        {
            let txs = vec![
                build_sanitized_transaction_for_test(2, &write_account_a, &write_account_b),
                build_sanitized_transaction_for_test(
                    1,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            sync_update(&prioritization_fee_cache, bank1.clone(), txs.iter());

            // Not possible to check the state in the thread
            // let slot_prioritization_fee = PrioritizationFeeCache::get_prioritization_fee(
            //     prioritization_fee_cache.cache.clone(),
            //     &slot,
            // );
            // assert_eq!(1, slot_prioritization_fee.len());
            // assert!(slot_prioritization_fee.contains_key(&bank1.bank_id()));
        }

        // Assert after add transactions for bank2 of slot 1
        {
            let txs = vec![
                build_sanitized_transaction_for_test(4, &write_account_b, &write_account_c),
                build_sanitized_transaction_for_test(
                    3,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            sync_update(&prioritization_fee_cache, bank2.clone(), txs.iter());

            // Not possible to check the state in the thread
            // let slot_prioritization_fee = PrioritizationFeeCache::get_prioritization_fee(
            //     prioritization_fee_cache.cache.clone(),
            //     &slot,
            // );
            // assert_eq!(2, slot_prioritization_fee.len());
            // assert!(slot_prioritization_fee.contains_key(&bank1.bank_id()));
            // assert!(slot_prioritization_fee.contains_key(&bank2.bank_id()));
        }

        // Assert after finalize with bank1 of slot 1,
        {
            sync_finalize_priority_fee_for_test(&prioritization_fee_cache, slot, bank1.bank_id());

            let cache_lock = prioritization_fee_cache.cache.read().unwrap();
            let slot_prioritization_fee = cache_lock.get(&slot).unwrap();
            assert_eq!(1, slot_prioritization_fee.len());
            assert!(slot_prioritization_fee.contains_key(&bank1.bank_id()));

            // and data available for query are from bank1
            assert_eq!(
                hashmap_of(vec![(slot, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[])
            );
            assert_eq!(
                hashmap_of(vec![(slot, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_a])
            );
            assert_eq!(
                hashmap_of(vec![(slot, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_b])
            );
            assert_eq!(
                hashmap_of(vec![(slot, 1)]),
                prioritization_fee_cache.get_prioritization_fees(&[write_account_c])
            );
            assert_eq!(
                hashmap_of(vec![(slot, 2)]),
                prioritization_fee_cache
                    .get_prioritization_fees(&[write_account_a, write_account_b])
            );
            assert_eq!(
                hashmap_of(vec![(slot, 2)]),
                prioritization_fee_cache.get_prioritization_fees(&[
                    write_account_a,
                    write_account_b,
                    write_account_c
                ])
            );
        }
    }
}
