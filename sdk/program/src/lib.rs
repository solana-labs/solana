#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]
#![allow(clippy::integer_arithmetic)]

// Allows macro expansion of `use ::solana_program::*` to work within this crate
extern crate self as solana_program;

pub mod account_info;
pub mod bpf_loader;
pub mod bpf_loader_deprecated;
pub mod bpf_loader_upgradeable;
pub mod clock;
pub mod decode_error;
pub mod entrypoint;
pub mod entrypoint_deprecated;
pub mod epoch_schedule;
pub mod feature;
pub mod fee_calculator;
pub mod hash;
pub mod incinerator;
pub mod instruction;
pub mod loader_instruction;
pub mod loader_upgradeable_instruction;
pub mod log;
pub mod message;
pub mod native_token;
pub mod nonce;
pub mod program;
pub mod program_error;
pub mod program_option;
pub mod program_pack;
pub mod program_stubs;
pub mod pubkey;
pub mod rent;
pub mod sanitize;
pub mod secp256k1_program;
pub mod serialize_utils;
pub mod short_vec;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake_history;
pub mod system_instruction;
pub mod system_program;
pub mod sysvar;

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
/// use solana_program::{declare_id, pubkey::Pubkey};
///
/// # mod item_wrapper {
/// #   use solana_program::declare_id;
/// declare_id!("My11111111111111111111111111111111111111111");
/// # }
/// # use item_wrapper::id;
///
/// let my_id = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
/// assert_eq!(id(), my_id);
/// ```
pub use solana_sdk_macro::program_declare_id as declare_id;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_frozen_abi_macro;
