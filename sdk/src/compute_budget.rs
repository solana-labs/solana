//! The compute budget native program.

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
    Eq,
    Serialize,
)]
pub enum ComputeBudgetInstruction {
    /// Deprecated
    // TODO: after feature remove_deprecated_request_unit_ix::id() is activated, replace it with 'unused'
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
    /// Set a specific compute unit limit that the transaction is allowed to consume.
    SetComputeUnitLimit(u32),
    /// Set a compute unit price in "micro-lamports" to pay a higher transaction
    /// fee for higher transaction prioritization.
    SetComputeUnitPrice(u64),
    /// Set a specific transaction-wide account data size limit, in bytes, is allowed to allocate.
    SetAccountsDataSizeLimit(u32),
}

impl ComputeBudgetInstruction {
    /// Create a `ComputeBudgetInstruction::RequestHeapFrame` `Instruction`
    pub fn request_heap_frame(bytes: u32) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::RequestHeapFrame(bytes), vec![])
    }

    /// Create a `ComputeBudgetInstruction::SetComputeUnitLimit` `Instruction`
    pub fn set_compute_unit_limit(units: u32) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::SetComputeUnitLimit(units), vec![])
    }

    /// Create a `ComputeBudgetInstruction::SetComputeUnitPrice` `Instruction`
    pub fn set_compute_unit_price(micro_lamports: u64) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::SetComputeUnitPrice(micro_lamports), vec![])
    }

    /// Create a `ComputeBudgetInstruction::SetAccountsDataSizeLimit` `Instruction`
    pub fn set_accounts_data_size_limit(bytes: u32) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::SetAccountsDataSizeLimit(bytes), vec![])
    }

    /// Serialize Instruction using borsh, this is only used in runtime::cost_model::tests but compilation
    /// can't be restricted as it's used across packages
    // #[cfg(test)]
    pub fn pack(self) -> Result<Vec<u8>, std::io::Error> {
        self.try_to_vec()
    }
}
