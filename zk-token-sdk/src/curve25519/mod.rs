//! Syscall operations for curve25519
//!
//! This module lives inside the zk-token-sdk for now, but should move to a general location since
//! it is independent of zk-tokens.

#[cfg(not(target_arch = "bpf"))]
pub mod errors;
pub mod ops;
pub mod pod;
