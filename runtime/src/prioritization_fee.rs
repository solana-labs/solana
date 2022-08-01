use {
    crate::transaction_priority_details::GetTransactionPriorityDetails,
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    std::collections::HashMap,
};

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

/// Block minimum prioritization fee stats, includes the minimum prioritization fee of transactions in a
/// block; and minimum fee for each writable accounts of all transactions in block. The only relevant
/// write account minimum fees are those greater than block minimum fee, because the minimum fee needed to land
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
}

impl Default for PrioritizationFee {
    fn default() -> Self {
        PrioritizationFee {
            min_transaction_fee: u64::MAX,
            min_writable_account_fees: HashMap::new(),
            is_finalized: false,
        }
    }
}

impl PrioritizationFee {
    /// Use `sanitized_tx` to update self for minimum transaction fee in the block and minimum fee for each writable account.
    pub fn update(
        &mut self,
        sanitized_tx: &SanitizedTransaction,
    ) -> Result<(), PrioritizationFeeError> {
        let account_locks = sanitized_tx
            .get_account_locks()
            .or(Err(PrioritizationFeeError::FailGetTransactionAccountLocks))?;

        let priority_details = sanitized_tx
            .get_transaction_priority_details()
            .ok_or(PrioritizationFeeError::FailGetTransactionPriorityDetails)?;

        if priority_details.priority < self.min_transaction_fee {
            self.min_transaction_fee = priority_details.priority;
        }
        for write_account in account_locks.writable {
            self.min_writable_account_fees
                .entry(*write_account)
                .and_modify(|write_lock_fee| {
                    *write_lock_fee = std::cmp::min(*write_lock_fee, priority_details.priority)
                })
                .or_insert(priority_details.priority);
        }
        Ok(())
    }

    /// Accounts that have minimum fees lesser or equal to the minimum fee in the block are redundant, they are
    /// removed to reduce memory footprint.
    pub fn prune_irrelevant_writable_accounts(&mut self) {
        self.min_writable_account_fees
            .retain(|_, account_fee| account_fee > &mut self.min_transaction_fee);
    }

    pub fn mark_block_completed(&mut self) -> Result<(), PrioritizationFeeError> {
        if self.is_finalized {
            return Err(PrioritizationFeeError::BlockIsAlreadyFinalized);
        }
        self.is_finalized = true;
        Ok(())
    }

    pub fn get_min_transaction_fee(&self) -> Option<u64> {
        (self.min_transaction_fee != u64::MAX).then(|| self.min_transaction_fee)
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
            let tx = build_sanitized_transaction_for_test(5, &write_account_a, &write_account_b);
            assert!(prioritization_fee.update(&tx).is_ok());
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
            let tx = build_sanitized_transaction_for_test(9, &write_account_b, &write_account_c);
            assert!(prioritization_fee.update(&tx).is_ok());
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
            let tx = build_sanitized_transaction_for_test(2, &write_account_a, &write_account_c);
            assert!(prioritization_fee.update(&tx).is_ok());
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
