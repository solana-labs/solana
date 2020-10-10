//! @brief Example Rust-based BPF program that issues a cross-program-invocation

pub mod instruction;
#[cfg(feature = "program")]
pub mod processor;
