//! 'cost_model` provides service to estimate a transaction's cost
//! It does so by analyzing accounts the transaction touches, and instructions
//! it includes. Using historical data as guideline, it estimates cost of
//! reading/writing account, the sum of that comes up to "account access cost";
//! Instructions take time to execute, both historical and runtime data are
//! used to determine each instruction's execution time, the sum of that
//! is transaction's "execution cost"
//! The main function is `calculate_cost` which returns &TransactionCost.
//!
use crate::execute_cost_table::ExecuteCostTable;
use log::*;
use solana_sdk::{pubkey::Pubkey, sanitized_transaction::SanitizedTransaction};
use std::collections::HashMap;

// 07-27-2021, compute_unit to microsecond conversion ratio collected from mainnet-beta
// differs between instructions. Some bpf instruction has much higher CU/US ratio
// (eg 7vxeyaXGLqcp66fFShqUdHxdacp4k4kwUpRSSeoZLCZ4 has average ratio 135), others
// have lower ratio (eg 9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin has an average ratio 14).
// With this, I am guestimating the flat_fee for sigver and account read/write
// as following. This can be adjusted when needed.
const SIGVER_COST: u64 = 1;
const NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST: u64 = 1;
const NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST: u64 = 2;
const SIGNED_READONLY_ACCOUNT_ACCESS_COST: u64 =
    SIGVER_COST + NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST;
const SIGNED_WRITABLE_ACCOUNT_ACCESS_COST: u64 =
    SIGVER_COST + NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST;

// 07-27-2021, cost model limit is set to "worst case scenario", which is the
// max compute unit it can execute. From mainnet-beta, the max CU of instruction
// is 3753, round up to 4_000. Say we allows max 50_000 instruction per writable i
// account, and  1_000_000 instruction per block. It comes to following limits:
pub const ACCOUNT_MAX_COST: u64 = 200_000_000;
pub const BLOCK_MAX_COST: u64 = 4_000_000_000;

const MAX_WRITABLE_ACCOUNTS: usize = 256;

#[derive(Debug, Clone)]
pub enum CostModelError {
    /// transaction that would fail sanitize, cost model is not able to process
    /// such transaction.
    InvalidTransaction,

    /// would exceed block max limit
    WouldExceedBlockMaxLimit,

    /// would exceed account max limit
    WouldExceedAccountMaxLimit,
}

// cost of transaction is made of account_access_cost and instruction execution_cost
// where
// account_access_cost is the sum of read/write/sign all accounts included in the transaction
//     read is cheaper than write.
// execution_cost is the sum of all instructions execution cost, which is
//     observed during runtime and feedback by Replay
#[derive(Default, Debug)]
pub struct TransactionCost {
    pub writable_accounts: Vec<Pubkey>,
    pub account_access_cost: u64,
    pub execution_cost: u64,
}

impl TransactionCost {
    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            writable_accounts: Vec::with_capacity(capacity),
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        self.writable_accounts.clear();
        self.account_access_cost = 0;
        self.execution_cost = 0;
    }
}

#[derive(Debug)]
pub struct CostModel {
    account_cost_limit: u64,
    block_cost_limit: u64,
    instruction_execution_cost_table: ExecuteCostTable,

    // reusable variables
    transaction_cost: TransactionCost,
}

impl Default for CostModel {
    fn default() -> Self {
        CostModel::new(ACCOUNT_MAX_COST, BLOCK_MAX_COST)
    }
}

impl CostModel {
    pub fn new(chain_max: u64, block_max: u64) -> Self {
        Self {
            account_cost_limit: chain_max,
            block_cost_limit: block_max,
            instruction_execution_cost_table: ExecuteCostTable::default(),
            transaction_cost: TransactionCost::new_with_capacity(MAX_WRITABLE_ACCOUNTS),
        }
    }

    pub fn get_account_cost_limit(&self) -> u64 {
        self.account_cost_limit
    }

    pub fn get_block_cost_limit(&self) -> u64 {
        self.block_cost_limit
    }

    pub fn initialize_cost_table(&mut self, cost_table: &[(Pubkey, u64)]) {
        for (program_id, cost) in cost_table {
            match self.upsert_instruction_cost(program_id, *cost) {
                Ok(c) => {
                    debug!(
                        "initiating cost table, instruction {:?} has cost {}",
                        program_id, c
                    );
                }
                Err(err) => {
                    debug!(
                        "initiating cost table, failed for instruction {:?}, err: {}",
                        program_id, err
                    );
                }
            }
        }
        debug!(
            "restored cost model instruction cost table from blockstore, current values: {:?}",
            self.get_instruction_cost_table()
        );
    }

    pub fn calculate_cost(&mut self, transaction: &SanitizedTransaction) -> &TransactionCost {
        self.transaction_cost.reset();

        // calculate transaction exeution cost
        self.transaction_cost.execution_cost = self.find_transaction_cost(transaction);

        // calculate account access cost
        let message = transaction.message();
        message.account_keys.iter().enumerate().for_each(|(i, k)| {
            let is_signer = message.is_signer(i);
            let is_writable = message.is_writable(i);

            if is_signer && is_writable {
                self.transaction_cost.writable_accounts.push(*k);
                self.transaction_cost.account_access_cost += SIGNED_WRITABLE_ACCOUNT_ACCESS_COST;
            } else if is_signer && !is_writable {
                self.transaction_cost.account_access_cost += SIGNED_READONLY_ACCOUNT_ACCESS_COST;
            } else if !is_signer && is_writable {
                self.transaction_cost.writable_accounts.push(*k);
                self.transaction_cost.account_access_cost +=
                    NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST;
            } else {
                self.transaction_cost.account_access_cost +=
                    NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST;
            }
        });
        debug!(
            "transaction {:?} has cost {:?}",
            transaction, self.transaction_cost
        );
        &self.transaction_cost
    }

    // To update or insert instruction cost to table.
    pub fn upsert_instruction_cost(
        &mut self,
        program_key: &Pubkey,
        cost: u64,
    ) -> Result<u64, &'static str> {
        self.instruction_execution_cost_table
            .upsert(program_key, cost);
        match self.instruction_execution_cost_table.get_cost(program_key) {
            Some(cost) => Ok(*cost),
            None => Err("failed to upsert to ExecuteCostTable"),
        }
    }

    pub fn get_instruction_cost_table(&self) -> &HashMap<Pubkey, u64> {
        self.instruction_execution_cost_table.get_cost_table()
    }

    fn find_instruction_cost(&self, program_key: &Pubkey) -> u64 {
        match self.instruction_execution_cost_table.get_cost(program_key) {
            Some(cost) => *cost,
            None => {
                let default_value = self.instruction_execution_cost_table.get_mode();
                debug!(
                    "Program key {:?} does not have assigned cost, using mode {}",
                    program_key, default_value
                );
                default_value
            }
        }
    }

    fn find_transaction_cost(&self, transaction: &SanitizedTransaction) -> u64 {
        let mut cost: u64 = 0;

        for instruction in &transaction.message().instructions {
            let program_id =
                transaction.message().account_keys[instruction.program_id_index as usize];
            let instruction_cost = self.find_instruction_cost(&program_id);
            trace!(
                "instruction {:?} has cost of {}",
                instruction,
                instruction_cost
            );
            cost += instruction_cost;
        }
        cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_runtime::{
        bank::Bank,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    };
    use solana_sdk::{
        bpf_loader,
        hash::Hash,
        instruction::CompiledInstruction,
        message::Message,
        signature::{Keypair, Signer},
        system_instruction::{self},
        system_program, system_transaction,
        transaction::Transaction,
    };
    use std::{
        convert::{TryFrom, TryInto},
        str::FromStr,
        sync::{Arc, RwLock},
        thread::{self, JoinHandle},
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

    #[test]
    fn test_cost_model_instruction_cost() {
        let mut testee = CostModel::default();

        let known_key = Pubkey::from_str("known11111111111111111111111111111111111111").unwrap();
        testee.upsert_instruction_cost(&known_key, 100).unwrap();
        // find cost for known programs
        assert_eq!(100, testee.find_instruction_cost(&known_key));

        testee
            .upsert_instruction_cost(&bpf_loader::id(), 1999)
            .unwrap();
        assert_eq!(1999, testee.find_instruction_cost(&bpf_loader::id()));

        // unknown program is assigned with default cost
        assert_eq!(
            testee.instruction_execution_cost_table.get_mode(),
            testee.find_instruction_cost(
                &Pubkey::from_str("unknown111111111111111111111111111111111111").unwrap()
            )
        );
    }

    #[test]
    fn test_cost_model_simple_transaction() {
        let (mint_keypair, start_hash) = test_setup();

        let keypair = Keypair::new();
        let simple_transaction: SanitizedTransaction =
            system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 2, start_hash)
                .try_into()
                .unwrap();
        debug!(
            "system_transaction simple_transaction {:?}",
            simple_transaction
        );

        // expected cost for one system transfer instructions
        let expected_cost = 8;

        let mut testee = CostModel::default();
        testee
            .upsert_instruction_cost(&system_program::id(), expected_cost)
            .unwrap();
        assert_eq!(
            expected_cost,
            testee.find_transaction_cost(&simple_transaction)
        );
    }

    #[test]
    fn test_cost_model_transaction_many_transfer_instructions() {
        let (mint_keypair, start_hash) = test_setup();

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let instructions =
            system_instruction::transfer_many(&mint_keypair.pubkey(), &[(key1, 1), (key2, 1)]);
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let tx: SanitizedTransaction = Transaction::new(&[&mint_keypair], message, start_hash)
            .try_into()
            .unwrap();
        debug!("many transfer transaction {:?}", tx);

        // expected cost for two system transfer instructions
        let program_cost = 8;
        let expected_cost = program_cost * 2;

        let mut testee = CostModel::default();
        testee
            .upsert_instruction_cost(&system_program::id(), program_cost)
            .unwrap();
        assert_eq!(expected_cost, testee.find_transaction_cost(&tx));
    }

    #[test]
    fn test_cost_model_message_many_different_instructions() {
        let (mint_keypair, start_hash) = test_setup();

        // construct a transaction with multiple random instructions
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let prog1 = solana_sdk::pubkey::new_rand();
        let prog2 = solana_sdk::pubkey::new_rand();
        let instructions = vec![
            CompiledInstruction::new(3, &(), vec![0, 1]),
            CompiledInstruction::new(4, &(), vec![0, 2]),
        ];
        let tx: SanitizedTransaction = Transaction::new_with_compiled_instructions(
            &[&mint_keypair],
            &[key1, key2],
            start_hash,
            vec![prog1, prog2],
            instructions,
        )
        .try_into()
        .unwrap();
        debug!("many random transaction {:?}", tx);

        let testee = CostModel::default();
        let result = testee.find_transaction_cost(&tx);

        // expected cost for two random/unknown program is
        let expected_cost = testee.instruction_execution_cost_table.get_mode() * 2;
        assert_eq!(expected_cost, result);
    }

    #[test]
    fn test_cost_model_sort_message_accounts_by_type() {
        // construct a transaction with two random instructions with same signer
        let signer1 = Keypair::new();
        let signer2 = Keypair::new();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let prog1 = Pubkey::new_unique();
        let prog2 = Pubkey::new_unique();
        let instructions = vec![
            CompiledInstruction::new(4, &(), vec![0, 2]),
            CompiledInstruction::new(5, &(), vec![1, 3]),
        ];
        let tx: SanitizedTransaction = Transaction::new_with_compiled_instructions(
            &[&signer1, &signer2],
            &[key1, key2],
            Hash::new_unique(),
            vec![prog1, prog2],
            instructions,
        )
        .try_into()
        .unwrap();

        let mut cost_model = CostModel::default();
        let tx_cost = cost_model.calculate_cost(&tx);
        assert_eq!(2 + 2, tx_cost.writable_accounts.len());
        assert_eq!(signer1.pubkey(), tx_cost.writable_accounts[0]);
        assert_eq!(signer2.pubkey(), tx_cost.writable_accounts[1]);
        assert_eq!(key1, tx_cost.writable_accounts[2]);
        assert_eq!(key2, tx_cost.writable_accounts[3]);
    }

    #[test]
    fn test_cost_model_insert_instruction_cost() {
        let key1 = Pubkey::new_unique();
        let cost1 = 100;

        let mut cost_model = CostModel::default();
        // Using default cost for unknown instruction
        assert_eq!(
            cost_model.instruction_execution_cost_table.get_mode(),
            cost_model.find_instruction_cost(&key1)
        );

        // insert instruction cost to table
        assert!(cost_model.upsert_instruction_cost(&key1, cost1).is_ok());

        // now it is known insturction with known cost
        assert_eq!(cost1, cost_model.find_instruction_cost(&key1));
    }

    #[test]
    fn test_cost_model_calculate_cost() {
        let (mint_keypair, start_hash) = test_setup();
        let tx: SanitizedTransaction =
            system_transaction::transfer(&mint_keypair, &Keypair::new().pubkey(), 2, start_hash)
                .try_into()
                .unwrap();

        let expected_account_cost = SIGNED_WRITABLE_ACCOUNT_ACCESS_COST
            + NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST
            + NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST;
        let expected_execution_cost = 8;

        let mut cost_model = CostModel::default();
        cost_model
            .upsert_instruction_cost(&system_program::id(), expected_execution_cost)
            .unwrap();
        let tx_cost = cost_model.calculate_cost(&tx);
        assert_eq!(expected_account_cost, tx_cost.account_access_cost);
        assert_eq!(expected_execution_cost, tx_cost.execution_cost);
        assert_eq!(2, tx_cost.writable_accounts.len());
    }

    #[test]
    fn test_cost_model_update_instruction_cost() {
        let key1 = Pubkey::new_unique();
        let cost1 = 100;
        let cost2 = 200;
        let updated_cost = (cost1 + cost2) / 2;

        let mut cost_model = CostModel::default();

        // insert instruction cost to table
        assert!(cost_model.upsert_instruction_cost(&key1, cost1).is_ok());
        assert_eq!(cost1, cost_model.find_instruction_cost(&key1));

        // update instruction cost
        assert!(cost_model.upsert_instruction_cost(&key1, cost2).is_ok());
        assert_eq!(updated_cost, cost_model.find_instruction_cost(&key1));
    }

    #[test]
    fn test_cost_model_can_be_shared_concurrently_with_rwlock() {
        let (mint_keypair, start_hash) = test_setup();
        // construct a transaction with multiple random instructions
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let prog1 = solana_sdk::pubkey::new_rand();
        let prog2 = solana_sdk::pubkey::new_rand();
        let instructions = vec![
            CompiledInstruction::new(3, &(), vec![0, 1]),
            CompiledInstruction::new(4, &(), vec![0, 2]),
        ];
        let tx = Arc::new(
            SanitizedTransaction::try_from(Transaction::new_with_compiled_instructions(
                &[&mint_keypair],
                &[key1, key2],
                start_hash,
                vec![prog1, prog2],
                instructions,
            ))
            .unwrap(),
        );

        let number_threads = 10;
        let expected_account_cost = SIGNED_WRITABLE_ACCOUNT_ACCESS_COST
            + NON_SIGNED_WRITABLE_ACCOUNT_ACCESS_COST * 2
            + NON_SIGNED_READONLY_ACCOUNT_ACCESS_COST * 2;
        let cost1 = 100;
        let cost2 = 200;
        // execution cost can be either 2 * Default (before write) or cost1+cost2 (after write)

        let cost_model: Arc<RwLock<CostModel>> = Arc::new(RwLock::new(CostModel::default()));

        let thread_handlers: Vec<JoinHandle<()>> = (0..number_threads)
            .map(|i| {
                let cost_model = cost_model.clone();
                let tx = tx.clone();

                if i == 5 {
                    thread::spawn(move || {
                        let mut cost_model = cost_model.write().unwrap();
                        assert!(cost_model.upsert_instruction_cost(&prog1, cost1).is_ok());
                        assert!(cost_model.upsert_instruction_cost(&prog2, cost2).is_ok());
                    })
                } else {
                    thread::spawn(move || {
                        let mut cost_model = cost_model.write().unwrap();
                        let tx_cost = cost_model.calculate_cost(&tx);
                        assert_eq!(3, tx_cost.writable_accounts.len());
                        assert_eq!(expected_account_cost, tx_cost.account_access_cost);
                    })
                }
            })
            .collect();

        for th in thread_handlers {
            th.join().unwrap();
        }
    }

    #[test]
    fn test_cost_model_init_cost_table() {
        // build cost table
        let cost_table = vec![
            (Pubkey::new_unique(), 10),
            (Pubkey::new_unique(), 20),
            (Pubkey::new_unique(), 30),
        ];

        // init cost model
        let mut cost_model = CostModel::default();
        cost_model.initialize_cost_table(&cost_table);

        // verify
        for (id, cost) in cost_table.iter() {
            assert_eq!(*cost, cost_model.find_instruction_cost(id));
        }
    }
}
