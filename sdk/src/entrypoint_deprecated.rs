//! The Rust-based BPF program entrypoint supported by the original BPF loader.
//!
//! The original BPF loader is deprecated and exists for backwards-compatibility
//! reasons. This module should not be used by new programs.
//!
//! For more information see the [`bpf_loader_deprecated`] module.
//!
//! [`bpf_loader_deprecated`]: crate::bpf_loader_deprecated

pub use solana_program::entrypoint_deprecated::*;
