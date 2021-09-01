#![cfg(feature = "full")]

use {
    crate::{
        borsh::try_from_slice_unchecked,
        instruction::{Instruction, InstructionError},
        transaction::{SanitizedTransaction, TransactionError},
    },
    borsh::{BorshDeserialize, BorshSchema, BorshSerialize},
};

crate::declare_id!("ComputeBudget111111111111111111111111111111");

const MAX_UNITS: u64 = 1_000_000;

/// Compute Budget Instructions
#[derive(
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Debug,
    Clone,
    PartialEq,
    AbiExample,
    AbiEnumVisitor,
)]
pub enum ComputeBudgetInstruction {
    /// Request a specific maximum number of compute units the transaction is
    /// allowed to consume.
    RequestUnits(u64),
}
impl ComputeBudgetInstruction {
    /// Create a `ComputeBudgetInstruction::RequestUnits` `Instruction`
    pub fn request_units(units: u64) -> Instruction {
        Instruction::new_with_borsh(id(), &ComputeBudgetInstruction::RequestUnits(units), vec![])
    }
}

#[derive(Clone, Copy, Debug, AbiExample, PartialEq)]
pub struct ComputeBudget {
    /// Number of compute units that an instruction is allowed.  Compute units
    /// are consumed by program execution, resources they use, etc...
    pub max_units: u64,
    /// Number of compute units consumed by a log_u64 call
    pub log_64_units: u64,
    /// Number of compute units consumed by a create_program_address call
    pub create_program_address_units: u64,
    /// Number of compute units consumed by an invoke call (not including the cost incurred by
    /// the called program)
    pub invoke_units: u64,
    /// Maximum cross-program invocation depth allowed
    pub max_invoke_depth: usize,
    /// Base number of compute units consumed to call SHA256
    pub sha256_base_cost: u64,
    /// Incremental number of units consumed by SHA256 (based on bytes)
    pub sha256_byte_cost: u64,
    /// Maximum BPF to BPF call depth
    pub max_call_depth: usize,
    /// Size of a stack frame in bytes, must match the size specified in the LLVM BPF backend
    pub stack_frame_size: usize,
    /// Number of compute units consumed by logging a `Pubkey`
    pub log_pubkey_units: u64,
    /// Maximum cross-program invocation instruction size
    pub max_cpi_instruction_size: usize,
    /// Number of account data bytes per conpute unit charged during a cross-program invocation
    pub cpi_bytes_per_unit: u64,
    /// Base number of compute units consumed to get a sysvar
    pub sysvar_base_cost: u64,
    /// Number of compute units consumed to call secp256k1_recover
    pub secp256k1_recover_cost: u64,
    /// Number of compute units consumed to do a syscall without any work
    pub syscall_base_cost: u64,
    /// Optional program heap region size, if `None` then loader default
    pub heap_size: Option<usize>,
}
impl Default for ComputeBudget {
    fn default() -> Self {
        Self::new()
    }
}
impl ComputeBudget {
    pub fn new() -> Self {
        ComputeBudget {
            max_units: 200_000,
            log_64_units: 100,
            create_program_address_units: 1500,
            invoke_units: 1000,
            max_invoke_depth: 4,
            sha256_base_cost: 85,
            sha256_byte_cost: 1,
            max_call_depth: 64,
            stack_frame_size: 4_096,
            log_pubkey_units: 100,
            max_cpi_instruction_size: 1280, // IPv6 Min MTU size
            cpi_bytes_per_unit: 250,        // ~50MB at 200,000 units
            sysvar_base_cost: 100,
            secp256k1_recover_cost: 25_000,
            syscall_base_cost: 100,
            heap_size: None,
        }
    }
    pub fn process_transaction(
        &mut self,
        tx: &SanitizedTransaction,
    ) -> Result<(), TransactionError> {
        let error = TransactionError::InstructionError(0, InstructionError::InvalidInstructionData);
        // Compute budget instruction must be in 1st or 2nd instruction (avoid nonce marker)
        for (program_id, instruction) in tx.message().program_instructions_iter().take(2) {
            if check_id(program_id) {
                let ComputeBudgetInstruction::RequestUnits(units) =
                    try_from_slice_unchecked::<ComputeBudgetInstruction>(&instruction.data)
                        .map_err(|_| error.clone())?;
                if units > MAX_UNITS {
                    return Err(error);
                }
                self.max_units = units;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hash::Hash, message::Message, pubkey::Pubkey, signature::Keypair, signer::Signer,
        transaction::Transaction,
    };
    use std::convert::TryInto;

    fn sanitize_tx(tx: Transaction) -> SanitizedTransaction {
        tx.try_into().unwrap()
    }

    #[test]
    fn test_process_transaction() {
        let payer_keypair = Keypair::new();
        let mut compute_budget = ComputeBudget::default();

        let tx = sanitize_tx(Transaction::new(
            &[&payer_keypair],
            Message::new(&[], Some(&payer_keypair.pubkey())),
            Hash::default(),
        ));
        compute_budget.process_transaction(&tx).unwrap();
        assert_eq!(compute_budget, ComputeBudget::default());

        let tx = sanitize_tx(Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[
                    ComputeBudgetInstruction::request_units(1),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        ));
        compute_budget.process_transaction(&tx).unwrap();
        assert_eq!(
            compute_budget,
            ComputeBudget {
                max_units: 1,
                ..ComputeBudget::default()
            }
        );

        let tx = sanitize_tx(Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[
                    ComputeBudgetInstruction::request_units(MAX_UNITS + 1),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        ));
        let result = compute_budget.process_transaction(&tx);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData
            ))
        );

        let tx = sanitize_tx(Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                    ComputeBudgetInstruction::request_units(MAX_UNITS),
                ],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        ));
        compute_budget.process_transaction(&tx).unwrap();
        assert_eq!(
            compute_budget,
            ComputeBudget {
                max_units: MAX_UNITS,
                ..ComputeBudget::default()
            }
        );
    }
}
