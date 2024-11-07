//! The [secp256k1 native program][np].
//!
//! [np]: https://docs.solanalabs.com/runtime/programs#secp256k1-program
//!
//! Constructors for secp256k1 program instructions, and documentation on the
//! program's usage can be found in [`solana_sdk::secp256k1_instruction`].
//!
//! [`solana_sdk::secp256k1_instruction`]: https://docs.rs/solana-sdk/latest/solana_sdk/secp256k1_instruction/index.html
pub use solana_sdk_ids::secp256k1_program::{check_id, id, ID};
