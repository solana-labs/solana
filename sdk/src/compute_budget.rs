#![cfg(feature = "full")]

use crate::{
    entrypoint::HEAP_LENGTH as MIN_HEAP_FRAME_BYTES,
    feature_set::{requestable_heap_size, FeatureSet},
    process_instruction::BpfComputeBudget,
    transaction::{Transaction, TransactionError},
};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use solana_sdk::{
    borsh::try_from_slice_unchecked,
    instruction::{Instruction, InstructionError},
};
use std::sync::Arc;

crate::declare_id!("ComputeBudget111111111111111111111111111111");

const MAX_UNITS: u32 = 1_000_000;
const MAX_HEAP_FRAME_BYTES: u32 = 256 * 1024;

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
    RequestUnits(u32),
    /// Request a specific transaction-wide program heap frame size in bytes.
    /// The value requested must be a multiple of 1024. This new heap frame size
    /// applies to each program executed, including all calls to CPIs.
    RequestHeapFrame(u32),
}

/// Create a `ComputeBudgetInstruction::RequestUnits` `Instruction`
pub fn request_units(units: u32) -> Instruction {
    Instruction::new_with_borsh(id(), &ComputeBudgetInstruction::RequestUnits(units), vec![])
}

/// Create a `ComputeBudgetInstruction::RequestHeapFrame` `Instruction`
pub fn request_heap_frame(bytes: u32) -> Instruction {
    Instruction::new_with_borsh(
        id(),
        &ComputeBudgetInstruction::RequestHeapFrame(bytes),
        vec![],
    )
}

pub fn process_request(
    compute_budget: &mut BpfComputeBudget,
    tx: &Transaction,
    feature_set: Arc<FeatureSet>,
) -> Result<(), TransactionError> {
    let error = TransactionError::InstructionError(0, InstructionError::InvalidInstructionData);
    // Compute budget instruction must be in the 1st 3 instructions (avoid
    // nonce marker), otherwise ignored
    for instruction in tx.message().instructions.iter().take(3) {
        if check_id(instruction.program_id(&tx.message().account_keys)) {
            match try_from_slice_unchecked(&instruction.data) {
                Ok(ComputeBudgetInstruction::RequestUnits(units)) => {
                    if units > MAX_UNITS {
                        return Err(error);
                    }
                    compute_budget.max_units = units as u64;
                }
                Ok(ComputeBudgetInstruction::RequestHeapFrame(bytes)) => {
                    if !feature_set.is_active(&requestable_heap_size::id())
                        || bytes > MAX_HEAP_FRAME_BYTES
                        || bytes < MIN_HEAP_FRAME_BYTES as u32
                        || bytes % 1024 != 0
                    {
                        return Err(error);
                    }
                    compute_budget.heap_size = Some(bytes as usize);
                }
                _ => return Err(error),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hash::Hash, message::Message, pubkey::Pubkey, signature::Keypair, signer::Signer};

    macro_rules! test {
        ( $instructions: expr, $expected_error: expr, $expected_budget: expr ) => {
            let payer_keypair = Keypair::new();
            let tx = Transaction::new(
                &[&payer_keypair],
                Message::new($instructions, Some(&payer_keypair.pubkey())),
                Hash::default(),
            );
            let feature_set = Arc::new(FeatureSet::all_enabled());
            let mut compute_budget = BpfComputeBudget::default();
            let result = process_request(&mut compute_budget, &tx, feature_set);
            assert_eq!($expected_error as Result<(), TransactionError>, result);
            assert_eq!(compute_budget, $expected_budget);
        };
    }

    #[test]
    fn test_process_request() {
        // Units
        test!(&[], Ok(()), BpfComputeBudget::default());
        test!(
            &[
                request_units(1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(()),
            BpfComputeBudget {
                max_units: 1,
                ..BpfComputeBudget::default()
            }
        );
        test!(
            &[
                request_units(MAX_UNITS + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            BpfComputeBudget::default()
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                request_units(MAX_UNITS),
            ],
            Ok(()),
            BpfComputeBudget {
                max_units: MAX_UNITS as u64,
                ..BpfComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                request_units(1),
            ],
            Ok(()),
            BpfComputeBudget::default()
        );

        // HeapFrame
        test!(&[], Ok(()), BpfComputeBudget::default());
        test!(
            &[
                request_heap_frame(40 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Ok(()),
            BpfComputeBudget {
                heap_size: Some(40 * 1024),
                ..BpfComputeBudget::default()
            }
        );
        test!(
            &[
                request_heap_frame(40 * 1024 + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            BpfComputeBudget::default()
        );
        test!(
            &[
                request_heap_frame(31 * 1024),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            BpfComputeBudget::default()
        );
        test!(
            &[
                request_heap_frame(MAX_HEAP_FRAME_BYTES + 1),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
            ],
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData,
            )),
            BpfComputeBudget::default()
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                request_heap_frame(MAX_HEAP_FRAME_BYTES),
            ],
            Ok(()),
            BpfComputeBudget {
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..BpfComputeBudget::default()
            }
        );
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                request_heap_frame(1), // ignored
            ],
            Ok(()),
            BpfComputeBudget::default()
        );

        // Combined
        test!(
            &[
                Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                request_heap_frame(MAX_HEAP_FRAME_BYTES),
                request_units(MAX_UNITS),
            ],
            Ok(()),
            BpfComputeBudget {
                max_units: MAX_UNITS as u64,
                heap_size: Some(MAX_HEAP_FRAME_BYTES as usize),
                ..BpfComputeBudget::default()
            }
        );
    }
}
