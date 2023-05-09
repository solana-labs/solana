//! Sequences of [`Instruction`]s executed within a single transaction.
//!
//! [`Instruction`]: crate::instruction::Instruction
//!
//! In Solana, programs execute instructions, and clients submit sequences
//! of instructions to the network to be atomically executed as [`Transaction`]s.
//!
//! [`Transaction`]: https://docs.rs/solana-sdk/latest/solana-sdk/transaction/struct.Transaction.html
//!
//! A [`Message`] is the compact internal encoding of a transaction, as
//! transmitted across the network and stored in, and operated on, by the
//! runtime. It contains a flat array of all accounts accessed by all
//! instructions in the message, a [`MessageHeader`] that describes the layout
//! of that account array, a [recent blockhash], and a compact encoding of the
//! message's instructions.
//!
//! [recent blockhash]: https://docs.solana.com/developing/programming-model/transactions#recent-blockhash
//!
//! Clients most often deal with `Instruction`s and `Transaction`s, with
//! `Message`s being created by `Transaction` constructors.
//!
//! To ensure reliable network delivery, serialized messages must fit into the
//! IPv6 MTU size, conservatively assumed to be 1280 bytes. Thus constrained,
//! care must be taken in the amount of data consumed by instructions, and the
//! number of accounts they require to function.
//!
//! This module defines two versions of `Message` in their own modules:
//! [`legacy`] and [`v0`]. `legacy` is reexported here and is the current
//! version as of Solana 1.10.0. `v0` is a [future message format] that encodes
//! more account keys into a transaction than the legacy format. The
//! [`VersionedMessage`] type is a thin wrapper around either message version.
//!
//! [future message format]: https://docs.solana.com/proposals/versioned-transactions
//!
//! Despite living in the `solana-program` crate, there is no way to access the
//! runtime's messages from within a Solana program, and only the legacy message
//! types continue to be exposed to Solana programs, for backwards compatibility
//! reasons.

mod compiled_keys;
pub mod legacy;

#[cfg(not(target_os = "solana"))]
#[path = ""]
mod non_bpf_modules {
    mod account_keys;
    mod address_loader;
    mod sanitized;
    mod versions;

    pub use {account_keys::*, address_loader::*, sanitized::*, versions::*};
}

#[cfg(not(target_os = "solana"))]
pub use non_bpf_modules::*;
pub use {compiled_keys::CompileError, legacy::Message};

/// The length of a message header in bytes.
pub const MESSAGE_HEADER_LENGTH: usize = 3;

/// Describes the organization of a `Message`'s account keys.
///
/// Every [`Instruction`] specifies which accounts it may reference, or
/// otherwise requires specific permissions of. Those specifications are:
/// whether the account is read-only, or read-write; and whether the account
/// must have signed the transaction containing the instruction.
///
/// Whereas individual `Instruction`s contain a list of all accounts they may
/// access, along with their required permissions, a `Message` contains a
/// single shared flat list of _all_ accounts required by _all_ instructions in
/// a transaction. When building a `Message`, this flat list is created and
/// `Instruction`s are converted to [`CompiledInstruction`]s. Those
/// `CompiledInstruction`s then reference by index the accounts they require in
/// the single shared account list.
///
/// [`Instruction`]: crate::instruction::Instruction
/// [`CompiledInstruction`]: crate::instruction::CompiledInstruction
///
/// The shared account list is ordered by the permissions required of the accounts:
///
/// - accounts that are writable and signers
/// - accounts that are read-only and signers
/// - accounts that are writable and not signers
/// - accounts that are read-only and not signers
///
/// Given this ordering, the fields of `MessageHeader` describe which accounts
/// in a transaction require which permissions.
///
/// When multiple transactions access the same read-only accounts, the runtime
/// may process them in parallel, in a single [PoH] entry. Transactions that
/// access the same read-write accounts are processed sequentially.
///
/// [PoH]: https://docs.solana.com/cluster/synchronization
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, Copy, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    /// The number of signatures required for this message to be considered
    /// valid. The signers of those signatures must match the first
    /// `num_required_signatures` of [`Message::account_keys`].
    // NOTE: Serialization-related changes must be paired with the direct read at sigverify.
    pub num_required_signatures: u8,

    /// The last `num_readonly_signed_accounts` of the signed keys are read-only
    /// accounts.
    pub num_readonly_signed_accounts: u8,

    /// The last `num_readonly_unsigned_accounts` of the unsigned keys are
    /// read-only accounts.
    pub num_readonly_unsigned_accounts: u8,
}
