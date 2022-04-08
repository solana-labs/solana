//! `cost_tracker` keeps tracking transaction cost per chained accounts as well as for entire block
//! The main functions are:
//! - would_fit(&tx_cost), immutable function to test if tx with tx_cost would fit into current block
//! - add_transaction_cost(&tx_cost), mutable function to accumulate tx_cost to tracker.
//!
use {
    crate::{block_cost_limits::*, cost_model::TransactionCost},
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::collections::HashMap,
};

const WRITABLE_ACCOUNTS_PER_BLOCK: usize = 512;

#[derive(Debug, Clone)]
pub enum CostTrackerError {
    /// would exceed block max limit
    WouldExceedBlockMaxLimit,

    /// would exceed vote max limit
    WouldExceedVoteMaxLimit,

    /// would exceed account max limit
    WouldExceedAccountMaxLimit,
}

#[derive(AbiExample, Debug)]
pub struct CostTracker {
    account_cost_limit: u64,
    block_cost_limit: u64,
    vote_cost_limit: u64,
    cost_by_writable_accounts: HashMap<Pubkey, u64>,
    block_cost: u64,
    vote_cost: u64,
    transaction_count: u64,
<<<<<<< HEAD
=======
    account_data_size: u64,

    /// The amount of total account data size remaining.  If `Some`, then do not add transactions
    /// that would cause `account_data_size` to exceed this limit.
    account_data_size_limit: Option<u64>,
<<<<<<< HEAD

    // Transactions have passed would_fit check, is being executed.
    // If the execution is successful, it's actual Units can be committed
    // to cost_tracker; otherwise, it should be removed without impacting
    // cost_tracker.
    pending_transactions: HashMap<Signature, TransactionCost>,
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
=======
>>>>>>> 924b8ea1e (Adjustments to cost_tracker updates)
}

impl Default for CostTracker {
    fn default() -> Self {
<<<<<<< HEAD
        CostTracker::new(MAX_WRITABLE_ACCOUNT_UNITS, MAX_BLOCK_UNITS, MAX_VOTE_UNITS)
=======
        // Clippy doesn't like asserts in const contexts, so need to explicitly allow them.  For
        // more info, see this issue: https://github.com/rust-lang/rust-clippy/issues/8159
        #![allow(clippy::assertions_on_constants)]
        const _: () = assert!(MAX_WRITABLE_ACCOUNT_UNITS <= MAX_BLOCK_UNITS);
        const _: () = assert!(MAX_VOTE_UNITS <= MAX_BLOCK_UNITS);

        Self {
            account_cost_limit: MAX_WRITABLE_ACCOUNT_UNITS,
            block_cost_limit: MAX_BLOCK_UNITS,
            vote_cost_limit: MAX_VOTE_UNITS,
            cost_by_writable_accounts: HashMap::with_capacity(WRITABLE_ACCOUNTS_PER_BLOCK),
            block_cost: 0,
            vote_cost: 0,
            transaction_count: 0,
            account_data_size: 0,
            account_data_size_limit: None,
        }
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
    }
}

impl CostTracker {
    pub fn new(account_cost_limit: u64, block_cost_limit: u64, vote_cost_limit: u64) -> Self {
        assert!(account_cost_limit <= block_cost_limit);
        assert!(vote_cost_limit <= block_cost_limit);
        Self {
            account_cost_limit,
            block_cost_limit,
            vote_cost_limit,
            cost_by_writable_accounts: HashMap::with_capacity(WRITABLE_ACCOUNTS_PER_BLOCK),
            block_cost: 0,
            vote_cost: 0,
            transaction_count: 0,
        }
    }

    // bench tests needs to reset limits
    pub fn set_limits(
        &mut self,
        account_cost_limit: u64,
        block_cost_limit: u64,
        vote_cost_limit: u64,
    ) {
        self.account_cost_limit = account_cost_limit;
        self.block_cost_limit = block_cost_limit;
        self.vote_cost_limit = vote_cost_limit;
    }

<<<<<<< HEAD
<<<<<<< HEAD
    pub fn would_transaction_fit(
        &self,
        transaction: &SanitizedTransaction,
        tx_cost: &TransactionCost,
    ) -> Result<(), CostTrackerError> {
        self.would_fit(&tx_cost.writable_accounts, tx_cost.sum(), transaction)
    }

    pub fn add_transaction_cost(
        &mut self,
        transaction: &SanitizedTransaction,
        tx_cost: &TransactionCost,
    ) {
        self.add_transaction(&tx_cost.writable_accounts, tx_cost.sum(), transaction);
    }

=======
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
    pub fn try_add(
        &mut self,
        _transaction: &SanitizedTransaction,
        tx_cost: &TransactionCost,
    ) -> Result<u64, CostTrackerError> {
<<<<<<< HEAD
        let cost = tx_cost.sum();
        self.would_fit(&tx_cost.writable_accounts, cost, transaction)?;
        self.add_transaction(&tx_cost.writable_accounts, cost, transaction);
=======
=======
    pub fn try_add(&mut self, tx_cost: &TransactionCost) -> Result<u64, CostTrackerError> {
>>>>>>> a058f348a (Address review comments)
        self.would_fit(tx_cost)?;
<<<<<<< HEAD
        self.pending_transactions
            .insert(*transaction.signature(), tx_cost.clone());
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
=======
        self.add_transaction_cost(tx_cost);
>>>>>>> 924b8ea1e (Adjustments to cost_tracker updates)
        Ok(self.block_cost)
    }

    pub fn update_execution_cost(
        &mut self,
        _estimated_tx_cost: &TransactionCost,
        _actual_execution_cost: u64,
    ) {
        // TODO: adjust block_cost / vote_cost / account_cost by (actual_execution_cost - execution_cost)
    }

    pub fn remove(&mut self, tx_cost: &TransactionCost) {
        self.remove_transaction_cost(tx_cost);
    }

    pub fn block_cost(&self) -> u64 {
        self.block_cost
    }

    pub fn transaction_count(&self) -> u64 {
        self.transaction_count
    }

    pub fn report_stats(&self, bank_slot: Slot) {
        // skip reporting if block is empty
        if self.transaction_count == 0 {
            return;
        }

        let (costliest_account, costliest_account_cost) = self.find_costliest_account();

        datapoint_info!(
            "cost_tracker_stats",
            ("bank_slot", bank_slot as i64, i64),
            ("block_cost", self.block_cost as i64, i64),
            ("vote_cost", self.vote_cost as i64, i64),
            ("transaction_count", self.transaction_count as i64, i64),
            (
                "number_of_accounts",
                self.cost_by_writable_accounts.len() as i64,
                i64
            ),
            ("costliest_account", costliest_account.to_string(), String),
            ("costliest_account_cost", costliest_account_cost as i64, i64),
        );
    }

    fn find_costliest_account(&self) -> (Pubkey, u64) {
        let mut costliest_account = Pubkey::default();
        let mut costliest_account_cost = 0;
        for (key, cost) in self.cost_by_writable_accounts.iter() {
            if *cost > costliest_account_cost {
                costliest_account = *key;
                costliest_account_cost = *cost;
            }
        }

        (costliest_account, costliest_account_cost)
    }

    fn would_fit(&self, tx_cost: &TransactionCost) -> Result<(), CostTrackerError> {
        let writable_accounts = &tx_cost.writable_accounts;
        let cost = tx_cost.sum();
        let vote_cost = if tx_cost.is_simple_vote { cost } else { 0 };

<<<<<<< HEAD
        for tx_cost in self.pending_transactions.values() {
            writable_account.extend(&tx_cost.writable_accounts);
            cost = cost.saturating_add(tx_cost.sum());
            account_data_size = account_data_size.saturating_add(tx_cost.account_data_size);
            vote_cost = vote_cost.saturating_add(if tx_cost.is_simple_vote { cost } else { 0 });
        }

        self.would_aggregated_transactions_fit(
            &writable_account,
            cost,
            account_data_size,
            vote_cost,
        )
    }

    fn would_aggregated_transactions_fit(
        &self,
        keys: &[Pubkey],
        cost: u64,
<<<<<<< HEAD
        transaction: &SanitizedTransaction,
=======
        account_data_len: u64,
        vote_cost: u64,
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
    ) -> Result<(), CostTrackerError> {
=======
>>>>>>> 924b8ea1e (Adjustments to cost_tracker updates)
        // check against the total package cost
        if self.block_cost.saturating_add(cost) > self.block_cost_limit {
            return Err(CostTrackerError::WouldExceedBlockMaxLimit);
        }

        // if vote transaction, check if it exceeds vote_transaction_limit
        if self.vote_cost.saturating_add(vote_cost) > self.vote_cost_limit {
            return Err(CostTrackerError::WouldExceedVoteMaxLimit);
        }

        // check if the transaction itself is more costly than the account_cost_limit
        if cost > self.account_cost_limit {
            return Err(CostTrackerError::WouldExceedAccountMaxLimit);
        }

<<<<<<< HEAD
=======
        // NOTE: Check if the total accounts data size is exceeded *before* the block accounts data
        // size.  This way, transactions are not unnecessarily retried.
        let account_data_size = self
            .account_data_size
            .saturating_add(tx_cost.account_data_size);
        if let Some(account_data_size_limit) = self.account_data_size_limit {
            if account_data_size > account_data_size_limit {
                return Err(CostTrackerError::WouldExceedAccountDataTotalLimit);
            }
        }

        if account_data_size > MAX_ACCOUNT_DATA_BLOCK_LEN {
            return Err(CostTrackerError::WouldExceedAccountDataBlockLimit);
        }

>>>>>>> 924b8ea1e (Adjustments to cost_tracker updates)
        // check each account against account_cost_limit,
        for account_key in writable_accounts.iter() {
            match self.cost_by_writable_accounts.get(account_key) {
                Some(chained_cost) => {
                    if chained_cost.saturating_add(cost) > self.account_cost_limit {
                        return Err(CostTrackerError::WouldExceedAccountMaxLimit);
                    } else {
                        continue;
                    }
                }
                None => continue,
            }
        }

        Ok(())
    }

<<<<<<< HEAD
<<<<<<< HEAD
    fn add_transaction(&mut self, keys: &[Pubkey], cost: u64, transaction: &SanitizedTransaction) {
        for account_key in keys.iter() {
=======
    fn add_transaction(&mut self, tx_cost: &TransactionCost) {
=======
    fn add_transaction_cost(&mut self, tx_cost: &TransactionCost) {
>>>>>>> 924b8ea1e (Adjustments to cost_tracker updates)
        let cost = tx_cost.sum();
        for account_key in tx_cost.writable_accounts.iter() {
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
            let account_cost = self
                .cost_by_writable_accounts
                .entry(*account_key)
                .or_insert(0);
            *account_cost = account_cost.saturating_add(cost);
        }
        self.block_cost = self.block_cost.saturating_add(cost);
        if tx_cost.is_simple_vote {
            self.vote_cost = self.vote_cost.saturating_add(cost);
        }
<<<<<<< HEAD
=======
        self.account_data_size = self
            .account_data_size
            .saturating_add(tx_cost.account_data_size);
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
        self.transaction_count = self.transaction_count.saturating_add(1);
    }

    fn remove_transaction_cost(&mut self, tx_cost: &TransactionCost) {
        let cost = tx_cost.sum();
        for account_key in tx_cost.writable_accounts.iter() {
            let account_cost = self
                .cost_by_writable_accounts
                .entry(*account_key)
                .or_insert(0);
            *account_cost = account_cost.saturating_sub(cost);
        }
        self.block_cost = self.block_cost.saturating_sub(cost);
        if tx_cost.is_simple_vote {
            self.vote_cost = self.vote_cost.saturating_sub(cost);
        }
        self.account_data_size = self
            .account_data_size
            .saturating_sub(tx_cost.account_data_size);
        self.transaction_count = self.transaction_count.saturating_sub(1);
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
<<<<<<< HEAD
            transaction::{TransactionError, VersionedTransaction},
=======
            transaction::{
                MessageHash, SanitizedTransaction, SimpleAddressLoader, VersionedTransaction,
            },
>>>>>>> a058f348a (Address review comments)
        },
        solana_vote_program::vote_transaction,
        std::{cmp, sync::Arc},
    };

    fn test_setup() -> (Keypair, Hash) {
        solana_logger::setup();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10);
        let bank = Arc::new(Bank::new_no_wallclock_throttle_for_tests(&genesis_config));
        let start_hash = bank.last_blockhash();
        (mint_keypair, start_hash)
    }

    fn build_simple_transaction(
        mint_keypair: &Keypair,
        start_hash: &Hash,
    ) -> (SanitizedTransaction, TransactionCost) {
        let keypair = Keypair::new();
        let simple_transaction = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(mint_keypair, &keypair.pubkey(), 2, *start_hash),
        );
        let mut tx_cost = TransactionCost::new_with_capacity(1);
        tx_cost.execution_cost = 5;
        tx_cost.writable_accounts.push(mint_keypair.pubkey());

        (simple_transaction, tx_cost)
    }

    fn build_simple_vote_transaction(
        mint_keypair: &Keypair,
        start_hash: &Hash,
    ) -> (SanitizedTransaction, TransactionCost) {
        let keypair = Keypair::new();
        let transaction = vote_transaction::new_vote_transaction(
            vec![42],
            Hash::default(),
            *start_hash,
            mint_keypair,
            &keypair,
            &keypair,
            None,
        );
        let message_hash = transaction.message.hash();
        let vote_transaction = SanitizedTransaction::try_create(
            VersionedTransaction::from(transaction),
            message_hash,
            Some(true),
            |_| Err(TransactionError::UnsupportedVersion),
        )
        .unwrap();
        let mut tx_cost = TransactionCost::new_with_capacity(1);
        tx_cost.execution_cost = 10;
        tx_cost.writable_accounts.push(mint_keypair.pubkey());
        tx_cost.is_simple_vote = true;

        (vote_transaction, tx_cost)
    }

    #[test]
    fn test_cost_tracker_initialization() {
        let testee = CostTracker::new(10, 11, 8);
        assert_eq!(10, testee.account_cost_limit);
        assert_eq!(11, testee.block_cost_limit);
        assert_eq!(8, testee.vote_cost_limit);
        assert_eq!(0, testee.cost_by_writable_accounts.len());
        assert_eq!(0, testee.block_cost);
    }

    #[test]
    fn test_cost_tracker_ok_add_one() {
        let (mint_keypair, start_hash) = test_setup();
        let (_tx, tx_cost) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost = tx_cost.sum();

        // build testee to have capacity for one simple transaction
<<<<<<< HEAD
        let mut testee = CostTracker::new(cost, cost, cost);
        assert!(testee.would_fit(&keys, cost, &tx).is_ok());
        testee.add_transaction(&keys, cost, &tx);
=======
        let mut testee = CostTracker::new(cost, cost, cost, None);
        assert!(testee.would_fit(&tx_cost).is_ok());
<<<<<<< HEAD
        testee.add_transaction(&tx_cost);
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
=======
        testee.add_transaction_cost(&tx_cost);
>>>>>>> a058f348a (Address review comments)
        assert_eq!(cost, testee.block_cost);
        assert_eq!(0, testee.vote_cost);
        let (_costliest_account, costliest_account_cost) = testee.find_costliest_account();
        assert_eq!(cost, costliest_account_cost);
    }

    #[test]
    fn test_cost_tracker_ok_add_one_vote() {
        let (mint_keypair, start_hash) = test_setup();
        let (_tx, tx_cost) = build_simple_vote_transaction(&mint_keypair, &start_hash);
        let cost = tx_cost.sum();

        // build testee to have capacity for one simple transaction
<<<<<<< HEAD
        let mut testee = CostTracker::new(cost, cost, cost);
        assert!(testee.would_fit(&keys, cost, &tx).is_ok());
        testee.add_transaction(&keys, cost, &tx);
=======
        let mut testee = CostTracker::new(cost, cost, cost, None);
        assert!(testee.would_fit(&tx_cost).is_ok());
<<<<<<< HEAD
        testee.add_transaction(&tx_cost);
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
=======
        testee.add_transaction_cost(&tx_cost);
>>>>>>> a058f348a (Address review comments)
        assert_eq!(cost, testee.block_cost);
        assert_eq!(cost, testee.vote_cost);
        let (_ccostliest_account, costliest_account_cost) = testee.find_costliest_account();
        assert_eq!(cost, costliest_account_cost);
    }

    #[test]
<<<<<<< HEAD
=======
    fn test_cost_tracker_add_data() {
        let (mint_keypair, start_hash) = test_setup();
        let (_tx, mut tx_cost) = build_simple_transaction(&mint_keypair, &start_hash);
        tx_cost.account_data_size = 1;
        let cost = tx_cost.sum();

        // build testee to have capacity for one simple transaction
        let mut testee = CostTracker::new(cost, cost, cost, None);
        assert!(testee.would_fit(&tx_cost).is_ok());
        let old = testee.account_data_size;
        testee.add_transaction_cost(&tx_cost);
        assert_eq!(old + 1, testee.account_data_size);
    }

    #[test]
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
    fn test_cost_tracker_ok_add_two_same_accounts() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with same signed account
        let (_tx1, tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost1 = tx_cost1.sum();
        let (_tx2, tx_cost2) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost2 = tx_cost2.sum();

        // build testee to have capacity for two simple transactions, with same accounts
        let mut testee = CostTracker::new(cost1 + cost2, cost1 + cost2, cost1 + cost2);
        {
<<<<<<< HEAD
            assert!(testee.would_fit(&keys1, cost1, &tx1).is_ok());
            testee.add_transaction(&keys1, cost1, &tx1);
        }
        {
            assert!(testee.would_fit(&keys2, cost2, &tx2).is_ok());
            testee.add_transaction(&keys2, cost2, &tx2);
=======
            assert!(testee.would_fit(&tx_cost1).is_ok());
            testee.add_transaction_cost(&tx_cost1);
        }
        {
            assert!(testee.would_fit(&tx_cost2).is_ok());
<<<<<<< HEAD
            testee.add_transaction(&tx_cost2);
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
=======
            testee.add_transaction_cost(&tx_cost2);
>>>>>>> a058f348a (Address review comments)
        }
        assert_eq!(cost1 + cost2, testee.block_cost);
        assert_eq!(1, testee.cost_by_writable_accounts.len());
        let (_ccostliest_account, costliest_account_cost) = testee.find_costliest_account();
        assert_eq!(cost1 + cost2, costliest_account_cost);
    }

    #[test]
    fn test_cost_tracker_ok_add_two_diff_accounts() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let second_account = Keypair::new();
        let (_tx1, tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost1 = tx_cost1.sum();
        let (_tx2, tx_cost2) = build_simple_transaction(&second_account, &start_hash);
        let cost2 = tx_cost2.sum();

        // build testee to have capacity for two simple transactions, with same accounts
        let mut testee = CostTracker::new(cmp::max(cost1, cost2), cost1 + cost2, cost1 + cost2);
        {
<<<<<<< HEAD
            assert!(testee.would_fit(&keys1, cost1, &tx1).is_ok());
            testee.add_transaction(&keys1, cost1, &tx1);
        }
        {
            assert!(testee.would_fit(&keys2, cost2, &tx2).is_ok());
            testee.add_transaction(&keys2, cost2, &tx2);
=======
            assert!(testee.would_fit(&tx_cost1).is_ok());
            testee.add_transaction_cost(&tx_cost1);
        }
        {
            assert!(testee.would_fit(&tx_cost2).is_ok());
<<<<<<< HEAD
            testee.add_transaction(&tx_cost2);
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
=======
            testee.add_transaction_cost(&tx_cost2);
>>>>>>> a058f348a (Address review comments)
        }
        assert_eq!(cost1 + cost2, testee.block_cost);
        assert_eq!(2, testee.cost_by_writable_accounts.len());
        let (_ccostliest_account, costliest_account_cost) = testee.find_costliest_account();
        assert_eq!(std::cmp::max(cost1, cost2), costliest_account_cost);
    }

    #[test]
    fn test_cost_tracker_chain_reach_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with same signed account
        let (_tx1, tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost1 = tx_cost1.sum();
        let (_tx2, tx_cost2) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost2 = tx_cost2.sum();

        // build testee to have capacity for two simple transactions, but not for same accounts
        let mut testee = CostTracker::new(cmp::min(cost1, cost2), cost1 + cost2, cost1 + cost2);
        // should have room for first transaction
        {
<<<<<<< HEAD
            assert!(testee.would_fit(&keys1, cost1, &tx1).is_ok());
            testee.add_transaction(&keys1, cost1, &tx1);
        }
        // but no more sapce on the same chain (same signer account)
        {
            assert!(testee.would_fit(&keys2, cost2, &tx2).is_err());
=======
            assert!(testee.would_fit(&tx_cost1).is_ok());
            testee.add_transaction_cost(&tx_cost1);
        }
        // but no more sapce on the same chain (same signer account)
        {
            assert!(testee.would_fit(&tx_cost2).is_err());
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
        }
    }

    #[test]
    fn test_cost_tracker_reach_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let second_account = Keypair::new();
        let (_tx1, tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let cost1 = tx_cost1.sum();
        let (_tx2, tx_cost2) = build_simple_transaction(&second_account, &start_hash);
        let cost2 = tx_cost2.sum();

        // build testee to have capacity for each chain, but not enough room for both transactions
        let mut testee =
            CostTracker::new(cmp::max(cost1, cost2), cost1 + cost2 - 1, cost1 + cost2 - 1);
        // should have room for first transaction
        {
<<<<<<< HEAD
            assert!(testee.would_fit(&keys1, cost1, &tx1).is_ok());
            testee.add_transaction(&keys1, cost1, &tx1);
        }
        // but no more room for package as whole
        {
            assert!(testee.would_fit(&keys2, cost2, &tx2).is_err());
=======
            assert!(testee.would_fit(&tx_cost1).is_ok());
            testee.add_transaction_cost(&tx_cost1);
        }
        // but no more room for package as whole
        {
            assert!(testee.would_fit(&tx_cost2).is_err());
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
        }
    }

    #[test]
    fn test_cost_tracker_reach_vote_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two mocking vote transactions with diff accounts
        let second_account = Keypair::new();
        let (_tx1, tx_cost1) = build_simple_vote_transaction(&mint_keypair, &start_hash);
        let (_tx2, tx_cost2) = build_simple_vote_transaction(&second_account, &start_hash);
        let cost1 = tx_cost1.sum();
        let cost2 = tx_cost2.sum();

        // build testee to have capacity for each chain, but not enough room for both votes
        let mut testee = CostTracker::new(cmp::max(cost1, cost2), cost1 + cost2, cost1 + cost2 - 1);
        // should have room for first vote
        {
<<<<<<< HEAD
            assert!(testee.would_fit(&keys1, cost1, &tx1).is_ok());
            testee.add_transaction(&keys1, cost1, &tx1);
        }
        // but no more room for package as whole
        {
            assert!(testee.would_fit(&keys2, cost2, &tx2).is_err());
=======
            assert!(testee.would_fit(&tx_cost1).is_ok());
            testee.add_transaction_cost(&tx_cost1);
        }
        // but no more room for package as whole
        {
            assert!(testee.would_fit(&tx_cost2).is_err());
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
        }
        // however there is room for none-vote tx3
        {
            let third_account = Keypair::new();
<<<<<<< HEAD
            let (tx3, keys3, cost3) = build_simple_transaction(&third_account, &start_hash);
            assert!(testee.would_fit(&keys3, cost3, &tx3).is_ok());
=======
            let (_tx3, tx_cost3) = build_simple_transaction(&third_account, &start_hash);
            assert!(testee.would_fit(&tx_cost3).is_ok());
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
        }
    }

    #[test]
<<<<<<< HEAD
=======
    fn test_cost_tracker_reach_data_block_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let second_account = Keypair::new();
        let (_tx1, mut tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let (_tx2, mut tx_cost2) = build_simple_transaction(&second_account, &start_hash);
        tx_cost1.account_data_size = MAX_ACCOUNT_DATA_BLOCK_LEN;
        tx_cost2.account_data_size = MAX_ACCOUNT_DATA_BLOCK_LEN + 1;
        let cost1 = tx_cost1.sum();
        let cost2 = tx_cost2.sum();

        // build testee that passes
        let testee = CostTracker::new(
            cmp::max(cost1, cost2),
            cost1 + cost2 - 1,
            cost1 + cost2 - 1,
            None,
        );
        assert!(testee.would_fit(&tx_cost1).is_ok());
        // data is too big
        assert_eq!(
            testee.would_fit(&tx_cost2),
            Err(CostTrackerError::WouldExceedAccountDataBlockLimit),
        );
    }

    #[test]
    fn test_cost_tracker_reach_data_total_limit() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let second_account = Keypair::new();
        let (_tx1, mut tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let (_tx2, mut tx_cost2) = build_simple_transaction(&second_account, &start_hash);
        let remaining_account_data_size = 1234;
        tx_cost1.account_data_size = remaining_account_data_size;
        tx_cost2.account_data_size = remaining_account_data_size + 1;
        let cost1 = tx_cost1.sum();
        let cost2 = tx_cost2.sum();

        // build testee that passes
        let testee = CostTracker::new(
            cmp::max(cost1, cost2),
            cost1 + cost2 - 1,
            cost1 + cost2 - 1,
            Some(remaining_account_data_size),
        );
        assert!(testee.would_fit(&tx_cost1).is_ok());
        // data is too big
        assert_eq!(
            testee.would_fit(&tx_cost2),
            Err(CostTrackerError::WouldExceedAccountDataTotalLimit),
        );
    }

    #[test]
    fn test_cost_tracker_remove() {
        let (mint_keypair, start_hash) = test_setup();
        // build two transactions with diff accounts
        let second_account = Keypair::new();
        let (_tx1, tx_cost1) = build_simple_transaction(&mint_keypair, &start_hash);
        let (_tx2, tx_cost2) = build_simple_transaction(&second_account, &start_hash);
        let cost1 = tx_cost1.sum();
        let cost2 = tx_cost2.sum();
        // build testee
        let mut testee = CostTracker::new(cost1 + cost2, cost1 + cost2, cost1 + cost2, None);

        assert!(testee.try_add(&tx_cost1).is_ok());
        assert!(testee.try_add(&tx_cost2).is_ok());
        assert_eq!(cost1 + cost2, testee.block_cost);

        // removing a tx_cost affects block_cost
        testee.remove(&tx_cost1);
        assert_eq!(cost2, testee.block_cost);

        // add back tx1
        assert!(testee.try_add(&tx_cost1).is_ok());
        assert_eq!(cost1 + cost2, testee.block_cost);

        // cannot add tx1 again, cost limit would be exceeded
        assert!(testee.try_add(&tx_cost1).is_err());
    }

    #[test]
>>>>>>> 9e07272af (- Only commit successfully executed transactions' cost to cost_tracker;)
    fn test_cost_tracker_try_add_is_atomic() {
        let (mint_keypair, start_hash) = test_setup();
        // build two mocking vote transactions with diff accounts
        let (_tx1, _tx_cost1) = build_simple_vote_transaction(&mint_keypair, &start_hash);

        let acct1 = Pubkey::new_unique();
        let acct2 = Pubkey::new_unique();
        let acct3 = Pubkey::new_unique();
        let cost = 100;
        let account_max = cost * 2;
        let block_max = account_max * 3; // for three accts

        let mut testee = CostTracker::new(account_max, block_max, block_max);

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
            let (_costliest_account, costliest_account_cost) = testee.find_costliest_account();
            assert_eq!(cost, testee.block_cost);
            assert_eq!(3, testee.cost_by_writable_accounts.len());
            assert_eq!(cost, costliest_account_cost);
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
            let (costliest_account, costliest_account_cost) = testee.find_costliest_account();
            assert_eq!(cost * 2, testee.block_cost);
            assert_eq!(3, testee.cost_by_writable_accounts.len());
            assert_eq!(cost * 2, costliest_account_cost);
            assert_eq!(acct2, costliest_account);
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
            let (costliest_account, costliest_account_cost) = testee.find_costliest_account();
            assert_eq!(cost * 2, testee.block_cost);
            assert_eq!(3, testee.cost_by_writable_accounts.len());
            assert_eq!(cost * 2, costliest_account_cost);
            assert_eq!(acct2, costliest_account);
        }
    }
}
