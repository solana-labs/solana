#![cfg(feature = "full")]

use {
    crate::instruction::Instruction,
    borsh::{BorshDeserialize, BorshSchema, BorshSerialize},
};

crate::declare_id!("ComputeBudget111111111111111111111111111111");

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

impl ComputeBudgetInstruction {
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
}
