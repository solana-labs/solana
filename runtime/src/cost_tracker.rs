//! `cost_tracker` keeps tracking transaction cost per chained accounts as well as for entire block
//! The main functions are:
//! - would_transaction_fit(&tx_cost), immutable function to test if tx with tx_cost would fit into current block
//! - add_transaction_cost(&tx_cost), mutable function to accumulate tx_cost to tracker.
//!
use {
    crate::{block_cost_limits::*, cost_model::TransactionCost},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::collections::HashSet,
};

#[derive(Debug, Clone)]
pub enum CostTrackerError {
    /// would exceed block max limit
    WouldExceedBlockMaxLimit,

    /// would exceed account max limit
    WouldExceedAccountMaxLimit,
}

#[derive(Debug, Default)]
struct ChainedCost {
    read_keys: HashSet<Pubkey>,
    write_keys: HashSet<Pubkey>,
    current_batch_cost: u64,
    total_chained_cost: u64,
}

impl ChainedCost {
    fn has_conflicts(&self, read_keys: &[Pubkey], write_keys: &[Pubkey]) -> bool {
        for account_key in read_keys.iter() {
            if self.write_keys.contains(account_key) {
                return true;
            }
        }
        for account_key in write_keys.iter() {
            if self.read_keys.contains(account_key) || self.write_keys.contains(account_key) {
                return true;
            }
        }
        false
    }

    fn add_new_batch(&mut self, read_keys: &[Pubkey], write_keys: &[Pubkey], cost: u64) {
        if self.has_conflicts(read_keys, write_keys) {
            self.total_chained_cost += self.current_batch_cost;
            self.current_batch_cost = cost;
            self.read_keys.clear();
            self.write_keys.clear();
        }
        self.current_batch_cost = std::cmp::max(self.current_batch_cost, cost);
        for key in read_keys.iter() {
            self.read_keys.insert(*key);
        }
        for key in write_keys.iter() {
            self.write_keys.insert(*key);
        }
    }
}

#[derive(AbiExample, Debug)]
pub struct CostTracker {
    account_cost_limit: u64,
    block_cost_limit: u64,
    chained_cost: ChainedCost,
    block_cost: u64,
    transaction_count: u64,
}

impl Default for CostTracker {
    fn default() -> Self {
        CostTracker::new(MAX_WRITABLE_ACCOUNT_UNITS, MAX_BLOCK_UNITS)
    }
}

impl CostTracker {
    pub fn new(account_cost_limit: u64, block_cost_limit: u64) -> Self {
        assert!(account_cost_limit <= block_cost_limit);
        Self {
            account_cost_limit,
            block_cost_limit,
            block_cost: 0,
            transaction_count: 0,
            chained_cost: ChainedCost::default(),
        }
    }

    // bench tests needs to reset limits
    pub fn set_limits(&mut self, account_cost_limit: u64, block_cost_limit: u64) {
        self.account_cost_limit = account_cost_limit;
        self.block_cost_limit = block_cost_limit;
    }

    pub fn would_transaction_fit(&self, tx_cost: &TransactionCost) -> Result<(), CostTrackerError> {
        self.would_fit(
            &tx_cost.readable_accounts,
            &tx_cost.writable_accounts,
            tx_cost.sum(),
        )
    }

    pub fn add_transaction_cost(&mut self, tx_cost: &TransactionCost) {
        self.add_transaction(
            &tx_cost.readable_accounts,
            &tx_cost.writable_accounts,
            tx_cost.sum(),
        );
    }

    pub fn try_add(&mut self, tx_cost: &TransactionCost) -> Result<u64, CostTrackerError> {
        let cost = tx_cost.sum();
        self.would_fit(&tx_cost.readable_accounts, &tx_cost.writable_accounts, cost)?;
        self.add_transaction(&tx_cost.readable_accounts, &tx_cost.writable_accounts, cost);
        Ok(self.block_cost)
    }

    pub fn report_stats(&self, bank_slot: Slot) {
        // skip reporting if block is empty
        if self.transaction_count == 0 {
            return;
        }

        datapoint_info!(
            "cost_tracker_stats",
            ("bank_slot", bank_slot as i64, i64),
            ("block_cost", self.block_cost as i64, i64),
            ("transaction_count", self.transaction_count as i64, i64),
        );
    }

    fn would_fit(
        &self,
        read_keys: &[Pubkey],
        write_keys: &[Pubkey],
        cost: u64,
    ) -> Result<(), CostTrackerError> {
        // check against the total package cost
        if self.block_cost + cost > self.block_cost_limit {
            return Err(CostTrackerError::WouldExceedBlockMaxLimit);
        }

        // Check that the chained cost if a new batch is
        // made doesn't increase above the max
        if self.account_cost_limit <= self.chained_cost.total_chained_cost.saturating_add(cost)
            && self.chained_cost.has_conflicts(read_keys, write_keys)
        {
            return Err(CostTrackerError::WouldExceedAccountMaxLimit);
        }

        Ok(())
    }

    fn add_transaction(&mut self, read_keys: &[Pubkey], write_keys: &[Pubkey], cost: u64) {
        self.chained_cost.add_new_batch(read_keys, write_keys, cost);
        self.block_cost += cost;
        self.transaction_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::Bank,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            system_transaction,
            transaction::Transaction,
        },
        std::{cmp, sync::Arc},
    };

    fn test_setup() -> (Keypair, Hash) {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10);
        let bank = Arc::new(Bank::new_no_wallclock_throttle(&genesis_config));
        let start_hash = bank.last_blockhash();
        (mint_keypair, start_hash)
    }

    fn build_simple_transaction(
        mint_keypair: &Keypair,
        start_hash: &Hash,
    ) -> (Transaction, Vec<Pubkey>, Vec<Pubkey>, u64) {
        let keypair = Keypair::new();
        let simple_transaction =
            system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 2, *start_hash);

        let (write_keys, read_keys) = simple_transaction
            .message()
            .get_account_keys_by_lock_type(true);
        let write_keys = write_keys.into_iter().cloned().collect();
        let read_keys = read_keys.into_iter().cloned().collect();
        (simple_transaction, read_keys, write_keys, 5)
    }

    #[test]
    fn test_cost_tracker_initialization() {
        let testee = CostTracker::new(10, 11);
        assert_eq!(10, testee.account_cost_limit);
        assert_eq!(11, testee.block_cost_limit);
        assert_eq!(0, testee.block_cost);
    }

    #[test]
    fn test_cost_tracker_ok_add_one() {
        let (mint_keypair, start_hash) = test_setup();
        let (_tx, read_keys, write_keys, cost) =
            build_simple_transaction(&mint_keypair, &start_hash);

        // build testee to have capacity for one simple transaction
        let mut testee = CostTracker::new(cost, cost);
        assert!(testee.would_fit(&read_keys, &write_keys, cost).is_ok());
        testee.add_transaction(&read_keys, &write_keys, cost);
        assert_eq!(cost, testee.block_cost);
    }

    #[test]
    fn test_cost_tracker_ok_add_two_same_accounts() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with same signed account
        let (_tx1, read_keys1, write_keys1, cost1) =
            build_simple_transaction(&mint_keypair, &start_hash);
        let (_tx2, read_keys2, write_keys2, cost2) =
            build_simple_transaction(&mint_keypair, &start_hash);

        // build testee to have capacity for two simple transactions, with same accounts
        let mut testee = CostTracker::new(cost1 + cost2, cost1 + cost2);
        {
            assert!(testee.would_fit(&read_keys1, &write_keys1, cost1).is_ok());
            testee.add_transaction(&read_keys1, &write_keys1, cost1);
        }
        {
            assert!(testee.would_fit(&read_keys2, &write_keys2, cost2).is_ok());
            testee.add_transaction(&read_keys2, &write_keys2, cost2);
        }
        assert_eq!(cost1 + cost2, testee.block_cost);
    }

    #[test]
    fn test_cost_tracker_ok_add_two_diff_accounts() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let (_tx1, read_keys1, write_keys1, cost1) =
            build_simple_transaction(&mint_keypair, &start_hash);
        let second_account = Keypair::new();
        let (_tx2, read_keys2, write_keys2, cost2) =
            build_simple_transaction(&second_account, &start_hash);

        // build testee to have capacity for two simple transactions, with same accounts
        let mut testee = CostTracker::new(cmp::max(cost1, cost2), cost1 + cost2);
        {
            assert!(testee.would_fit(&read_keys1, &write_keys1, cost1).is_ok());
            testee.add_transaction(&read_keys1, &write_keys1, cost1);
        }
        {
            assert!(testee.would_fit(&read_keys2, &write_keys2, cost2).is_ok());
            testee.add_transaction(&read_keys2, &write_keys2, cost2);
        }
        assert_eq!(cost1 + cost2, testee.block_cost);
    }

    #[test]
    fn test_cost_tracker_chain_reach_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with same signed account
        let (_tx1, read_keys1, write_keys1, cost1) =
            build_simple_transaction(&mint_keypair, &start_hash);
        let (_tx2, read_keys2, write_keys2, cost2) =
            build_simple_transaction(&mint_keypair, &start_hash);

        // build testee to have capacity for two simple transactions, but not for same accounts
        let mut testee = CostTracker::new(cmp::min(cost1, cost2), cost1 + cost2);
        // should have room for first transaction
        {
            assert!(testee.would_fit(&read_keys1, &write_keys1, cost1).is_ok());
            testee.add_transaction(&read_keys1, &write_keys1, cost1);
        }
        // but no more sapce on the same chain (same signer account)
        {
            assert!(testee.would_fit(&read_keys2, &write_keys2, cost2).is_err());
        }
    }

    #[test]
    fn test_cost_tracker_reach_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let (_tx1, read_keys1, write_keys1, cost1) =
            build_simple_transaction(&mint_keypair, &start_hash);
        let second_account = Keypair::new();
        let (_tx2, read_keys2, write_keys2, cost2) =
            build_simple_transaction(&second_account, &start_hash);

        // build testee to have capacity for each chain, but not enough room for both transactions
        let mut testee = CostTracker::new(cmp::max(cost1, cost2), cost1 + cost2 - 1);
        // should have room for first transaction
        {
            assert!(testee.would_fit(&read_keys1, &write_keys1, cost1).is_ok());
            testee.add_transaction(&read_keys1, &write_keys1, cost1);
        }
        // but no more room for package as whole
        {
            assert!(testee.would_fit(&read_keys2, &write_keys2, cost2).is_err());
        }
    }

    #[test]
    fn test_cost_tracker_try_add_is_atomic() {
        let (mint_keypair, start_hash) = test_setup();
        let (_tx, _read_keys, _write_keys, _cost) =
            build_simple_transaction(&mint_keypair, &start_hash);

        let acct1 = Pubkey::new_unique();
        let acct2 = Pubkey::new_unique();
        let acct3 = Pubkey::new_unique();
        let cost = 100;
        let account_max = cost * 2;
        let block_max = account_max * 3; // for three accts

        let mut testee = CostTracker::new(account_max, block_max);

        // case 1: a tx writes to 3 accounts, should success, we will have:
        // | acct1 | $cost |
        // | acct2 | $cost |
        // | acct3 | $cost |
        // and block_cost = $cost
        {
            let tx_cost = TransactionCost {
                writable_accounts: vec![acct1, acct2, acct3],
                execution_cost: cost,
                ..TransactionCost::default()
            };
            assert!(testee.try_add(&tx_cost).is_ok());
            assert_eq!(cost, testee.block_cost);
        }

        // case 2: add tx writes to acct2 with $cost, should succeed, result to
        // | acct1 | $cost |
        // | acct2 | $cost * 2 |
        // | acct3 | $cost |
        // and block_cost = $cost * 2
        {
            let tx_cost = TransactionCost {
                writable_accounts: vec![acct2],
                execution_cost: cost,
                ..TransactionCost::default()
            };
            assert!(testee.try_add(&tx_cost).is_ok());
            assert_eq!(cost * 2, testee.block_cost);
        }

        // case 3: add tx writes to [acct1, acct2], acct2 exceeds limit, should failed atomically,
        // we shoudl still have:
        // | acct1 | $cost |
        // | acct2 | $cost * 2 |
        // | acct3 | $cost |
        // and block_cost = $cost * 2
        {
            let tx_cost = TransactionCost {
                writable_accounts: vec![acct1, acct2],
                execution_cost: cost,
                ..TransactionCost::default()
            };
            assert!(testee.try_add(&tx_cost).is_err());
            assert_eq!(cost * 2, testee.block_cost);
        }
    }
}
