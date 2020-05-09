// Allows macro expansion of `use ::solana_sdk::*` to work within this crate
extern crate self as solana_sdk;

pub mod account;
pub mod account_utils;
pub mod bpf_loader;
pub mod clock;
pub mod commitment_config;
pub mod entrypoint_native;
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
