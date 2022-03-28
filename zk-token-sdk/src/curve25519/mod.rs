//! Syscall operations for curve25519
//!
//! This module lives inside the zk-token-sdk for now, but should move to a general location since
//! it is independent of zk-tokens.

#[cfg(not(target_arch = "bpf"))]
pub mod curve_syscall_traits;
pub mod errors;
pub mod edwards;
pub mod ristretto;
pub mod scalar;
