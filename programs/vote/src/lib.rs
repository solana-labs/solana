#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]

pub mod authorized_voters;
pub mod vote_instruction;
pub mod vote_state;
pub mod vote_transaction;

#[macro_use]
extern crate safecoin_metrics;

#[macro_use]
extern crate safecoin_frozen_abi_macro;

safecoin_sdk::declare_id!("Vote111111111111111111111111111111111111111");
