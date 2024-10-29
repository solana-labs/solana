//! Access to special accounts with dynamically-updated data.
//!
//! Sysvars are special accounts that contain dynamically-updated data about the
//! network cluster, the blockchain history, and the executing transaction. Each
//! sysvar is defined in its own crate. The [`clock`], [`epoch_schedule`],
//! [`instructions`], and [`rent`] sysvars are most useful to on-chain programs.
//!
//! [`clock`]: https://docs.rs/solana-clock/latest
//! [`epoch_schedule`]: https://docs.rs/solana-epoch-schedule/latest
//! [`instructions`]: https://docs.rs/solana-program/latest/solana_program/sysvar/instructions
//! [`rent`]: https://docs.rs/solana-rent/latest
//!
//! All sysvar accounts are owned by the account identified by [`solana_sysvar::ID`].
//!
//! [`solana_sysvar::ID`]: crate::ID
//!
//! For more details see the Solana [documentation on sysvars][sysvardoc].
//!
//! [sysvardoc]: https://docs.solanalabs.com/runtime/sysvars

/// Re-export types required for macros
pub use solana_pubkey::{declare_deprecated_id, declare_id, Pubkey};

/// A type that holds sysvar data and has an associated sysvar `Pubkey`.
pub trait SysvarId {
    /// The `Pubkey` of the sysvar.
    fn id() -> Pubkey;

    /// Returns `true` if the given pubkey is the program ID.
    fn check_id(pubkey: &Pubkey) -> bool;
}

/// Declares an ID that implements [`SysvarId`].
#[macro_export]
macro_rules! declare_sysvar_id(
    ($name:expr, $type:ty) => (
        $crate::declare_id!($name);

        impl $crate::SysvarId for $type {
            fn id() -> $crate::Pubkey {
                id()
            }

            fn check_id(pubkey: &$crate::Pubkey) -> bool {
                check_id(pubkey)
            }
        }
    )
);

/// Same as [`declare_sysvar_id`] except that it reports that this ID has been deprecated.
#[macro_export]
macro_rules! declare_deprecated_sysvar_id(
    ($name:expr, $type:ty) => (
        $crate::declare_deprecated_id!($name);

        impl $crate::SysvarId for $type {
            fn id() -> $crate::Pubkey {
                #[allow(deprecated)]
                id()
            }

            fn check_id(pubkey: &$crate::Pubkey) -> bool {
                #[allow(deprecated)]
                check_id(pubkey)
            }
        }
    )
);

// Owner pubkey for sysvar accounts
solana_pubkey::declare_id!("Sysvar1111111111111111111111111111111111111");
