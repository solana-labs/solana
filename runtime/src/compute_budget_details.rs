use {
    solana_program_runtime::compute_budget_processor::process_compute_budget_instructions,
    solana_sdk::{
        instruction::CompiledInstruction,
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, SanitizedVersionedTransaction},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputeBudgetDetails {
    pub compute_unit_price: u64,
    pub compute_unit_limit: u64,
}

pub trait GetComputeBudgetDetails {
    fn get_compute_budget_details(
        &self,
        round_compute_unit_price_enabled: bool,
    ) -> Option<ComputeBudgetDetails>;

    fn process_compute_budget_instruction<'a>(
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
        _round_compute_unit_price_enabled: bool,
    ) -> Option<ComputeBudgetDetails> {
        let compute_budget_limits = process_compute_budget_instructions(instructions).ok()?;
        Some(ComputeBudgetDetails {
            compute_unit_price: compute_budget_limits.compute_unit_price,
            compute_unit_limit: u64::from(compute_budget_limits.compute_unit_limit),
        })
    }
}

impl GetComputeBudgetDetails for SanitizedVersionedTransaction {
    fn get_compute_budget_details(
        &self,
        round_compute_unit_price_enabled: bool,
    ) -> Option<ComputeBudgetDetails> {
        Self::process_compute_budget_instruction(
            self.get_message().program_instructions_iter(),
            round_compute_unit_price_enabled,
        )
    }
}

impl GetComputeBudgetDetails for SanitizedTransaction {
    fn get_compute_budget_details(
        &self,
        round_compute_unit_price_enabled: bool,
    ) -> Option<ComputeBudgetDetails> {
        Self::process_compute_budget_instruction(
            self.message().program_instructions_iter(),
            round_compute_unit_price_enabled,
        )
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            message::Message,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_instruction,
            transaction::{Transaction, VersionedTransaction},
        },
    };

    #[test]
    fn test_get_compute_budget_details_with_valid_request_heap_frame_tx() {
        let keypair = Keypair::new();
        let transaction = Transaction::new_unsigned(Message::new(
            &[
                system_instruction::transfer(&keypair.pubkey(), &Pubkey::new_unique(), 1),
                ComputeBudgetInstruction::request_heap_frame(32 * 1024),
            ],
            Some(&keypair.pubkey()),
        ));

        // assert for SanitizedVersionedTransaction
        let versioned_transaction = VersionedTransaction::from(transaction.clone());
        let sanitized_versioned_transaction =
            SanitizedVersionedTransaction::try_new(versioned_transaction).unwrap();
        assert_eq!(
            sanitized_versioned_transaction.get_compute_budget_details(false),
            Some(ComputeBudgetDetails {
                compute_unit_price: 0,
                compute_unit_limit:
                    solana_program_runtime::compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                    as u64,
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction = SanitizedTransaction::from_transaction_for_tests(transaction);
        assert_eq!(
            sanitized_transaction.get_compute_budget_details(false),
            Some(ComputeBudgetDetails {
                compute_unit_price: 0,
                compute_unit_limit:
                    solana_program_runtime::compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                    as u64,
            })
        );
    }

    #[test]
    fn test_get_compute_budget_details_with_valid_set_compute_units_limit() {
        let requested_cu = 101u32;
        let keypair = Keypair::new();
        let transaction = Transaction::new_unsigned(Message::new(
            &[
                system_instruction::transfer(&keypair.pubkey(), &Pubkey::new_unique(), 1),
                ComputeBudgetInstruction::set_compute_unit_limit(requested_cu),
            ],
            Some(&keypair.pubkey()),
        ));

        // assert for SanitizedVersionedTransaction
        let versioned_transaction = VersionedTransaction::from(transaction.clone());
        let sanitized_versioned_transaction =
            SanitizedVersionedTransaction::try_new(versioned_transaction).unwrap();
        assert_eq!(
            sanitized_versioned_transaction.get_compute_budget_details(false),
            Some(ComputeBudgetDetails {
                compute_unit_price: 0,
                compute_unit_limit: requested_cu as u64,
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction = SanitizedTransaction::from_transaction_for_tests(transaction);
        assert_eq!(
            sanitized_transaction.get_compute_budget_details(false),
            Some(ComputeBudgetDetails {
                compute_unit_price: 0,
                compute_unit_limit: requested_cu as u64,
            })
        );
    }

    #[test]
    fn test_get_compute_budget_details_with_valid_set_compute_unit_price() {
        let requested_price = 1_000;
        let keypair = Keypair::new();
        let transaction = Transaction::new_unsigned(Message::new(
            &[
                system_instruction::transfer(&keypair.pubkey(), &Pubkey::new_unique(), 1),
                ComputeBudgetInstruction::set_compute_unit_price(requested_price),
            ],
            Some(&keypair.pubkey()),
        ));

        // assert for SanitizedVersionedTransaction
        let versioned_transaction = VersionedTransaction::from(transaction.clone());
        let sanitized_versioned_transaction =
            SanitizedVersionedTransaction::try_new(versioned_transaction).unwrap();
        assert_eq!(
            sanitized_versioned_transaction.get_compute_budget_details(false),
            Some(ComputeBudgetDetails {
                compute_unit_price: requested_price,
                compute_unit_limit:
                    solana_program_runtime::compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                    as u64,
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction = SanitizedTransaction::from_transaction_for_tests(transaction);
        assert_eq!(
            sanitized_transaction.get_compute_budget_details(false),
            Some(ComputeBudgetDetails {
                compute_unit_price: requested_price,
                compute_unit_limit:
                    solana_program_runtime::compute_budget_processor::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                    as u64,
            })
        );
    }
}
