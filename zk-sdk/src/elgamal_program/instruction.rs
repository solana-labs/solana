//! Instructions provided by the [`ZK ElGamal proof`] program.
//!
//! There are two types of instructions in the proof program: proof verification instructions and
//! the `CloseContextState` instruction.
//!
//! Each proof verification instruction verifies a certain type of zero-knowledge proof. These
//! instructions are processed by the program in two steps:
//!   1. The program verifies the zero-knowledge proof.
//!   2. The program optionally stores the context component of the zero-knowledge proof to a
//!      dedicated [`context-state`] account.
//!
//! In step 1, the zero-knowledge proof can either be included directly as the instruction data or
//! pre-written to an account. The progrma determines whether the proof is provided as instruction
//! data or pre-written to an account by inspecting the length of the data. If the instruction data
//! is exactly 5 bytes (instruction discriminator + unsigned 32-bit integer), then the program
//! assumes that the first account provided with the instruction contains the zero-knowledge proof
//! and verifies the account data at the offset specified in the instruction data. Otherwise, the
//! program assumes that the zero-knowledge proof is provided as part of the instruction data.
//!
//! In step 2, the program determines whether to create a context-state account by inspecting the
//! number of accounts provided with the instruction. If two additional accounts are provided with
//! the instruction after verifying the zero-knowledge proof, then the program writes the context
//! data to the specified context-state account.
//!
//! NOTE: A context-state account must be pre-allocated to the exact size of the context data that
//! is expected for a proof type before it is included as part of a proof verification instruction.
//!
//! The `CloseContextState` instruction closes a context state account. A transaction containing
//! this instruction must be signed by the context account's owner. This instruction can be used by
//! the account owner to reclaim lamports for storage.
//!
//! [`ZK ElGamal proof`]: https://docs.solanalabs.com/runtime/zk-token-proof
//! [`context-state`]: https://docs.solanalabs.com/runtime/zk-token-proof#context-data

use {
    num_derive::{FromPrimitive, ToPrimitive},
    num_traits::ToPrimitive,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    },
};

#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofInstruction {
    /// Close a zero-knowledge proof context state.
    ///
    /// Accounts expected by this instruction:
    ///   0. `[writable]` The proof context account to close
    ///   1. `[writable]` The destination account for lamports
    ///   2. `[signer]` The context account's owner
    ///
    /// Data expected by this instruction:
    ///   None
    ///
    CloseContextState,
}

/// Pubkeys associated with a context state account to be used as parameters to functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextStateInfo<'a> {
    pub context_state_account: &'a Pubkey,
    pub context_state_authority: &'a Pubkey,
}

/// Create a `CloseContextState` instruction.
pub fn close_context_state(
    context_state_info: ContextStateInfo,
    destination_account: &Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(*context_state_info.context_state_account, false),
        AccountMeta::new(*destination_account, false),
        AccountMeta::new_readonly(*context_state_info.context_state_authority, true),
    ];

    let data = vec![ToPrimitive::to_u8(&ProofInstruction::CloseContextState).unwrap()];

    Instruction {
        program_id: crate::elgamal_program::id(),
        accounts,
        data,
    }
}
