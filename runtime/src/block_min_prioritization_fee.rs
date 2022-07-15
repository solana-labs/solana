use {
    crate::transaction_priority_details::GetTransactionPriorityDetails,
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    std::collections::HashMap,
};

pub enum BlockMinPrioritizationFeeError {
    // Not able to get account locks from sanitized transaction, which is required to update block
    // min fees.
    FailGetTransactionAccountLocks,

    // Not able to read priority details, including compute-unit price, from transaction.
    // Compute-unit price is required to update block min fees.
    FailGetTransactionPriorityDetails,

    // Block is already finalized, trying to finalize it again is usually unexpected
    BlockIsAlreadyFinalized,
}

/// Block min prioritization fee stats, includes the min prioritization fee of transactions in a
/// block; and min fee for each writable accounts of all transactions in block. The only relevant
/// write account min fees are those greater than block min fee, because the min fee needed to land
/// a transaction is determined by Max( min_block_fee, min_write_lock_fee(key), ...)
#[derive(Debug)]
pub struct BlockMinPrioritizationFee {
    // The min prioritization fee of transaction that landed in this block.
    block_fee: u64,

    // The min prioritization fees of writable accounts of transactions in this block.
    write_lock_fees: HashMap<Pubkey, u64>,

    // Default to `false`, set to `true` when a block is completed, therefore the min fees recorded
    // are finalized, and can be made available for use (e.g., RPC query)
    is_finalized: bool,
}

impl Default for BlockMinPrioritizationFee {
    fn default() -> Self {
        BlockMinPrioritizationFee {
            block_fee: u64::MAX,
            write_lock_fees: HashMap::new(),
            is_finalized: false,
        }
    }
}

impl BlockMinPrioritizationFee {
    /// Use `sanitized_tx` to update self for min block fee and min fee for all writable accounts in
    /// transaction.
    pub fn update_for_transaction(
        &mut self,
        sanitized_tx: &SanitizedTransaction,
    ) -> Result<(), BlockMinPrioritizationFeeError> {
        let account_locks = sanitized_tx.get_account_locks().or(Err(
            BlockMinPrioritizationFeeError::FailGetTransactionAccountLocks,
        ))?;

        let priority_details = sanitized_tx
            .get_transaction_priority_details()
            .ok_or(BlockMinPrioritizationFeeError::FailGetTransactionPriorityDetails)?;

        self.block_fee = std::cmp::min(self.block_fee, priority_details.priority);
        for write_account in account_locks.writable {
            let write_lock_fee = self
                .write_lock_fees
                .entry(*write_account)
                .or_insert(u64::MAX);
            *write_lock_fee = std::cmp::min(*write_lock_fee, priority_details.priority);
        }
        Ok(())
    }

    /// Accounts that have its min fee lesser or equal to block min fee are redundant, they are
    /// removed to reduce memory footprint.
    pub fn prune_irrelevant_accounts(&mut self) {
        self.write_lock_fees
            .retain(|_, account_fee| account_fee > &mut self.block_fee);
    }

    pub fn mark_block_completed(&mut self) -> Result<(), BlockMinPrioritizationFeeError> {
        (!self.is_finalized)
            .then(|| {
                self.is_finalized = true;
            })
            .ok_or(BlockMinPrioritizationFeeError::BlockIsAlreadyFinalized)
    }

    pub fn get_block_fee(&self) -> Option<u64> {
        (self.block_fee != u64::MAX).then(|| self.block_fee)
    }

    pub fn get_account_fee(&self, key: &Pubkey) -> Option<u64> {
        self.write_lock_fees.get(key).copied()
    }

    pub fn is_finalized(&self) -> bool {
        self.is_finalized
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
    fn test_update_block_min_prioritization_fee() {
        solana_logger::setup();
        let write_account_a = Pubkey::new_unique();
        let write_account_b = Pubkey::new_unique();
        let write_account_c = Pubkey::new_unique();

        let mut block_min_prioritization_fee = BlockMinPrioritizationFee::default();
        assert!(block_min_prioritization_fee.get_block_fee().is_none());

        // Assert for 1st transaction
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [5,   a, b             ]  -->  [5,     5,         5,         nil      ]
        {
            let tx = build_sanitized_transaction_for_test(5, &write_account_a, &write_account_b);
            assert!(block_min_prioritization_fee
                .update_for_transaction(&tx)
                .is_ok());
            assert_eq!(5, block_min_prioritization_fee.get_block_fee().unwrap());
            assert_eq!(
                5,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_a)
                    .unwrap()
            );
            assert_eq!(
                5,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_b)
                    .unwrap()
            );
            assert!(block_min_prioritization_fee
                .get_account_fee(&write_account_c)
                .is_none());
        }

        // Assert for second transaction:
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [9,      b, c          ]  -->  [5,     5,         5,         9        ]
        {
            let tx = build_sanitized_transaction_for_test(9, &write_account_b, &write_account_c);
            assert!(block_min_prioritization_fee
                .update_for_transaction(&tx)
                .is_ok());
            assert_eq!(5, block_min_prioritization_fee.get_block_fee().unwrap());
            assert_eq!(
                5,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_a)
                    .unwrap()
            );
            assert_eq!(
                5,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_b)
                    .unwrap()
            );
            assert_eq!(
                9,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_c)
                    .unwrap()
            );
        }

        // Assert for third transaction:
        // [fee, write_accounts...]  -->  [block, account_a, account_b, account_c]
        // -----------------------------------------------------------------------
        // [2,   a,    c          ]  -->  [2,     2,         5,         2        ]
        {
            let tx = build_sanitized_transaction_for_test(2, &write_account_a, &write_account_c);
            assert!(block_min_prioritization_fee
                .update_for_transaction(&tx)
                .is_ok());
            assert_eq!(2, block_min_prioritization_fee.get_block_fee().unwrap());
            assert_eq!(
                2,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_a)
                    .unwrap()
            );
            assert_eq!(
                5,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_b)
                    .unwrap()
            );
            assert_eq!(
                2,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_c)
                    .unwrap()
            );
        }

        // assert after prune, account a and c should be removed from cache to save space
        {
            block_min_prioritization_fee.prune_irrelevant_accounts();
            assert_eq!(1, block_min_prioritization_fee.write_lock_fees.len());
            assert_eq!(2, block_min_prioritization_fee.get_block_fee().unwrap());
            assert!(block_min_prioritization_fee
                .get_account_fee(&write_account_a)
                .is_none());
            assert_eq!(
                5,
                block_min_prioritization_fee
                    .get_account_fee(&write_account_b)
                    .unwrap()
            );
            assert!(block_min_prioritization_fee
                .get_account_fee(&write_account_c)
                .is_none());
        }
    }

    #[test]
    fn test_mark_block_completed() {
        let mut block_min_prioritization_fee = BlockMinPrioritizationFee::default();

        assert!(block_min_prioritization_fee.mark_block_completed().is_ok());
        assert!(block_min_prioritization_fee.mark_block_completed().is_err());
    }
}
