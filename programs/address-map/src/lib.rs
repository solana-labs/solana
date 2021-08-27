#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

use solana_sdk::{clock::Epoch, declare_id};

pub mod instruction;
pub mod processor;
pub mod state;

declare_id!("AddressMap111111111111111111111111111111111");

/// The number of epochs required to deactivate an address map. If an address
/// map is deactivated in some slot in epoch X, it can no longer be used to map
/// addresses from the start of epoch X + 1.
///
/// # Notes
///
/// The deactivation cooldown time must be at least one epoch to ensure that
/// address map accounts cannot be activated more than once. Address maps
/// are derived from the authority and the current epoch when created.
pub const DEACTIVATION_COOLDOWN: Epoch = 1;
