//! 'cost_model` provides service to estimate a transaction's cost
//! following proposed fee schedule #16984; Relevant cluster cost
//! measuring is described by #19627
//!
//! The main function is `calculate_cost` which returns &TransactionCost.
//!

use {
    crate::{bank::Bank, block_cost_limits::*},
    log::*,
    solana_program_runtime::compute_budget::{
        ComputeBudget, DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
    },
    solana_sdk::{
        compute_budget,
        feature_set::{
            cap_transaction_accounts_data_size, remove_deprecated_request_unit_ix,
            use_default_units_in_fee_calculation, FeatureSet,
        },
        instruction::CompiledInstruction,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        system_instruction::SystemInstruction,
        system_program,
        transaction::SanitizedTransaction,
    },
};

const MAX_WRITABLE_ACCOUNTS: usize = 256;

// costs are stored in number of 'compute unit's
#[derive(Debug)]
pub struct TransactionCost {
    pub writable_accounts: Vec<Pubkey>,
    pub signature_cost: u64,
    pub write_lock_cost: u64,
    pub data_bytes_cost: u64,
    pub builtins_execution_cost: u64,
    pub bpf_execution_cost: u64,
    pub account_data_size: u64,
    pub is_simple_vote: bool,
}

impl Default for TransactionCost {
    fn default() -> Self {
        Self {
            writable_accounts: Vec::with_capacity(MAX_WRITABLE_ACCOUNTS),
            signature_cost: 0u64,
            write_lock_cost: 0u64,
            data_bytes_cost: 0u64,
            builtins_execution_cost: 0u64,
            bpf_execution_cost: 0u64,
            account_data_size: 0u64,
            is_simple_vote: false,
        }
    }
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
        self.signature_cost = 0;
        self.write_lock_cost = 0;
        self.data_bytes_cost = 0;
        self.builtins_execution_cost = 0;
        self.bpf_execution_cost = 0;
        self.is_simple_vote = false;
    }

    pub fn sum(&self) -> u64 {
        self.sum_without_bpf()
            .saturating_add(self.bpf_execution_cost)
    }

    pub fn sum_without_bpf(&self) -> u64 {
        self.signature_cost
            .saturating_add(self.write_lock_cost)
            .saturating_add(self.data_bytes_cost)
            .saturating_add(self.builtins_execution_cost)
    }
}

pub struct CostModel;

impl CostModel {
    pub fn calculate_cost(
        transaction: &SanitizedTransaction,
        feature_set: &FeatureSet,
    ) -> TransactionCost {
        let mut tx_cost = TransactionCost::new_with_capacity(MAX_WRITABLE_ACCOUNTS);

        tx_cost.signature_cost = Self::get_signature_cost(transaction);
        Self::get_write_lock_cost(&mut tx_cost, transaction);
        Self::get_transaction_cost(&mut tx_cost, transaction, feature_set);
        tx_cost.account_data_size = Self::calculate_account_data_size(transaction);
        tx_cost.is_simple_vote = transaction.is_simple_vote_transaction();

        debug!("transaction {:?} has cost {:?}", transaction, tx_cost);
        tx_cost
    }

    fn get_signature_cost(transaction: &SanitizedTransaction) -> u64 {
        transaction.signatures().len() as u64 * SIGNATURE_COST
    }

    fn get_write_lock_cost(tx_cost: &mut TransactionCost, transaction: &SanitizedTransaction) {
        let message = transaction.message();
        message
            .account_keys()
            .iter()
            .enumerate()
            .for_each(|(i, k)| {
                let is_writable = message.is_writable(i);

                if is_writable {
                    tx_cost.writable_accounts.push(*k);
                    tx_cost.write_lock_cost += WRITE_LOCK_UNITS;
                }
            });
    }

    fn get_transaction_cost(
        tx_cost: &mut TransactionCost,
        transaction: &SanitizedTransaction,
        feature_set: &FeatureSet,
    ) {
        let mut builtin_costs = 0u64;
        let mut bpf_costs = 0u64;
        let mut data_bytes_len_total = 0u64;

        for (program_id, instruction) in transaction.message().program_instructions_iter() {
            // to keep the same behavior, look for builtin first
            if let Some(builtin_cost) = BUILT_IN_INSTRUCTION_COSTS.get(program_id) {
                builtin_costs = builtin_costs.saturating_add(*builtin_cost);
            } else if !compute_budget::check_id(program_id) {
                bpf_costs = bpf_costs.saturating_add(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT.into());
            }
            data_bytes_len_total =
                data_bytes_len_total.saturating_add(instruction.data.len() as u64);
        }

        // calculate bpf cost based on compute budget instructions
        let mut budget = ComputeBudget::default();
        let result = budget.process_instructions(
            transaction.message().program_instructions_iter(),
            feature_set.is_active(&use_default_units_in_fee_calculation::id()),
            !feature_set.is_active(&remove_deprecated_request_unit_ix::id()),
            feature_set.is_active(&cap_transaction_accounts_data_size::id()),
            Bank::get_loaded_accounts_data_limit_type(feature_set),
        );

        // if tx contained user-space instructions and a more accurate estimate available correct it
        if bpf_costs > 0 && result.is_ok() {
            bpf_costs = budget.compute_unit_limit
        }

        tx_cost.builtins_execution_cost = builtin_costs;
        tx_cost.bpf_execution_cost = bpf_costs;
        tx_cost.data_bytes_cost = data_bytes_len_total / INSTRUCTION_DATA_BYTES_COST;
    }

    fn calculate_account_data_size_on_deserialized_system_instruction(
        instruction: SystemInstruction,
    ) -> u64 {
        match instruction {
            SystemInstruction::CreateAccount {
                lamports: _lamports,
                space,
                owner: _owner,
            } => space,
            SystemInstruction::CreateAccountWithSeed {
                base: _base,
                seed: _seed,
                lamports: _lamports,
                space,
                owner: _owner,
            } => space,
            SystemInstruction::Allocate { space } => space,
            SystemInstruction::AllocateWithSeed {
                base: _base,
                seed: _seed,
                space,
                owner: _owner,
            } => space,
            _ => 0,
        }
    }

    fn calculate_account_data_size_on_instruction(
        program_id: &Pubkey,
        instruction: &CompiledInstruction,
    ) -> u64 {
        if program_id == &system_program::id() {
            if let Ok(instruction) = limited_deserialize(&instruction.data) {
                return Self::calculate_account_data_size_on_deserialized_system_instruction(
                    instruction,
                );
            }
        }
        0
    }

    /// eventually, potentially determine account data size of all writable accounts
    /// at the moment, calculate account data size of account creation
    fn calculate_account_data_size(transaction: &SanitizedTransaction) -> u64 {
        transaction
            .message()
            .program_instructions_iter()
            .map(|(program_id, instruction)| {
                Self::calculate_account_data_size_on_instruction(program_id, instruction)
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::Bank,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            inline_spl_token,
        },
        solana_sdk::{
            compute_budget::{self, ComputeBudgetInstruction},
            hash::Hash,
            instruction::CompiledInstruction,
            message::Message,
            signature::{Keypair, Signer},
            system_instruction::{self},
            system_program, system_transaction,
            transaction::Transaction,
        },
        std::sync::Arc,
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

    #[test]
    fn test_cost_model_data_len_cost() {
        let lamports = 0;
        let owner = Pubkey::default();
        let seed = String::default();
        let space = 100;
        let base = Pubkey::default();
        for instruction in [
            SystemInstruction::CreateAccount {
                lamports,
                space,
                owner,
            },
            SystemInstruction::CreateAccountWithSeed {
                base,
                seed: seed.clone(),
                lamports,
                space,
                owner,
            },
            SystemInstruction::Allocate { space },
            SystemInstruction::AllocateWithSeed {
                base,
                seed,
                space,
                owner,
            },
        ] {
            assert_eq!(
                space,
                CostModel::calculate_account_data_size_on_deserialized_system_instruction(
                    instruction
                )
            );
        }
        assert_eq!(
            0,
            CostModel::calculate_account_data_size_on_deserialized_system_instruction(
                SystemInstruction::TransferWithSeed {
                    lamports,
                    from_seed: String::default(),
                    from_owner: Pubkey::default(),
                }
            )
        );
    }

    #[test]
    fn test_cost_model_simple_transaction() {
        let (mint_keypair, start_hash) = test_setup();

        let keypair = Keypair::new();
        let simple_transaction = SanitizedTransaction::from_transaction_for_tests(
            system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 2, start_hash),
        );
        debug!(
            "system_transaction simple_transaction {:?}",
            simple_transaction
        );

        // expected cost for one system transfer instructions
        let expected_execution_cost = BUILT_IN_INSTRUCTION_COSTS
            .get(&system_program::id())
            .unwrap();

        let mut tx_cost = TransactionCost::default();
        CostModel::get_transaction_cost(
            &mut tx_cost,
            &simple_transaction,
            &FeatureSet::all_enabled(),
        );
        assert_eq!(*expected_execution_cost, tx_cost.builtins_execution_cost);
        assert_eq!(0, tx_cost.bpf_execution_cost);
        assert_eq!(3, tx_cost.data_bytes_cost);
    }

    #[test]
    fn test_cost_model_token_transaction() {
        let (mint_keypair, start_hash) = test_setup();

        let instructions = vec![CompiledInstruction::new(3, &(), vec![1, 2, 0])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&mint_keypair],
            &[
                solana_sdk::pubkey::new_rand(),
                solana_sdk::pubkey::new_rand(),
            ],
            start_hash,
            vec![inline_spl_token::id()],
            instructions,
        );
        let token_transaction = SanitizedTransaction::from_transaction_for_tests(tx);
        debug!("token_transaction {:?}", token_transaction);

        let mut tx_cost = TransactionCost::default();
        CostModel::get_transaction_cost(
            &mut tx_cost,
            &token_transaction,
            &FeatureSet::all_enabled(),
        );
        assert_eq!(0, tx_cost.builtins_execution_cost);
        assert_eq!(200_000, tx_cost.bpf_execution_cost);
        assert_eq!(0, tx_cost.data_bytes_cost);
    }

    #[test]
    fn test_cost_model_compute_budget_transaction() {
        let (mint_keypair, start_hash) = test_setup();

        let instructions = vec![
            CompiledInstruction::new(3, &(), vec![1, 2, 0]),
            CompiledInstruction::new_from_raw_parts(
                4,
                ComputeBudgetInstruction::SetComputeUnitLimit(12_345)
                    .pack()
                    .unwrap(),
                vec![],
            ),
        ];
        let tx = Transaction::new_with_compiled_instructions(
            &[&mint_keypair],
            &[
                solana_sdk::pubkey::new_rand(),
                solana_sdk::pubkey::new_rand(),
            ],
            start_hash,
            vec![inline_spl_token::id(), compute_budget::id()],
            instructions,
        );
        let token_transaction = SanitizedTransaction::from_transaction_for_tests(tx);

        let mut tx_cost = TransactionCost::default();
        CostModel::get_transaction_cost(
            &mut tx_cost,
            &token_transaction,
            &FeatureSet::all_enabled(),
        );
        assert_eq!(0, tx_cost.builtins_execution_cost);
        assert_eq!(12_345, tx_cost.bpf_execution_cost);
        assert_eq!(1, tx_cost.data_bytes_cost);
    }

    #[test]
    fn test_cost_model_transaction_many_transfer_instructions() {
        let (mint_keypair, start_hash) = test_setup();

        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let instructions =
            system_instruction::transfer_many(&mint_keypair.pubkey(), &[(key1, 1), (key2, 1)]);
        let message = Message::new(&instructions, Some(&mint_keypair.pubkey()));
        let tx = SanitizedTransaction::from_transaction_for_tests(Transaction::new(
            &[&mint_keypair],
            message,
            start_hash,
        ));
        debug!("many transfer transaction {:?}", tx);

        // expected cost for two system transfer instructions
        let program_cost = BUILT_IN_INSTRUCTION_COSTS
            .get(&system_program::id())
            .unwrap();
        let expected_cost = program_cost * 2;

        let mut tx_cost = TransactionCost::default();
        CostModel::get_transaction_cost(&mut tx_cost, &tx, &FeatureSet::all_enabled());
        assert_eq!(expected_cost, tx_cost.builtins_execution_cost);
        assert_eq!(0, tx_cost.bpf_execution_cost);
        assert_eq!(6, tx_cost.data_bytes_cost);
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
        let tx = SanitizedTransaction::from_transaction_for_tests(
            Transaction::new_with_compiled_instructions(
                &[&mint_keypair],
                &[key1, key2],
                start_hash,
                vec![prog1, prog2],
                instructions,
            ),
        );
        debug!("many random transaction {:?}", tx);

        let expected_cost = DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64 * 2;
        let mut tx_cost = TransactionCost::default();
        CostModel::get_transaction_cost(&mut tx_cost, &tx, &FeatureSet::all_enabled());
        assert_eq!(0, tx_cost.builtins_execution_cost);
        assert_eq!(expected_cost, tx_cost.bpf_execution_cost);
        assert_eq!(0, tx_cost.data_bytes_cost);
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
        let tx = SanitizedTransaction::from_transaction_for_tests(
            Transaction::new_with_compiled_instructions(
                &[&signer1, &signer2],
                &[key1, key2],
                Hash::new_unique(),
                vec![prog1, prog2],
                instructions,
            ),
        );

        let tx_cost = CostModel::calculate_cost(&tx, &FeatureSet::all_enabled());
        assert_eq!(2 + 2, tx_cost.writable_accounts.len());
        assert_eq!(signer1.pubkey(), tx_cost.writable_accounts[0]);
        assert_eq!(signer2.pubkey(), tx_cost.writable_accounts[1]);
        assert_eq!(key1, tx_cost.writable_accounts[2]);
        assert_eq!(key2, tx_cost.writable_accounts[3]);
    }

    #[test]
    fn test_cost_model_calculate_cost() {
        let (mint_keypair, start_hash) = test_setup();
        let tx = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &Keypair::new().pubkey(),
            2,
            start_hash,
        ));

        let expected_account_cost = WRITE_LOCK_UNITS * 2;
        let expected_execution_cost = BUILT_IN_INSTRUCTION_COSTS
            .get(&system_program::id())
            .unwrap();

        let tx_cost = CostModel::calculate_cost(&tx, &FeatureSet::all_enabled());
        assert_eq!(expected_account_cost, tx_cost.write_lock_cost);
        assert_eq!(*expected_execution_cost, tx_cost.builtins_execution_cost);
        assert_eq!(2, tx_cost.writable_accounts.len());
    }
}
