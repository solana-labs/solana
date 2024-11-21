//! Instructions for the compute budget native program.
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(feature = "frozen-abi", feature(min_specialization))]

#[cfg(feature = "borsh")]
use borsh::{BorshDeserialize, BorshSerialize};
use solana_instruction::Instruction;
pub use solana_sdk_ids::compute_budget::{check_id, id, ID};

/// Compute Budget Instructions
#[cfg_attr(
    feature = "frozen-abi",
    derive(
        solana_frozen_abi_macro::AbiExample,
        solana_frozen_abi_macro::AbiEnumVisitor
    )
)]
#[cfg_attr(feature = "borsh", derive(BorshSerialize, BorshDeserialize))]
#[cfg_attr(
    feature = "serde",
    derive(serde_derive::Deserialize, serde_derive::Serialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComputeBudgetInstruction {
    Unused, // deprecated variant, reserved value.
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
    /// Set a specific transaction-wide account data size limit, in bytes, is allowed to load.
    SetLoadedAccountsDataSizeLimit(u32),
}

macro_rules! to_instruction {
    ($discriminator: expr, $num: expr, $num_type: ty) => {{
        let mut data = [0u8; size_of::<$num_type>() + 1];
        data[0] = $discriminator;
        data[1..].copy_from_slice(&$num.to_le_bytes());
        Instruction {
            program_id: id(),
            data: data.to_vec(),
            accounts: vec![],
        }
    }};
}

impl ComputeBudgetInstruction {
    /// Create a `ComputeBudgetInstruction::RequestHeapFrame` `Instruction`
    pub fn request_heap_frame(bytes: u32) -> Instruction {
        to_instruction!(1, bytes, u32)
    }

    /// Create a `ComputeBudgetInstruction::SetComputeUnitLimit` `Instruction`
    pub fn set_compute_unit_limit(units: u32) -> Instruction {
        to_instruction!(2, units, u32)
    }

    /// Create a `ComputeBudgetInstruction::SetComputeUnitPrice` `Instruction`
    pub fn set_compute_unit_price(micro_lamports: u64) -> Instruction {
        to_instruction!(3, micro_lamports, u64)
    }

    /// Serialize Instruction using borsh, this is only used in runtime::cost_model::tests but compilation
    /// can't be restricted as it's used across packages
    #[cfg(feature = "dev-context-only-utils")]
    pub fn pack(self) -> Result<Vec<u8>, borsh::io::Error> {
        borsh::to_vec(&self)
    }

    /// Create a `ComputeBudgetInstruction::SetLoadedAccountsDataSizeLimit` `Instruction`
    pub fn set_loaded_accounts_data_size_limit(bytes: u32) -> Instruction {
        to_instruction!(4, bytes, u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_instruction() {
        let ix = ComputeBudgetInstruction::set_compute_unit_limit(257);
        assert_eq!(ix.data, vec![2, 1, 1, 0, 0]);
        let ix = ComputeBudgetInstruction::set_compute_unit_price(u64::MAX);
        assert_eq!(ix.data, vec![3, 255, 255, 255, 255, 255, 255, 255, 255]);
    }
}
