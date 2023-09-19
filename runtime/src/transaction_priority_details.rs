use {
    solana_program_runtime::compute_budget::ComputeBudget,
    solana_sdk::{
        instruction::CompiledInstruction,
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, SanitizedVersionedTransaction},
    },
};

#[derive(Debug, PartialEq, Eq)]
pub struct TransactionPriorityDetails {
    pub priority: u64,
    pub compute_unit_limit: u64,
}

pub trait GetTransactionPriorityDetails {
    fn get_transaction_priority_details(
        &self,
        round_compute_unit_price_enabled: bool,
    ) -> Option<TransactionPriorityDetails>;

    fn process_compute_budget_instruction<'a>(
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
        _round_compute_unit_price_enabled: bool,
    ) -> Option<TransactionPriorityDetails> {
        let mut compute_budget = ComputeBudget::default();
        let prioritization_fee_details = compute_budget
            .process_instructions(
                instructions,
                true, // supports prioritization by request_units_deprecated instruction
                true, // enable support set accounts data size instruction
                      // TODO: round_compute_unit_price_enabled: bool
            )
            .ok()?;
        Some(TransactionPriorityDetails {
            priority: prioritization_fee_details.get_priority(),
            compute_unit_limit: compute_budget.compute_unit_limit,
        })
    }
}

impl GetTransactionPriorityDetails for SanitizedVersionedTransaction {
    fn get_transaction_priority_details(
        &self,
        round_compute_unit_price_enabled: bool,
    ) -> Option<TransactionPriorityDetails> {
        Self::process_compute_budget_instruction(
            self.get_message().program_instructions_iter(),
            round_compute_unit_price_enabled,
        )
    }
}

impl GetTransactionPriorityDetails for SanitizedTransaction {
    fn get_transaction_priority_details(
        &self,
        round_compute_unit_price_enabled: bool,
    ) -> Option<TransactionPriorityDetails> {
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
            compute_budget::{self, ComputeBudgetInstruction},
            instruction::Instruction,
            message::Message,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_instruction,
            transaction::{Transaction, VersionedTransaction},
        },
    };

    #[test]
    fn test_get_priority_with_valid_request_heap_frame_tx() {
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
            sanitized_versioned_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: 0,
                compute_unit_limit:
                    solana_program_runtime::compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                        as u64
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction =
            SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap();
        assert_eq!(
            sanitized_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: 0,
                compute_unit_limit:
                    solana_program_runtime::compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                        as u64
            })
        );
    }

    #[test]
    fn test_get_priority_with_valid_set_compute_units_limit() {
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
            sanitized_versioned_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: 0,
                compute_unit_limit: requested_cu as u64,
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction =
            SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap();
        assert_eq!(
            sanitized_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: 0,
                compute_unit_limit: requested_cu as u64,
            })
        );
    }

    #[test]
    fn test_get_priority_with_valid_set_compute_unit_price() {
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
            sanitized_versioned_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: requested_price,
                compute_unit_limit:
                    solana_program_runtime::compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                        as u64
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction =
            SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap();
        assert_eq!(
            sanitized_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: requested_price,
                compute_unit_limit:
                    solana_program_runtime::compute_budget::DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
                        as u64
            })
        );
    }

    #[test]
    fn test_get_priority_with_deprecated_compute_unit_request() {
        let priority = 1_000;
        let units = 200_000;
        let additional_fee = units * priority / 1_000_000;
        let keypair = Keypair::new();
        let transaction = Transaction::new_unsigned(Message::new(
            &[
                system_instruction::transfer(&keypair.pubkey(), &Pubkey::new_unique(), 1),
                Instruction::new_with_borsh(
                    compute_budget::id(),
                    &ComputeBudgetInstruction::RequestUnitsDeprecated {
                        units,
                        additional_fee,
                    },
                    vec![],
                ),
            ],
            Some(&keypair.pubkey()),
        ));

        // assert for SanitizedVersionedTransaction
        let versioned_transaction = VersionedTransaction::from(transaction.clone());
        let sanitized_versioned_transaction =
            SanitizedVersionedTransaction::try_new(versioned_transaction).unwrap();
        assert_eq!(
            sanitized_versioned_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: priority as u64,
                compute_unit_limit: units as u64
            })
        );

        // assert for SanitizedTransaction
        let sanitized_transaction =
            SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap();
        assert_eq!(
            sanitized_transaction.get_transaction_priority_details(false),
            Some(TransactionPriorityDetails {
                priority: priority as u64,
                compute_unit_limit: units as u64
            })
        );
    }
}
