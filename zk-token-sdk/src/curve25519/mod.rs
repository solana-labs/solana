//! Syscall operations for curve25519
//!
//! This module lives inside the zk-token-sdk for now, but should move to a general location since
//! it is independent of zk-tokens.

pub mod curve_syscall_traits;
pub mod edwards;
#[cfg(not(target_os = "solana"))]
pub mod errors;
pub mod ristretto;
pub mod scalar;
