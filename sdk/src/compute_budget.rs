#![cfg(feature = "full")]

use crate::{
    process_instruction::BpfComputeBudget,
    transaction::{Transaction, TransactionError},
};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use solana_sdk::{
    borsh::try_from_slice_unchecked,
    instruction::{Instruction, InstructionError},
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

/// Create a `ComputeBudgetInstruction::RequestUnits` `Instruction`
pub fn request_units(units: u64) -> Instruction {
    Instruction::new_with_borsh(id(), &ComputeBudgetInstruction::RequestUnits(units), vec![])
}

pub fn process_request(
    compute_budget: &mut BpfComputeBudget,
    tx: &Transaction,
) -> Result<(), TransactionError> {
    let error = TransactionError::InstructionError(0, InstructionError::InvalidInstructionData);
    // Compute budget instruction must be in 1st or 2nd instruction (avoid nonce marker)
    for instruction in tx.message().instructions.iter().take(2) {
        if check_id(instruction.program_id(&tx.message().account_keys)) {
            let ComputeBudgetInstruction::RequestUnits(units) =
                try_from_slice_unchecked::<ComputeBudgetInstruction>(&instruction.data)
                    .map_err(|_| error.clone())?;
            if units > MAX_UNITS {
                return Err(error);
            }
            compute_budget.max_units = units;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compute_budget, hash::Hash, message::Message, pubkey::Pubkey, signature::Keypair,
        signer::Signer,
    };

    #[test]
    fn test_process_request() {
        let payer_keypair = Keypair::new();
        let mut compute_budget = BpfComputeBudget::default();

        let tx = Transaction::new(
            &[&payer_keypair],
            Message::new(&[], Some(&payer_keypair.pubkey())),
            Hash::default(),
        );
        process_request(&mut compute_budget, &tx).unwrap();
        assert_eq!(compute_budget, BpfComputeBudget::default());

        let tx = Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[
                    compute_budget::request_units(1),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        );
        process_request(&mut compute_budget, &tx).unwrap();
        assert_eq!(
            compute_budget,
            BpfComputeBudget {
                max_units: 1,
                ..BpfComputeBudget::default()
            }
        );

        let tx = Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[
                    compute_budget::request_units(MAX_UNITS + 1),
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                ],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        );
        let result = process_request(&mut compute_budget, &tx);
        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                0,
                InstructionError::InvalidInstructionData
            ))
        );

        let tx = Transaction::new(
            &[&payer_keypair],
            Message::new(
                &[
                    Instruction::new_with_bincode(Pubkey::new_unique(), &0, vec![]),
                    compute_budget::request_units(MAX_UNITS),
                ],
                Some(&payer_keypair.pubkey()),
            ),
            Hash::default(),
        );
        process_request(&mut compute_budget, &tx).unwrap();
        assert_eq!(
            compute_budget,
            BpfComputeBudget {
                max_units: MAX_UNITS,
                ..BpfComputeBudget::default()
            }
        );
    }
}
