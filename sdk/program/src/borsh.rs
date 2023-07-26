//! Utilities for the [borsh] serialization format.
//!
//! This file re-exports the utilities for the latest version of borsh supported
//! by the Solana SDK.
//!
//! To avoid backwards-incompatibilities when the Solana SDK changes its dependency
//! on borsh, it's recommended to instead use the version-specific file directly,
//! ie. `borsh0_10`.
//!
//! This file remains for developers who want to use the latest by default.
//!
//! [borsh]: https://borsh.io/
pub use crate::borsh0_10::*;
