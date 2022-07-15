use {
    crate::prioritization_fee::*,
    solana_sdk::{
        clock::Slot, pubkey::Pubkey, saturating_add_assign, transaction::SanitizedTransaction,
    },
    std::collections::HashMap,
};

/// The maximum number of blocks to keep in `PrioritizationFeeCache`; States from
/// up to 150 recent blocks should be sufficient to estimate minimal prioritization fee to
/// land transactions to current block.
const MAX_NUM_RECENT_BLOCKS: usize = 150;

/// Holds up to MAX_NUM_RECENT_BLOCKS recent block's min prioritization fee for block,
/// and for each writable accounts per block.
pub struct PrioritizationFeeCache {
    cache: HashMap<Slot, PrioritizationFee>,
    // metrics counts
    successful_transaction_update_count: u64,
    fail_get_transaction_priority_details_count: u64,
    fail_get_transaction_account_locks_count: u64,
    fail_finalize_block_not_found: u64,
}

impl Default for PrioritizationFeeCache {
    fn default() -> Self {
        Self::new(MAX_NUM_RECENT_BLOCKS)
    }
}

impl PrioritizationFeeCache {
    pub fn new(capacity: usize) -> Self {
        PrioritizationFeeCache {
            cache: HashMap::with_capacity(capacity),
            //            ..default()
            successful_transaction_update_count: 0,
            fail_get_transaction_priority_details_count: 0,
            fail_get_transaction_account_locks_count: 0,
            fail_finalize_block_not_found: 0,
        }
    }

    fn get_prioritization_fee(&self, slot: &Slot) -> Option<&PrioritizationFee> {
        self.cache.get(slot)
    }

    fn get_mut_prioritization_fee(&mut self, slot: &Slot) -> Option<&mut PrioritizationFee> {
        self.cache.get_mut(slot)
    }

    fn get_or_add_mut_prioritization_fee(&mut self, slot: &Slot) -> &mut PrioritizationFee {
        self.cache
            .entry(*slot)
            .or_insert_with(PrioritizationFee::default)
    }

    fn report_metrics_for_slot(&mut self, slot: Slot) {
        // report current min fees for slot
        if let Some(block) = self.get_prioritization_fee(&slot) {
            datapoint_info!(
                "block_prioritization_fee",
                ("slot", slot as i64, i64),
                ("entity", "block", String),
                (
                    "min_prioritization_fee",
                    block.get_min_transaction_fee().unwrap_or(0) as i64,
                    i64
                ),
            );
            for (account_key, fee) in block.get_writable_account_fees() {
                datapoint_info!(
                    "block_prioritization_fee",
                    ("slot", slot as i64, i64),
                    ("entity", account_key.to_string(), String),
                    ("min_prioritization_fee", *fee as i64, i64),
                );
            }
        }
        // report and reset counter
        {
            datapoint_info!(
                "block_prioritization_fee_counters",
                ("slot", slot as i64, i64),
                (
                    "successful_transaction_update_count",
                    self.successful_transaction_update_count as i64,
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
                    "fail_finalize_block_not_found",
                    self.fail_finalize_block_not_found as i64,
                    i64
                ),
            );
            self.successful_transaction_update_count = 0;
            self.fail_get_transaction_priority_details_count = 0;
            self.fail_get_transaction_account_locks_count = 0;
            self.fail_finalize_block_not_found = 0;
        }
    }

    /// Update block's min prioritization fee with `txs`,
    /// Returns updated min prioritization fee for `slot`
    pub fn update_transactions<'a>(
        &mut self,
        slot: Slot,
        txs: impl Iterator<Item = &'a SanitizedTransaction>,
    ) -> Option<u64> {
        let updated_min_fee: Option<u64>;

        let mut successful_transaction_update_count: u64 = 0;
        let mut fail_get_transaction_priority_details_count: u64 = 0;
        let mut fail_get_transaction_account_locks_count: u64 = 0;

        {
            let block_prioritization_fee = self.get_or_add_mut_prioritization_fee(&slot);

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

            updated_min_fee = block_prioritization_fee.get_min_transaction_fee();
        }

        saturating_add_assign!(
            self.successful_transaction_update_count,
            successful_transaction_update_count
        );
        saturating_add_assign!(
            self.fail_get_transaction_priority_details_count,
            fail_get_transaction_priority_details_count
        );
        saturating_add_assign!(
            self.fail_get_transaction_account_locks_count,
            fail_get_transaction_account_locks_count
        );

        updated_min_fee
    }

    /// bank is completely replayed from blockstore, prune irrelevant accounts to save space,
    /// its fee stats can be made available to queries
    pub fn finalize_block(&mut self, slot: Slot) {
        if let Some(block) = self.get_mut_prioritization_fee(&slot) {
            block.prune_irrelevant_writable_accounts();
            let _ = block.mark_block_completed();
        } else {
            saturating_add_assign!(self.fail_finalize_block_not_found, 1);
        }

        self.report_metrics_for_slot(slot);
    }

    /// Returns number of blocks that have finalized min fees collection
    pub fn available_block_count(&self) -> usize {
        self.cache
            .iter()
            .filter(|(_slot, min_prioritization_fee)| min_prioritization_fee.is_finalized())
            .count()
    }

    /// Query block minimum fees from finalized blocks in cache,
    /// Returns a vector of fee; call site can use it to produce
    /// average, or top 5% etc.
    pub fn get_prioritization_fees(&self) -> Vec<u64> {
        self.cache
            .iter()
            .filter_map(|(_slot, min_prioritization_fee)| {
                min_prioritization_fee
                    .is_finalized()
                    .then(|| min_prioritization_fee.get_min_transaction_fee())
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
            .filter_map(|(_slot, min_prioritization_fee)| {
                min_prioritization_fee
                    .is_finalized()
                    .then(|| min_prioritization_fee.get_writable_account_fee(account_key))
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
                .get_prioritization_fee(&slot)
                .unwrap();
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
                .get_prioritization_fee(&100)
                .is_none());
        }

        // assert after prune, account a and c should be removed from cache to save space
        {
            prioritization_fee_cache.finalize_block(slot);
            let fee = prioritization_fee_cache
                .get_prioritization_fee(&slot)
                .unwrap();
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
            .get_or_add_mut_prioritization_fee(&1)
            .mark_block_completed()
            .is_ok());
        assert!(prioritization_fee_cache
            .get_or_add_mut_prioritization_fee(&2)
            .mark_block_completed()
            .is_ok());
        prioritization_fee_cache.get_or_add_mut_prioritization_fee(&3);

        assert_eq!(2, prioritization_fee_cache.available_block_count());
    }

    fn assert_vec_eq(expected: &mut Vec<u64>, actual: &mut Vec<u64>) {
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_get_prioritization_fee() {
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
            prioritization_fee_cache.finalize_block(1);
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
            prioritization_fee_cache.finalize_block(2);
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
            prioritization_fee_cache.finalize_block(3);
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
            prioritization_fee_cache.finalize_block(1);
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
            prioritization_fee_cache.finalize_block(2);
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
            prioritization_fee_cache.finalize_block(3);
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
