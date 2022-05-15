#![cfg(feature = "full")]

use {
    crate::instruction::Instruction,
    borsh::{BorshDeserialize, BorshSerialize},
};

crate::declare_id!("ComputeBudget111111111111111111111111111111");

/// Compute Budget Instructions
#[derive(
    AbiExample,
    AbiEnumVisitor,
    BorshDeserialize,
    BorshSerialize,
    Clone,
    Debug,
    Deserialize,
    PartialEq,
    Serialize,
)]
pub enum ComputeBudgetInstruction {
    /// Deprecated
    RequestUnitsDeprecated {
        /// Units to request
        units: u32,
        /// Additional fee to add
        additional_fee: u32,
    },
    /// Request a specific transaction-wide program heap region size in bytes.
    /// The value requested must be a multiple of 1024. This new heap region
    /// size applies to each program executed in the transaction, including all
    /// calls to CPIs.
    RequestHeapFrame(u32),
    /// Request a specific maximum number of compute units the transaction is
    /// allowed to consume and an additional fee to pay.
    RequestUnits(u32),
    /// Set a compute unit price in "micro-lamports" to pay a higher transaction
    /// fee for higher transaction prioritization.
    SetComputeUnitPrice(u64),
}

impl ComputeBudgetInstruction {
    /// Create a `ComputeBudgetInstruction::RequestHeapFrame` `Instruction`
    pub fn request_heap_frame(bytes: u32) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::RequestHeapFrame(bytes), vec![])
    }

    /// Create a `ComputeBudgetInstruction::RequestUnits` `Instruction`
    pub fn request_units(units: u32) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::RequestUnits(units), vec![])
    }

    /// Create a `ComputeBudgetInstruction::SetComputeUnitPrice` `Instruction`
    pub fn set_compute_unit_price(micro_lamports: u64) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::SetComputeUnitPrice(micro_lamports), vec![])
    }
}
