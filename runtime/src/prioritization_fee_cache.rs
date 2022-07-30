use {
    crate::prioritization_fee::*,
    crossbeam_channel::{unbounded, Receiver, Sender},
    log::*,
    solana_measure::measure,
    solana_sdk::{
        clock::Slot, pubkey::Pubkey, saturating_add_assign, transaction::SanitizedTransaction,
    },
    std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        thread::{Builder, JoinHandle},
    },
};

/// The maximum number of blocks to keep in `PrioritizationFeeCache`; States from
/// up to 150 recent blocks should be sufficient to estimate minimal prioritization fee to
/// land transactions to current block.
const MAX_NUM_RECENT_BLOCKS: usize = 150;

#[derive(Debug, Default)]
struct PrioritizationFeeCacheMetrics {
    // Count of transactions that successfully updated each slot's prioritization fee cache.
    successful_transaction_update_count: u64,

    // Count of writable accounts has min prioritization fee in this slot
    total_writable_accounts_count: u64,

    // Count of writeable accounts those min prioritization fee is higher than min transaction fee
    // in this slot.
    relevant_writable_accounts_count: u64,

    // Count of transactions that failed to get their priority details.
    fail_get_transaction_priority_details_count: u64,

    // Count of transactions that failed to get their account locks.
    fail_get_transaction_account_locks_count: u64,

    // Accumulated time spent on tracking prioritization fee for each slot.
    total_update_elapsed_us: u64,
}

impl PrioritizationFeeCacheMetrics {
    fn increment_successful_transaction_update_count(&mut self, val: u64) {
        saturating_add_assign!(self.successful_transaction_update_count, val);
    }

    fn increment_total_writable_accounts_count(&mut self, val: u64) {
        saturating_add_assign!(self.total_writable_accounts_count, val);
    }

    fn increment_relevant_writable_accounts_count(&mut self, val: u64) {
        saturating_add_assign!(self.relevant_writable_accounts_count, val);
    }

    fn increment_fail_get_transaction_priority_details_count(&mut self, val: u64) {
        saturating_add_assign!(self.fail_get_transaction_priority_details_count, val);
    }

    fn increment_fail_get_transaction_account_locks_count(&mut self, val: u64) {
        saturating_add_assign!(self.fail_get_transaction_account_locks_count, val);
    }

    fn increment_total_update_elapsed_us(&mut self, val: u64) {
        saturating_add_assign!(self.total_update_elapsed_us, val);
    }

    fn report(&mut self, slot: Slot) {
        datapoint_info!(
            "block_prioritization_fee_counters",
            ("slot", slot as i64, i64),
            (
                "successful_transaction_update_count",
                self.successful_transaction_update_count as i64,
                i64
            ),
            (
                "total_writable_accounts_count",
                self.total_writable_accounts_count as i64,
                i64
            ),
            (
                "relevant_writable_accounts_count",
                self.relevant_writable_accounts_count as i64,
                i64
            ),
            (
                "fail_get_transaction_priority_details_count",
                self.fail_get_transaction_priority_details_count as i64,
                i64
            ),
            (
                "fail_get_transaction_account_locks_count",
                self.fail_get_transaction_account_locks_count as i64,
                i64
            ),
            (
                "total_update_elapsed_us",
                self.total_update_elapsed_us as i64,
                i64
            ),
        );
        *self = PrioritizationFeeCacheMetrics::default();
    }
}

/// Each block's PrioritizationFee entry is wrapped in Arc<Mutex<...>>
/// Reader and writer should avoid to contend `PrioritizationFeeCache` but on individual block's PrioritizationFeeEntry.
type PrioritizationFeeEntry = Arc<Mutex<PrioritizationFee>>;

enum FinalizingSerivceUpdate {
    BankFrozen {
        slot: Slot,
        prioritization_fee: PrioritizationFeeEntry,
    },
    Exit,
}

/// Stores up to MAX_NUM_RECENT_BLOCKS recent block's prioritization fee,
/// A separate internal thread `finalizing_thread` handles additional tasks when a bank is frozen,
/// includes pruning PrioritizationFee's HashMap, collecting stats and reporting metrics.
pub struct PrioritizationFeeCache {
    cache: HashMap<Slot, PrioritizationFeeEntry>,
    metrics: PrioritizationFeeCacheMetrics,
    // Asynchronously finalize prioritization fee when a bank is completed replay.
    finalizing_thread: Option<JoinHandle<()>>,
    sender: Sender<FinalizingSerivceUpdate>,
}

impl Default for PrioritizationFeeCache {
    fn default() -> Self {
        Self::new(MAX_NUM_RECENT_BLOCKS)
    }
}

impl Drop for PrioritizationFeeCache {
    fn drop(&mut self) {
        let _ = self.sender.send(FinalizingSerivceUpdate::Exit);
        self.finalizing_thread
            .take()
            .unwrap()
            .join()
            .expect("Prioritization fee cache finalizing thread failed to join");
    }
}

impl PrioritizationFeeCache {
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = unbounded();
        let finalizing_thread = Some(
            Builder::new()
                .name("prioritization-fee-cache-finalizing-thread".to_string())
                .spawn(move || {
                    Self::finalizing_loop(receiver);
                })
                .unwrap(),
        );

        PrioritizationFeeCache {
            cache: HashMap::with_capacity(capacity),
            metrics: PrioritizationFeeCacheMetrics::default(),
            finalizing_thread,
            sender,
        }
    }

    /// get prioritization fee entry, create new entry if necessary
    fn get_prioritization_fee(&mut self, slot: &Slot) -> PrioritizationFeeEntry {
        match self.cache.get(slot) {
            Some(entry) => Arc::clone(entry),
            None => {
                let entry = Arc::new(Mutex::new(PrioritizationFee::default()));
                self.cache.insert(*slot, Arc::clone(&entry));
                entry
            }
        }
    }

    fn find_prioritization_fee(&self, slot: &Slot) -> Option<&PrioritizationFeeEntry> {
        self.cache.get(slot)
    }

    /// Update block's min prioritization fee with `txs`,
    /// Returns updated min prioritization fee for `slot`
    pub fn update_transactions<'a>(
        &mut self,
        slot: Slot,
        txs: impl Iterator<Item = &'a SanitizedTransaction>,
    ) -> Option<u64> {
        let mut successful_transaction_update_count: u64 = 0;
        let mut fail_get_transaction_priority_details_count: u64 = 0;
        let mut fail_get_transaction_account_locks_count: u64 = 0;

        let (updated_min_fee, cache_update_time) = measure!(
            {
                let block_prioritization_fee = self.get_prioritization_fee(&slot);

                // Hold lock of slot's prioritization fee entry until all transactions are
                // processed
                let mut block_prioritization_fee = block_prioritization_fee.lock().unwrap();
                for sanitized_tx in txs {
                    match block_prioritization_fee.update(sanitized_tx) {
                        Err(PrioritizationFeeError::FailGetTransactionPriorityDetails) => {
                            saturating_add_assign!(fail_get_transaction_priority_details_count, 1)
                        }
                        Err(PrioritizationFeeError::FailGetTransactionAccountLocks) => {
                            saturating_add_assign!(fail_get_transaction_account_locks_count, 1)
                        }
                        _ => saturating_add_assign!(successful_transaction_update_count, 1),
                    }
                }
                block_prioritization_fee.get_min_transaction_fee()
            },
            "cache_update"
        );

        self.metrics
            .increment_successful_transaction_update_count(successful_transaction_update_count);
        self.metrics
            .increment_fail_get_transaction_priority_details_count(
                fail_get_transaction_priority_details_count,
            );
        self.metrics
            .increment_fail_get_transaction_account_locks_count(
                fail_get_transaction_account_locks_count,
            );
        self.metrics
            .increment_total_update_elapsed_us(cache_update_time.as_us());

        updated_min_fee
    }

    /// bank is completely replayed from blockstore, prune irrelevant accounts to save space,
    /// its fee stats can be made available to queries
    pub fn finalize_priority_fee(&mut self, slot: Slot) {
        if let Some(prioritization_fee) = self.find_prioritization_fee(&slot) {
            self.sender
                .send(FinalizingSerivceUpdate::BankFrozen {
                    slot,
                    prioritization_fee: prioritization_fee.clone(),
                })
                .unwrap_or_else(|err| {
                    warn!(
                        "prioritization fee cache signalling bank frozen failed: {:?}",
                        err
                    )
                });
        } else {
            // this should not happen
            warn!("bank {} is frozen, but prioritization fee cache does not have record for this bank", slot);
        }
    }

    fn finalizing_loop(receiver: Receiver<FinalizingSerivceUpdate>) {
        for update in receiver.iter() {
            match update {
                FinalizingSerivceUpdate::BankFrozen {
                    slot,
                    prioritization_fee,
                } => {
                    let mut pre_writable_accounts_count: usize = 0;
                    let mut post_writable_accounts_count: usize = 0;

                    // prune cache by evicting write account entry from prioritization fee if its fee is less
                    // or equal to block's min transaction fee, because they are irrelevant in calculating
                    // block min fee.
                    {
                        let mut prioritization_fee = prioritization_fee.lock().unwrap();
                        pre_writable_accounts_count =
                            prioritization_fee.get_writable_accounts_count();
                        prioritization_fee.prune_irrelevant_writable_accounts();
                        post_writable_accounts_count =
                            prioritization_fee.get_writable_accounts_count();
                        let _ = prioritization_fee.mark_block_completed();
                    }
                    /*
                            metric
                                .increment_total_writable_accounts_count(pre_writable_accounts_count as u64);
                            metrics
                                .increment_relevant_writable_accounts_count(post_writable_accounts_count as u64);
                            metrics.report(slot);

                            // report current min fees for slot, including min_transaction_fee and top 10
                            // min_writable_account_fees
                                datapoint_info!(
                                    "block_prioritization_fee",
                                    ("slot", slot as i64, i64),
                                    ("entity", "block", String),
                                    (
                                        "min_prioritization_fee",
                                        prioritization_fee.get_min_transaction_fee().unwrap_or(0) as i64,
                                        i64
                                    ),
                                );
                                let mut accounts_fees: Vec<_> =
                                    prioritization_fee.get_writable_account_fees().collect();
                                accounts_fees.sort_by(|lh, rh| rh.1.cmp(lh.1));
                                for (account_key, fee) in accounts_fees.iter().take(10) {
                                    datapoint_info!(
                                        "block_prioritization_fee",
                                        ("slot", slot as i64, i64),
                                        ("entity", account_key.to_string(), String),
                                        ("min_prioritization_fee", **fee as i64, i64),
                                    );
                                }
                    // */
                }
                FinalizingSerivceUpdate::Exit => {
                    break;
                }
            }
        }
    }

    /// Returns number of blocks that have finalized min fees collection
    pub fn available_block_count(&self) -> usize {
        self.cache
            .iter()
            .filter(|(_slot, prioritization_fee)| prioritization_fee.lock().unwrap().is_finalized())
            .count()
    }

    /// Query block minimum fees from finalized blocks in cache,
    /// Returns a vector of fee; call site can use it to produce
    /// average, or top 5% etc.
    pub fn get_prioritization_fees(&self) -> Vec<u64> {
        self.cache
            .iter()
            .filter_map(|(_slot, prioritization_fee)| {
                let prioritization_fee_read = prioritization_fee.lock().unwrap();
                prioritization_fee_read
                    .is_finalized()
                    .then(|| prioritization_fee_read.get_min_transaction_fee())
            })
            .flatten()
            .collect()
    }

    /// Query given account minimum fees from finalized blocks in cache,
    /// Returns a vector of fee; call site can use it to produce
    /// average, or top 5% etc.
    pub fn get_account_prioritization_fees(&self, account_key: &Pubkey) -> Vec<u64> {
        self.cache
            .iter()
            .filter_map(|(_slot, prioritization_fee)| {
                let prioritization_fee_read = prioritization_fee.lock().unwrap();
                prioritization_fee_read
                    .is_finalized()
                    .then(|| prioritization_fee_read.get_writable_account_fee(account_key))
            })
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction, message::Message, pubkey::Pubkey,
            system_instruction, transaction::Transaction,
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

    // finalization is asynchronous, this test helper will block until finalization is completed.
    fn sync_finalize_priority_fee_for_test(
        prioritization_fee_cache: &mut PrioritizationFeeCache,
        slot: Slot,
    ) {
        prioritization_fee_cache.finalize_priority_fee(slot);
        let fee = prioritization_fee_cache
            .find_prioritization_fee(&slot)
            .unwrap();

        // wait till finalization is done
        while !fee.lock().unwrap().is_finalized() {
            std::thread::sleep(std::time::Duration::from_millis(100));
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
        // transaction                    block min prioritization fee cache
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

        let slot = 1;

        let mut prioritization_fee_cache = PrioritizationFeeCache::default();
        assert_eq!(
            2,
            prioritization_fee_cache
                .update_transactions(slot, txs.iter())
                .unwrap()
        );

        // assert block min fee and account a, b, c fee accordingly
        {
            let fee = prioritization_fee_cache
                .find_prioritization_fee(&slot)
                .unwrap();
            let fee = fee.lock().unwrap();
            assert_eq!(2, fee.get_min_transaction_fee().unwrap());
            assert_eq!(2, fee.get_writable_account_fee(&write_account_a).unwrap());
            assert_eq!(5, fee.get_writable_account_fee(&write_account_b).unwrap());
            assert_eq!(2, fee.get_writable_account_fee(&write_account_c).unwrap());
            // assert unknown account d fee
            assert!(fee
                .get_writable_account_fee(&Pubkey::new_unique())
                .is_none());
            // assert unknown slot
            assert!(prioritization_fee_cache
                .find_prioritization_fee(&100)
                .is_none());
        }

        // assert after prune, account a and c should be removed from cache to save space
        {
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, slot);
            let fee = prioritization_fee_cache
                .find_prioritization_fee(&slot)
                .unwrap();
            let fee = fee.lock().unwrap();
            assert_eq!(2, fee.get_min_transaction_fee().unwrap());
            assert!(fee.get_writable_account_fee(&write_account_a).is_none());
            assert_eq!(5, fee.get_writable_account_fee(&write_account_b).unwrap());
            assert!(fee.get_writable_account_fee(&write_account_c).is_none());
        }
    }

    #[test]
    fn test_available_block_count() {
        let mut prioritization_fee_cache = PrioritizationFeeCache::default();

        assert!(prioritization_fee_cache
            .get_prioritization_fee(&1)
            .lock()
            .unwrap()
            .mark_block_completed()
            .is_ok());
        assert!(prioritization_fee_cache
            .get_prioritization_fee(&2)
            .lock()
            .unwrap()
            .mark_block_completed()
            .is_ok());
        // add slot 3 entry to cache, but not finalize it
        prioritization_fee_cache.get_prioritization_fee(&3);

        // assert available block count should be 2 finalized blocks
        assert_eq!(2, prioritization_fee_cache.available_block_count());
    }

    fn assert_vec_eq(expected: &mut Vec<u64>, actual: &mut Vec<u64>) {
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_get_prioritization_fees() {
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        let mut prioritization_fee_cache = PrioritizationFeeCache::default();

        // Assert no min fee from empty cache
        assert!(prioritization_fee_cache
            .get_prioritization_fees()
            .is_empty());

        // Assert after add one transaction for slot 1
        {
            let txs = vec![build_sanitized_transaction_for_test(
                5,
                &write_account_a,
                &write_account_b,
            )];
            assert_eq!(
                5,
                prioritization_fee_cache
                    .update_transactions(1, txs.iter())
                    .unwrap()
            );
            // before block is marked as completed
            assert!(prioritization_fee_cache
                .get_prioritization_fees()
                .is_empty());
            // after block is completed
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, 1);
            assert_eq!(vec![5], prioritization_fee_cache.get_prioritization_fees());
        }

        // Assert after add one transaction for slot 2
        {
            let txs = vec![build_sanitized_transaction_for_test(
                9,
                &write_account_b,
                &write_account_c,
            )];
            assert_eq!(
                9,
                prioritization_fee_cache
                    .update_transactions(2, txs.iter())
                    .unwrap()
            );
            // before block is marked as completed
            assert_eq!(vec![5], prioritization_fee_cache.get_prioritization_fees());
            // after block is completed
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, 2);
            assert_vec_eq(
                &mut vec![5, 9],
                &mut prioritization_fee_cache.get_prioritization_fees(),
            );
        }

        // Assert after add one transaction for slot 3
        {
            let txs = vec![build_sanitized_transaction_for_test(
                2,
                &write_account_a,
                &write_account_c,
            )];
            assert_eq!(
                2,
                prioritization_fee_cache
                    .update_transactions(3, txs.iter())
                    .unwrap()
            );
            // before block is marked as completed
            assert_vec_eq(
                &mut vec![5, 9],
                &mut prioritization_fee_cache.get_prioritization_fees(),
            );
            // after block is completed
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, 3);
            assert_vec_eq(
                &mut vec![5, 9, 2],
                &mut prioritization_fee_cache.get_prioritization_fees(),
            );
        }
    }

    #[test]
    fn test_get_account_prioritization_fees() {
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        let mut prioritization_fee_cache = PrioritizationFeeCache::default();

        // Assert no min fee from empty cache
        assert!(prioritization_fee_cache
            .get_account_prioritization_fees(&write_account_a)
            .is_empty());
        assert!(prioritization_fee_cache
            .get_account_prioritization_fees(&write_account_b)
            .is_empty());
        assert!(prioritization_fee_cache
            .get_account_prioritization_fees(&write_account_c)
            .is_empty());

        // Assert after add one transaction for slot 1
        {
            let txs = vec![
                build_sanitized_transaction_for_test(5, &write_account_a, &write_account_b),
                build_sanitized_transaction_for_test(
                    0,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            prioritization_fee_cache.update_transactions(1, txs.iter());
            // before block is marked as completed
            assert!(prioritization_fee_cache
                .get_account_prioritization_fees(&write_account_a)
                .is_empty());
            assert!(prioritization_fee_cache
                .get_account_prioritization_fees(&write_account_b)
                .is_empty());
            assert!(prioritization_fee_cache
                .get_account_prioritization_fees(&write_account_c)
                .is_empty());
            // after block is completed
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, 1);
            assert_eq!(
                vec![5],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_a)
            );
            assert_eq!(
                vec![5],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_b)
            );
            assert!(prioritization_fee_cache
                .get_account_prioritization_fees(&write_account_c)
                .is_empty());
        }

        // Assert after add one transaction for slot 2
        {
            let txs = vec![
                build_sanitized_transaction_for_test(9, &write_account_b, &write_account_c),
                build_sanitized_transaction_for_test(
                    0,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            prioritization_fee_cache.update_transactions(2, txs.iter());
            // before block is marked as completed
            assert_eq!(
                vec![5],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_a)
            );
            assert_eq!(
                vec![5],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_b)
            );
            assert!(prioritization_fee_cache
                .get_account_prioritization_fees(&write_account_c)
                .is_empty());
            // after block is completed
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, 2);
            assert_eq!(
                vec![5],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_a)
            );
            assert_vec_eq(
                &mut vec![5, 9],
                &mut prioritization_fee_cache.get_account_prioritization_fees(&write_account_b),
            );
            assert_eq!(
                vec![9],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_c)
            );
        }

        // Assert after add one transaction for slot 3
        {
            let txs = vec![
                build_sanitized_transaction_for_test(2, &write_account_a, &write_account_c),
                build_sanitized_transaction_for_test(
                    0,
                    &Pubkey::new_unique(),
                    &Pubkey::new_unique(),
                ),
            ];
            prioritization_fee_cache.update_transactions(3, txs.iter());
            // before block is marked as completed
            assert_eq!(
                vec![5],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_a)
            );
            assert_vec_eq(
                &mut vec![5, 9],
                &mut prioritization_fee_cache.get_account_prioritization_fees(&write_account_b),
            );
            assert_eq!(
                vec![9],
                prioritization_fee_cache.get_account_prioritization_fees(&write_account_c)
            );
            // after block is completed
            sync_finalize_priority_fee_for_test(&mut prioritization_fee_cache, 3);
            assert_vec_eq(
                &mut vec![5, 2],
                &mut prioritization_fee_cache.get_account_prioritization_fees(&write_account_a),
            );
            assert_vec_eq(
                &mut vec![5, 9],
                &mut prioritization_fee_cache.get_account_prioritization_fees(&write_account_b),
            );
            assert_vec_eq(
                &mut vec![9, 2],
                &mut prioritization_fee_cache.get_account_prioritization_fees(&write_account_c),
            );
        }
    }
}
