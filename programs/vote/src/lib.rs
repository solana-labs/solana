pub mod authorized_voters;
pub mod vote_instruction;
pub mod vote_state;
pub mod vote_transaction;

#[macro_use]
extern crate solana_metrics;

solana_sdk::declare_id!("Vote111111111111111111111111111111111111111");
