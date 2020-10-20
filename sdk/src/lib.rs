#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

// Allows macro expansion of `use ::solana_sdk::*` to work within this crate
extern crate self as solana_sdk;

pub mod account;
pub mod account_utils;
pub mod bpf_loader;
pub mod bpf_loader_deprecated;
pub mod builtins;
pub mod clock;
pub mod commitment_config;
pub mod decode_error;
pub mod deserialize_utils;
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
pub mod native_loader;
pub mod native_token;
pub mod nonce;
pub mod packet;
pub mod poh_config;
pub mod program_option;
pub mod program_pack;
pub mod program_utils;
pub mod pubkey;
pub mod rent;
pub mod rpc_port;
pub mod sanitize;
pub mod secp256k1_program;
pub mod short_vec;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake_history;
pub mod stake_weighted_timestamp;
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
#[rustversion::since(1.46.0)]
pub use solana_sdk_macro::respan;

// On-chain program specific modules
pub mod account_info;
pub mod entrypoint;
pub mod entrypoint_deprecated;
pub mod log;
pub mod program;
pub mod program_error;

#[cfg(all(feature = "program", not(target_arch = "bpf")))]
extern crate lazy_static;

#[cfg(all(feature = "program", not(target_arch = "bpf")))]
pub mod program_stubs;

// Unused `solana_sdk::program_stubs!()` macro retained for source backwards compatibility with v1.3.x programs
#[macro_export]
#[deprecated(
    since = "1.4.2",
    note = "program_stubs macro is obsolete and can be safely removed"
)]
macro_rules! program_stubs {
    () => {};
}

pub mod serialize_utils;

// Modules not usable by on-chain programs
#[cfg(feature = "everything")]
pub mod client;
#[cfg(feature = "everything")]
pub mod genesis_config;
#[cfg(feature = "everything")]
pub mod hard_forks;
#[cfg(feature = "everything")]
pub mod secp256k1;
#[cfg(feature = "everything")]
pub mod shred_version;
#[cfg(feature = "everything")]
pub mod signature;
#[cfg(feature = "everything")]
pub mod signers;
#[cfg(feature = "everything")]
pub mod system_transaction;
#[cfg(feature = "everything")]
pub mod transaction;
#[cfg(feature = "everything")]
pub mod transport;

#[macro_use]
extern crate serde_derive;
pub extern crate bs58;
extern crate log as logger;

#[macro_use]
extern crate solana_frozen_abi_macro;
