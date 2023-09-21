//! 'cost_model` provides service to estimate a transaction's cost
//! following proposed fee schedule #16984; Relevant cluster cost
//! measuring is described by #19627
//!
//! The main function is `calculate_cost` which returns &TransactionCost.
//!

use {
    crate::{block_cost_limits::*, transaction_cost::*},
    log::*,
    solana_program_runtime::compute_budget::{
        ComputeBudget, DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT, MAX_COMPUTE_UNIT_LIMIT,
    },
    solana_sdk::{
        borsh0_10::try_from_slice_unchecked,
        compute_budget::{self, ComputeBudgetInstruction},
        feature_set::{
            add_set_tx_loaded_accounts_data_size_instruction,
            include_loaded_accounts_data_size_in_fee_calculation,
            remove_deprecated_request_unit_ix, FeatureSet,
        },
        fee::FeeStructure,
        instruction::CompiledInstruction,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        system_instruction::SystemInstruction,
        system_program,
        transaction::SanitizedTransaction,
    },
};

pub struct CostModel;

impl CostModel {
    pub fn calculate_cost(
        transaction: &SanitizedTransaction,
        feature_set: &FeatureSet,
    ) -> TransactionCost {
        if transaction.is_simple_vote_transaction() {
            TransactionCost::SimpleVote {
                writable_accounts: Self::get_writable_accounts(transaction),
            }
        } else {
            let mut tx_cost = UsageCostDetails::new_with_default_capacity();

            tx_cost.signature_cost = Self::get_signature_cost(transaction);
            Self::get_write_lock_cost(&mut tx_cost, transaction);
            Self::get_transaction_cost(&mut tx_cost, transaction, feature_set);
            tx_cost.account_data_size = Self::calculate_account_data_size(transaction);

            debug!("transaction {:?} has cost {:?}", transaction, tx_cost);
            TransactionCost::Transaction(tx_cost)
        }
    }

    // Calculate cost of loaded accounts size in the same way heap cost is charged at
    // rate of 8cu per 32K. Citing `program_runtime\src\compute_budget.rs`: "(cost of
    // heap is about) 0.5us per 32k at 15 units/us rounded up"
    //
    // Before feature `support_set_loaded_accounts_data_size_limit_ix` is enabled, or
    // if user doesn't use compute budget ix `set_loaded_accounts_data_size_limit_ix`
    // to set limit, `compute_budget.loaded_accounts_data_size_limit` is set to default
    // limit of 64MB; which will convert to (64M/32K)*8CU = 16_000 CUs
    //
    pub fn calculate_loaded_accounts_data_size_cost(compute_budget: &ComputeBudget) -> u64 {
        FeeStructure::calculate_memory_usage_cost(
            compute_budget.loaded_accounts_data_size_limit,
            compute_budget.heap_cost,
        )
    }

    fn get_signature_cost(transaction: &SanitizedTransaction) -> u64 {
        transaction.signatures().len() as u64 * SIGNATURE_COST
    }

    fn get_writable_accounts(transaction: &SanitizedTransaction) -> Vec<Pubkey> {
        let message = transaction.message();
        message
            .account_keys()
            .iter()
            .enumerate()
            .filter_map(|(i, k)| {
                if message.is_writable(i) {
                    Some(*k)
                } else {
                    None
                }
            })
            .collect()
    }

    fn get_write_lock_cost(tx_cost: &mut UsageCostDetails, transaction: &SanitizedTransaction) {
        tx_cost.writable_accounts = Self::get_writable_accounts(transaction);
        tx_cost.write_lock_cost =
            WRITE_LOCK_UNITS.saturating_mul(tx_cost.writable_accounts.len() as u64);
    }

    fn get_transaction_cost(
        tx_cost: &mut UsageCostDetails,
        transaction: &SanitizedTransaction,
        feature_set: &FeatureSet,
    ) {
        let mut builtin_costs = 0u64;
        let mut bpf_costs = 0u64;
        let mut loaded_accounts_data_size_cost = 0u64;
        let mut data_bytes_len_total = 0u64;
        let mut compute_unit_limit_is_set = false;

        for (program_id, instruction) in transaction.message().program_instructions_iter() {
            // to keep the same behavior, look for builtin first
            if let Some(builtin_cost) = BUILT_IN_INSTRUCTION_COSTS.get(program_id) {
                builtin_costs = builtin_costs.saturating_add(*builtin_cost);
            } else {
                bpf_costs = bpf_costs
                    .saturating_add(u64::from(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT))
                    .min(u64::from(MAX_COMPUTE_UNIT_LIMIT));
            }
            data_bytes_len_total =
                data_bytes_len_total.saturating_add(instruction.data.len() as u64);

            if compute_budget::check_id(program_id) {
                if let Ok(ComputeBudgetInstruction::SetComputeUnitLimit(_)) =
                    try_from_slice_unchecked(&instruction.data)
                {
                    compute_unit_limit_is_set = true;
                }
            }
        }

        // calculate bpf cost based on compute budget instructions
        let mut compute_budget = ComputeBudget::default();

        let result = compute_budget.process_instructions(
            transaction.message().program_instructions_iter(),
            !feature_set.is_active(&remove_deprecated_request_unit_ix::id()),
            feature_set.is_active(&add_set_tx_loaded_accounts_data_size_instruction::id()),
        );

        // if failed to process compute_budget instructions, the transaction will not be executed
        // by `bank`, therefore it should be considered as no execution cost by cost model.
        match result {
            Ok(_) => {
                // if tx contained user-space instructions and a more accurate estimate available correct it,
                // where "user-space instructions" must be specifically checked by
                // 'compute_unit_limit_is_set' flag, because compute_budget does not distinguish
                // builtin and bpf instructions when calculating default compute-unit-limit. (see
                // compute_budget.rs test `test_process_mixed_instructions_without_compute_budget`)
                if bpf_costs > 0 && compute_unit_limit_is_set {
                    bpf_costs = compute_budget.compute_unit_limit
                }

                if feature_set
                    .is_active(&include_loaded_accounts_data_size_in_fee_calculation::id())
                {
                    loaded_accounts_data_size_cost =
                        Self::calculate_loaded_accounts_data_size_cost(&compute_budget);
                }
            }
            Err(_) => {
                builtin_costs = 0;
                bpf_costs = 0;
            }
        }

        tx_cost.builtins_execution_cost = builtin_costs;
        tx_cost.bpf_execution_cost = bpf_costs;
        tx_cost.loaded_accounts_data_size_cost = loaded_accounts_data_size_cost;
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
        solana_sdk::{
            compute_budget::{self, ComputeBudgetInstruction},
            fee::ACCOUNT_DATA_COST_PAGE_SIZE,
            hash::Hash,
            instruction::{CompiledInstruction, Instruction},
            message::Message,
            signature::{Keypair, Signer},
            system_instruction::{self},
            system_program, system_transaction,
            transaction::Transaction,
        },
    };

    fn test_setup() -> (Keypair, Hash) {
        solana_logger::setup();
        (Keypair::new(), Hash::new_unique())
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

        let mut tx_cost = UsageCostDetails::default();
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
            vec![Pubkey::new_unique()],
            instructions,
        );
        let token_transaction = SanitizedTransaction::from_transaction_for_tests(tx);
        debug!("token_transaction {:?}", token_transaction);

        let mut tx_cost = UsageCostDetails::default();
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
            vec![Pubkey::new_unique(), compute_budget::id()],
            instructions,
        );
        let token_transaction = SanitizedTransaction::from_transaction_for_tests(tx);

        let mut tx_cost = UsageCostDetails::default();
        CostModel::get_transaction_cost(
            &mut tx_cost,
            &token_transaction,
            &FeatureSet::all_enabled(),
        );
        assert_eq!(
            *BUILT_IN_INSTRUCTION_COSTS
                .get(&compute_budget::id())
                .unwrap(),
            tx_cost.builtins_execution_cost
        );
        assert_eq!(12_345, tx_cost.bpf_execution_cost);
        assert_eq!(1, tx_cost.data_bytes_cost);
    }

    #[test]
    fn test_cost_model_with_failed_compute_budget_transaction() {
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
            // to trigger `duplicate_instruction_error` error
            CompiledInstruction::new_from_raw_parts(
                4,
                ComputeBudgetInstruction::SetComputeUnitLimit(1_000)
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
            vec![Pubkey::new_unique(), compute_budget::id()],
            instructions,
        );
        let token_transaction = SanitizedTransaction::from_transaction_for_tests(tx);

        let mut tx_cost = UsageCostDetails::default();
        CostModel::get_transaction_cost(
            &mut tx_cost,
            &token_transaction,
            &FeatureSet::all_enabled(),
        );
        assert_eq!(0, tx_cost.builtins_execution_cost);
        assert_eq!(0, tx_cost.bpf_execution_cost);
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

        let mut tx_cost = UsageCostDetails::default();
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
        let mut tx_cost = UsageCostDetails::default();
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
        assert_eq!(2 + 2, tx_cost.writable_accounts().len());
        assert_eq!(signer1.pubkey(), tx_cost.writable_accounts()[0]);
        assert_eq!(signer2.pubkey(), tx_cost.writable_accounts()[1]);
        assert_eq!(key1, tx_cost.writable_accounts()[2]);
        assert_eq!(key2, tx_cost.writable_accounts()[3]);
    }

    #[test]
    fn test_cost_model_calculate_cost_all_default() {
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
        // feature `include_loaded_accounts_data_size_in_fee_calculation` enabled, using
        // default loaded_accounts_data_size_limit
        const DEFAULT_PAGE_COST: u64 = 8;
        let expected_loaded_accounts_data_size_cost =
            solana_program_runtime::compute_budget::MAX_LOADED_ACCOUNTS_DATA_SIZE_BYTES as u64
                / ACCOUNT_DATA_COST_PAGE_SIZE
                * DEFAULT_PAGE_COST;

        let tx_cost = CostModel::calculate_cost(&tx, &FeatureSet::all_enabled());
        assert_eq!(expected_account_cost, tx_cost.write_lock_cost());
        assert_eq!(*expected_execution_cost, tx_cost.builtins_execution_cost());
        assert_eq!(2, tx_cost.writable_accounts().len());
        assert_eq!(
            expected_loaded_accounts_data_size_cost,
            tx_cost.loaded_accounts_data_size_cost()
        );
    }

    #[test]
    fn test_cost_model_calculate_cost_disabled_feature() {
        let (mint_keypair, start_hash) = test_setup();
        let tx = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &Keypair::new().pubkey(),
            2,
            start_hash,
        ));

        let feature_set = FeatureSet::default();
        assert!(!feature_set.is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()));
        let expected_account_cost = WRITE_LOCK_UNITS * 2;
        let expected_execution_cost = BUILT_IN_INSTRUCTION_COSTS
            .get(&system_program::id())
            .unwrap();
        // feature `include_loaded_accounts_data_size_in_fee_calculation` not enabled
        let expected_loaded_accounts_data_size_cost = 0;

        let tx_cost = CostModel::calculate_cost(&tx, &feature_set);
        assert_eq!(expected_account_cost, tx_cost.write_lock_cost());
        assert_eq!(*expected_execution_cost, tx_cost.builtins_execution_cost());
        assert_eq!(2, tx_cost.writable_accounts().len());
        assert_eq!(
            expected_loaded_accounts_data_size_cost,
            tx_cost.loaded_accounts_data_size_cost()
        );
    }

    #[test]
    fn test_cost_model_calculate_cost_enabled_feature_with_limit() {
        let (mint_keypair, start_hash) = test_setup();
        let to_keypair = Keypair::new();
        let data_limit = 32 * 1024u32;
        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    system_instruction::transfer(&mint_keypair.pubkey(), &to_keypair.pubkey(), 2),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_limit),
                ],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                start_hash,
            ));

        let feature_set = FeatureSet::all_enabled();
        assert!(feature_set.is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()));
        let expected_account_cost = WRITE_LOCK_UNITS * 2;
        let expected_execution_cost = BUILT_IN_INSTRUCTION_COSTS
            .get(&system_program::id())
            .unwrap()
            + BUILT_IN_INSTRUCTION_COSTS
                .get(&compute_budget::id())
                .unwrap();
        // feature `include_loaded_accounts_data_size_in_fee_calculation` is enabled, accounts data
        // size limit is set.
        let expected_loaded_accounts_data_size_cost = (data_limit as u64) / (32 * 1024) * 8;

        let tx_cost = CostModel::calculate_cost(&tx, &feature_set);
        assert_eq!(expected_account_cost, tx_cost.write_lock_cost());
        assert_eq!(expected_execution_cost, tx_cost.builtins_execution_cost());
        assert_eq!(2, tx_cost.writable_accounts().len());
        assert_eq!(
            expected_loaded_accounts_data_size_cost,
            tx_cost.loaded_accounts_data_size_cost()
        );
    }

    #[test]
    fn test_cost_model_calculate_cost_disabled_feature_with_limit() {
        let (mint_keypair, start_hash) = test_setup();
        let to_keypair = Keypair::new();
        let data_limit = 32 * 1024u32;
        let tx =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    system_instruction::transfer(&mint_keypair.pubkey(), &to_keypair.pubkey(), 2),
                    ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_limit),
                ],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                start_hash,
            ));

        let feature_set = FeatureSet::default();
        assert!(!feature_set.is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()));
        let expected_account_cost = WRITE_LOCK_UNITS * 2;
        // with features all disabled, builtins and loaded account size don't cost CU
        let expected_execution_cost = 0;
        let expected_loaded_accounts_data_size_cost = 0;

        let tx_cost = CostModel::calculate_cost(&tx, &feature_set);
        assert_eq!(expected_account_cost, tx_cost.write_lock_cost());
        assert_eq!(expected_execution_cost, tx_cost.builtins_execution_cost());
        assert_eq!(2, tx_cost.writable_accounts().len());
        assert_eq!(
            expected_loaded_accounts_data_size_cost,
            tx_cost.loaded_accounts_data_size_cost()
        );
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn test_calculate_loaded_accounts_data_size_cost() {
        let mut compute_budget = ComputeBudget::default();

        // accounts data size are priced in block of 32K, ...

        // ... requesting less than 32K should still be charged as one block
        compute_budget.loaded_accounts_data_size_limit = 31_usize * 1024;
        assert_eq!(
            compute_budget.heap_cost,
            CostModel::calculate_loaded_accounts_data_size_cost(&compute_budget)
        );

        // ... requesting exact 32K should be charged as one block
        compute_budget.loaded_accounts_data_size_limit = 32_usize * 1024;
        assert_eq!(
            compute_budget.heap_cost,
            CostModel::calculate_loaded_accounts_data_size_cost(&compute_budget)
        );

        // ... requesting slightly above 32K should be charged as 2 block
        compute_budget.loaded_accounts_data_size_limit = 33_usize * 1024;
        assert_eq!(
            compute_budget.heap_cost * 2,
            CostModel::calculate_loaded_accounts_data_size_cost(&compute_budget)
        );

        // ... requesting exact 64K should be charged as 2 block
        compute_budget.loaded_accounts_data_size_limit = 64_usize * 1024;
        assert_eq!(
            compute_budget.heap_cost * 2,
            CostModel::calculate_loaded_accounts_data_size_cost(&compute_budget)
        );
    }

    #[test]
    fn test_transaction_cost_with_mix_instruction_without_compute_budget() {
        let (mint_keypair, start_hash) = test_setup();

        let transaction =
            SanitizedTransaction::from_transaction_for_tests(Transaction::new_signed_with_payer(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0_u8, vec![]),
                    system_instruction::transfer(&mint_keypair.pubkey(), &Pubkey::new_unique(), 2),
                ],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                start_hash,
            ));
        // transaction has one builtin instruction, and one bpf instruction, no ComputeBudget::compute_unit_limit
        let expected_builtin_cost = *BUILT_IN_INSTRUCTION_COSTS
            .get(&solana_system_program::id())
            .unwrap();
        let expected_bpf_cost = DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT;

        let mut tx_cost = UsageCostDetails::default();
        CostModel::get_transaction_cost(&mut tx_cost, &transaction, &FeatureSet::all_enabled());

        assert_eq!(expected_builtin_cost, tx_cost.builtins_execution_cost);
        assert_eq!(expected_bpf_cost as u64, tx_cost.bpf_execution_cost);
    }
}
