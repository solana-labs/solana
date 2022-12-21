use {
    solana_measure::measure,
    solana_sdk::{clock::Slot, pubkey::Pubkey, saturating_add_assign},
    std::collections::HashMap,
};

#[derive(Debug, Default)]
struct PrioritizationFeeMetrics {
    // Count of writable accounts in slot
    total_writable_accounts_count: u64,

    // Count of writeable accounts with a minimum prioritization fee higher than the minimum transaction
    // fee for this slot.
    relevant_writable_accounts_count: u64,

    // Count of transactions that have non-zero prioritization fee.
    prioritized_transactions_count: u64,

    // Count of transactions that have zero prioritization fee.
    non_prioritized_transactions_count: u64,

    // Total prioritization fees included in this slot.
    total_prioritization_fee: u64,

    // The minimum prioritization fee of prioritized transactions in this slot.
    min_prioritization_fee: Option<u64>,

    // The maximum prioritization fee of prioritized transactions in this slot.
    max_prioritization_fee: u64,

    // Accumulated time spent on tracking prioritization fee for each slot.
    total_update_elapsed_us: u64,
}

impl PrioritizationFeeMetrics {
    fn accumulate_total_prioritization_fee(&mut self, val: u64) {
        saturating_add_assign!(self.total_prioritization_fee, val);
    }

    fn accumulate_total_update_elapsed_us(&mut self, val: u64) {
        saturating_add_assign!(self.total_update_elapsed_us, val);
    }

    fn update_prioritization_fee(&mut self, fee: u64) {
        if fee == 0 {
            saturating_add_assign!(self.non_prioritized_transactions_count, 1);
            return;
        }

        // update prioritized transaction fee metrics.
        saturating_add_assign!(self.prioritized_transactions_count, 1);

        self.max_prioritization_fee = self.max_prioritization_fee.max(fee);

        self.min_prioritization_fee = Some(
            self.min_prioritization_fee
                .map_or(fee, |min_fee| min_fee.min(fee)),
        );
    }

    fn report(&self, slot: Slot) {
        datapoint_info!(
            "block_prioritization_fee",
            ("slot", slot as i64, i64),
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
                "prioritized_transactions_count",
                self.prioritized_transactions_count as i64,
                i64
            ),
            (
                "non_prioritized_transactions_count",
                self.non_prioritized_transactions_count as i64,
                i64
            ),
            (
                "total_prioritization_fee",
                self.total_prioritization_fee as i64,
                i64
            ),
            (
                "min_prioritization_fee",
                self.min_prioritization_fee.unwrap_or(0) as i64,
                i64
            ),
            (
                "max_prioritization_fee",
                self.max_prioritization_fee as i64,
                i64
            ),
            (
                "total_update_elapsed_us",
                self.total_update_elapsed_us as i64,
                i64
            ),
        );
    }
}

pub enum PrioritizationFeeError {
    // Not able to get account locks from sanitized transaction, which is required to update block
    // minimum fees.
    FailGetTransactionAccountLocks,

    // Not able to read priority details, including compute-unit price, from transaction.
    // Compute-unit price is required to update block minimum fees.
    FailGetTransactionPriorityDetails,

    // Block is already finalized, trying to finalize it again is usually unexpected
    BlockIsAlreadyFinalized,
}

/// Block minimum prioritization fee stats, includes the minimum prioritization fee for a transaction in this
/// block; and the minimum fee for each writable account in all transactions in this block. The only relevant
/// write account minimum fees are those greater than the block minimum transaction fee, because the minimum fee needed to land
/// a transaction is determined by Max( min_transaction_fee, min_writable_account_fees(key), ...)
#[derive(Debug)]
pub struct PrioritizationFee {
    // The minimum prioritization fee of transactions that landed in this block.
    min_transaction_fee: u64,

    // The minimum prioritization fee of each writable account in transactions in this block.
    min_writable_account_fees: HashMap<Pubkey, u64>,

    // Default to `false`, set to `true` when a block is completed, therefore the minimum fees recorded
    // are finalized, and can be made available for use (e.g., RPC query)
    is_finalized: bool,

    // slot prioritization fee metrics
    metrics: PrioritizationFeeMetrics,
}

impl Default for PrioritizationFee {
    fn default() -> Self {
        PrioritizationFee {
            min_transaction_fee: u64::MAX,
            min_writable_account_fees: HashMap::new(),
            is_finalized: false,
            metrics: PrioritizationFeeMetrics::default(),
        }
    }
}

impl PrioritizationFee {
    /// Update self for minimum transaction fee in the block and minimum fee for each writable account.
    pub fn update(
        &mut self,
        transaction_fee: u64,
        writable_accounts: &[Pubkey],
    ) -> Result<(), PrioritizationFeeError> {
        let (_, update_time) = measure!(
            {
                if transaction_fee < self.min_transaction_fee {
                    self.min_transaction_fee = transaction_fee;
                }

                for write_account in writable_accounts.iter() {
                    self.min_writable_account_fees
                        .entry(*write_account)
                        .and_modify(|write_lock_fee| {
                            *write_lock_fee = std::cmp::min(*write_lock_fee, transaction_fee)
                        })
                        .or_insert(transaction_fee);
                }

                self.metrics
                    .accumulate_total_prioritization_fee(transaction_fee);
                self.metrics.update_prioritization_fee(transaction_fee);
            },
            "update_time",
        );

        self.metrics
            .accumulate_total_update_elapsed_us(update_time.as_us());
        Ok(())
    }

    /// Accounts that have minimum fees lesser or equal to the minimum fee in the block are redundant, they are
    /// removed to reduce memory footprint when mark_block_completed() is called.
    fn prune_irrelevant_writable_accounts(&mut self) {
        self.metrics.total_writable_accounts_count = self.get_writable_accounts_count() as u64;
        self.min_writable_account_fees
            .retain(|_, account_fee| account_fee > &mut self.min_transaction_fee);
        self.metrics.relevant_writable_accounts_count = self.get_writable_accounts_count() as u64;
    }

    pub fn mark_block_completed(&mut self) -> Result<(), PrioritizationFeeError> {
        if self.is_finalized {
            return Err(PrioritizationFeeError::BlockIsAlreadyFinalized);
        }
        self.prune_irrelevant_writable_accounts();
        self.is_finalized = true;
        Ok(())
    }

    pub fn get_min_transaction_fee(&self) -> Option<u64> {
        (self.min_transaction_fee != u64::MAX).then_some(self.min_transaction_fee)
    }

    pub fn get_writable_account_fee(&self, key: &Pubkey) -> Option<u64> {
        self.min_writable_account_fees.get(key).copied()
    }

    pub fn get_writable_account_fees(&self) -> impl Iterator<Item = (&Pubkey, &u64)> {
        self.min_writable_account_fees.iter()
    }

    pub fn get_writable_accounts_count(&self) -> usize {
        self.min_writable_account_fees.len()
    }

    pub fn is_finalized(&self) -> bool {
        self.is_finalized
    }

    pub fn report_metrics(&self, slot: Slot) {
        self.metrics.report(slot);

        // report this slot's min_transaction_fee and top 10 min_writable_account_fees
        let min_transaction_fee = self.get_min_transaction_fee().unwrap_or(0);
        let mut accounts_fees: Vec<_> = self.get_writable_account_fees().collect();
        accounts_fees.sort_by(|lh, rh| rh.1.cmp(lh.1));
        datapoint_info!(
            "block_min_prioritization_fee",
            ("slot", slot as i64, i64),
            ("entity", "block", String),
            ("min_prioritization_fee", min_transaction_fee as i64, i64),
        );
        for (account_key, fee) in accounts_fees.iter().take(10) {
            datapoint_info!(
                "block_min_prioritization_fee",
                ("slot", slot as i64, i64),
                ("entity", account_key.to_string(), String),
                ("min_prioritization_fee", **fee as i64, i64),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::pubkey::Pubkey};

    #[test]
    fn test_update_prioritization_fee() {
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        let mut prioritization_fee = PrioritizationFee::default();
        assert!(prioritization_fee.get_min_transaction_fee().is_none());

        // Assert for 1st transaction
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [5,   a, b             ]  -->  [5,     5,         5,         nil      ]
        {
            assert!(prioritization_fee
                .update(5, &[write_account_a, write_account_b])
                .is_ok());
            assert_eq!(5, prioritization_fee.get_min_transaction_fee().unwrap());
            assert_eq!(
                5,
                prioritization_fee
                    .get_writable_account_fee(&write_account_a)
                    .unwrap()
            );
            assert_eq!(
                5,
                prioritization_fee
                    .get_writable_account_fee(&write_account_b)
                    .unwrap()
            );
            assert!(prioritization_fee
                .get_writable_account_fee(&write_account_c)
                .is_none());
        }

        // Assert for second transaction:
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [9,      b, c          ]  -->  [5,     5,         5,         9        ]
        {
            assert!(prioritization_fee
                .update(9, &[write_account_b, write_account_c])
                .is_ok());
            assert_eq!(5, prioritization_fee.get_min_transaction_fee().unwrap());
            assert_eq!(
                5,
                prioritization_fee
                    .get_writable_account_fee(&write_account_a)
                    .unwrap()
            );
            assert_eq!(
                5,
                prioritization_fee
                    .get_writable_account_fee(&write_account_b)
                    .unwrap()
            );
            assert_eq!(
                9,
                prioritization_fee
                    .get_writable_account_fee(&write_account_c)
                    .unwrap()
            );
        }

        // Assert for third transaction:
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [2,   a,    c          ]  -->  [2,     2,         5,         2        ]
        {
            assert!(prioritization_fee
                .update(2, &[write_account_a, write_account_c])
                .is_ok());
            assert_eq!(2, prioritization_fee.get_min_transaction_fee().unwrap());
            assert_eq!(
                2,
                prioritization_fee
                    .get_writable_account_fee(&write_account_a)
                    .unwrap()
            );
            assert_eq!(
                5,
                prioritization_fee
                    .get_writable_account_fee(&write_account_b)
                    .unwrap()
            );
            assert_eq!(
                2,
                prioritization_fee
                    .get_writable_account_fee(&write_account_c)
                    .unwrap()
            );
        }

        // assert after prune, account a and c should be removed from cache to save space
        {
            prioritization_fee.prune_irrelevant_writable_accounts();
            assert_eq!(1, prioritization_fee.min_writable_account_fees.len());
            assert_eq!(2, prioritization_fee.get_min_transaction_fee().unwrap());
            assert!(prioritization_fee
                .get_writable_account_fee(&write_account_a)
                .is_none());
            assert_eq!(
                5,
                prioritization_fee
                    .get_writable_account_fee(&write_account_b)
                    .unwrap()
            );
            assert!(prioritization_fee
                .get_writable_account_fee(&write_account_c)
                .is_none());
        }
    }

    #[test]
    fn test_mark_block_completed() {
        let mut prioritization_fee = PrioritizationFee::default();

        assert!(prioritization_fee.mark_block_completed().is_ok());
        assert!(prioritization_fee.mark_block_completed().is_err());
    }
}
