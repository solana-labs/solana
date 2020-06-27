#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]

// Allows macro expansion of `use ::solana_sdk::*` to work within this crate
extern crate self as solana_sdk;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
pub mod abi_digester;
#[cfg(RUSTC_WITH_SPECIALIZATION)]
pub mod abi_example;

pub mod account;
pub mod account_utils;
pub mod bpf_loader;
pub mod clock;
pub mod commitment_config;
pub mod decode_error;
pub mod entrypoint_native;
pub mod epoch_info;
pub mod epoch_schedule;
pub mod fee_calculator;
pub mod hash;
pub mod incinerator;
pub mod inflation;
pub mod instruction;
pub mod loader_instruction;
pub mod message;
pub mod move_loader;
pub mod native_loader;
pub mod native_token;
pub mod nonce;
pub mod packet;
pub mod poh_config;
pub mod program_utils;
pub mod pubkey;
pub mod rent;
pub mod rpc_port;
pub mod sanitize;
pub mod short_vec;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake_history;
pub mod system_instruction;
pub mod system_program;
pub mod sysvar;
pub mod timing;

/// Convenience macro to declare a static public key and functions to interact with it
///
/// Input: a single literal base58 string representation of a program's id
///
/// # Example
///
/// ```
/// # // wrapper is used so that the macro invocation occurs in the item position
/// # // rather than in the statement position which isn't allowed.
/// use std::str::FromStr;
/// use solana_sdk::{declare_id, pubkey::Pubkey};
///
/// # mod item_wrapper {
/// #   use solana_sdk::declare_id;
/// declare_id!("My11111111111111111111111111111111111111111");
/// # }
/// # use item_wrapper::id;
///
/// let my_id = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
/// assert_eq!(id(), my_id);
/// ```
pub use solana_sdk_macro::declare_id;
pub use solana_sdk_macro::pubkeys;

// On-chain program specific modules
pub mod account_info;
pub mod entrypoint;
pub mod log;
pub mod program;
pub mod program_error;
pub mod program_stubs;

// Modules not usable by on-chain programs
#[cfg(not(feature = "program"))]
pub mod client;
#[cfg(not(feature = "program"))]
pub mod genesis_config;
#[cfg(not(feature = "program"))]
pub mod hard_forks;
#[cfg(not(feature = "program"))]
pub mod shred_version;
#[cfg(not(feature = "program"))]
pub mod signature;
#[cfg(not(feature = "program"))]
pub mod signers;
#[cfg(not(feature = "program"))]
pub mod system_transaction;
#[cfg(not(feature = "program"))]
pub mod transaction;
#[cfg(not(feature = "program"))]
pub mod transport;

#[macro_use]
extern crate serde_derive;
pub extern crate bs58;
extern crate log as logger;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
#[cfg(test)]
#[macro_use]
extern crate solana_sdk_macro_frozen_abi;

/// Parses a program-instruction enum to generate Instruction build functions and other utilities
///
/// `#[instructions(...)]` container attribute should contain the program id expression (eg.
/// `test_program::id()`)
///
/// Input:
///   * Annotated program-instruction enum. Requirements:
///     * Enum must either derive Serialize, or have a custom `serialize()` implementation
///     * Only named variant fields supported
///
/// Output:
///   * Documented program-instruction enum
///   * A helper function for each variant to generate an Instruction from account Pubkeys and field input
///   * A verbose enum that exposes account instruction information, as well as a `from_instruction` method for conversion
///
/// Account annotation: Variants should be tagged with the `accounts` attribute, containing a list
/// of accounts. Each account should be in the format `account_name(<attributes>)`, where
/// `<attributes>` are a comma-separated list.
///
/// Supported account attributes:
///   * `WRITABLE` - account should be loaded as read-write
///   * `SIGNER` - account is a signer of the transaction
///   * `optional` - account is optional, account utilities will be wrapped in an Option
///   * `multiple` - instruction supports multiple accounts with identical attributes, account utilities will be wrapped in a Vec
///   * `desc` - human-readable description of the account for documentation, name-value format: `desc = "<description>"`
///
///
/// Example input enum:
///
/// ```rust,ignore
/// #[instructions(test_program::id())]
/// #[derive(Clone, Debug, PartialEq, Serialize)]
/// pub enum TestInstruction {
///     /// Transfer lamports
///     #[accounts(
///         from_account(SIGNER, WRITABLE, desc = "Funding account"),
///         to_account(WRITABLE, desc = "Recipient account")
///     )]
///     Transfer {
///         /// The transfer amount
///         lamports: u64,
///     },
/// }
/// ```
///
///
/// Example output:
///
/// ```rust
/// use bincode::serialize;
/// use serde_derive::Serialize;
/// use solana_sdk::{
///     instruction::{AccountMeta, Instruction},
///     pubkey::Pubkey,
/// };
/// mod test_program {
///     solana_sdk::declare_id!("8dGutFWpfHymgGDV6is389USqGRqSfpGZyhBrF1VPWDg");
/// }
///
/// #[derive(Clone, Debug, PartialEq, Serialize)]
/// pub enum TestInstruction {
///     /// Transfer lamports
///     #[doc = "<br/>"]
///     #[doc = "* Accounts expected by this instruction:"]
///     #[doc = "  0. `[WRITABLE, SIGNER]` Funding account"]
///     #[doc = "  1. `[WRITABLE]` Recipient account"]
///     Transfer {
///         /// The transfer amount
///         lamports: u64
///     },
/// }
///
/// pub fn transfer(
///     from_account: Pubkey,
///     to_account: Pubkey,
///     lamports: u64
/// ) -> Instruction {
///     let mut accounts: Vec<AccountMeta> = vec![];
///     accounts.push(AccountMeta::new(from_account, true));
///     accounts.push(AccountMeta::new(to_account, false));
///
///     let data = serialize(&TestInstruction::Transfer{lamports}).unwrap();
///     Instruction {
///         program_id: test_program::id(),
///         data,
///         accounts,
///     }
/// }
///
/// #[derive(Clone, Debug, PartialEq, Serialize)]
/// pub enum TestInstructionVerbose {
///     /// Transfer lamports
///     Transfer {
///         from_account: u8,
///         to_account: u8,
///         /// The transfer amount
///         lamports: u64
///     },
/// }
///
/// impl TestInstructionVerbose {
///    pub fn from_instruction(instruction: TestInstruction, account_keys: Vec<u8>) -> Self {
///        match instruction {
///            TestInstruction::Transfer { lamports } => TestInstructionVerbose::Transfer {
///                from_account: account_keys[0],
///                to_account: account_keys[1],
///                lamports,
///             }
///         }
///     }
/// }
/// ```
pub use solana_sdk_program_macros::instructions;
