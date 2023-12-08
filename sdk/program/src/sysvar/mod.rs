//! Access to special accounts with dynamically-updated data.
//!
//! Sysvars are special accounts that contain dynamically-updated data about the
//! network cluster, the blockchain history, and the executing transaction. Each
//! sysvar is defined in its own submodule within this module. The [`clock`],
//! [`epoch_schedule`], [`instructions`], and [`rent`] sysvars are most useful
//! to on-chain programs.
//!
//! Simple sysvars implement the [`Sysvar::get`] method, which loads a sysvar
//! directly from the runtime, as in this example that logs the `clock` sysvar:
//!
//! ```
//! use solana_program::{
//!     account_info::AccountInfo,
//!     clock,
//!     entrypoint::ProgramResult,
//!     msg,
//!     pubkey::Pubkey,
//!     sysvar::Sysvar,
//! };
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let clock = clock::Clock::get()?;
//!     msg!("clock: {:#?}", clock);
//!     Ok(())
//! }
//! ```
//!
//! Since Solana sysvars are accounts, if the `AccountInfo` is provided to the
//! program, then the program can deserialize the sysvar with
//! [`Sysvar::from_account_info`] to access its data, as in this example that
//! again logs the [`clock`] sysvar.
//!
//! ```
//! use solana_program::{
//!     account_info::{next_account_info, AccountInfo},
//!     clock,
//!     entrypoint::ProgramResult,
//!     msg,
//!     pubkey::Pubkey,
//!     sysvar::Sysvar,
//! };
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!     let clock_account = next_account_info(account_info_iter)?;
//!     let clock = clock::Clock::from_account_info(&clock_account)?;
//!     msg!("clock: {:#?}", clock);
//!     Ok(())
//! }
//! ```
//!
//! When possible, programs should prefer to call `Sysvar::get` instead of
//! deserializing with `Sysvar::from_account_info`, as the latter imposes extra
//! overhead of deserialization while also requiring the sysvar account address
//! be passed to the program, wasting the limited space available to
//! transactions. Deserializing sysvars that can instead be retrieved with
//! `Sysvar::get` should be only be considered for compatibility with older
//! programs that pass around sysvar accounts.
//!
//! Some sysvars are too large to deserialize within a program, and
//! `Sysvar::from_account_info` returns an error, or the serialization attempt
//! will exhaust the program's compute budget. Some sysvars do not implement
//! `Sysvar::get` and return an error. Some sysvars have custom deserializers
//! that do not implement the `Sysvar` trait. These cases are documented in the
//! modules for individual sysvars.
//!
//! All sysvar accounts are owned by the account identified by [`sysvar::ID`].
//!
//! [`sysvar::ID`]: crate::sysvar::ID
//!
//! For more details see the Solana [documentation on sysvars][sysvardoc].
//!
//! [sysvardoc]: https://docs.solana.com/developing/runtime-facilities/sysvars

use {
    crate::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey},
    lazy_static::lazy_static,
};

pub mod clock;
pub mod epoch_rewards;
pub mod epoch_schedule;
pub mod fees;
pub mod instructions;
pub mod last_restart_slot;
pub mod recent_blockhashes;
pub mod rent;
pub mod rewards;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake_history;

lazy_static! {
    // This will be deprecated and so this list shouldn't be modified
    pub static ref ALL_IDS: Vec<Pubkey> = vec![
        clock::id(),
        epoch_schedule::id(),
        #[allow(deprecated)]
        fees::id(),
        #[allow(deprecated)]
        recent_blockhashes::id(),
        rent::id(),
        rewards::id(),
        slot_hashes::id(),
        slot_history::id(),
        stake_history::id(),
        instructions::id(),
    ];
}

/// Returns `true` of the given `Pubkey` is a sysvar account.
pub fn is_sysvar_id(id: &Pubkey) -> bool {
    ALL_IDS.iter().any(|key| key == id)
}

/// Declares an ID that implements [`SysvarId`].
#[macro_export]
macro_rules! declare_sysvar_id(
    ($name:expr, $type:ty) => (
        $crate::declare_id!($name);

        impl $crate::sysvar::SysvarId for $type {
            fn id() -> $crate::pubkey::Pubkey {
                id()
            }

            fn check_id(pubkey: &$crate::pubkey::Pubkey) -> bool {
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

        impl $crate::sysvar::SysvarId for $type {
            fn id() -> $crate::pubkey::Pubkey {
                #[allow(deprecated)]
                id()
            }

            fn check_id(pubkey: &$crate::pubkey::Pubkey) -> bool {
                #[allow(deprecated)]
                check_id(pubkey)
            }
        }
    )
);

// Owner pubkey for sysvar accounts
crate::declare_id!("Sysvar1111111111111111111111111111111111111");

/// A type that holds sysvar data and has an associated sysvar `Pubkey`.
pub trait SysvarId {
    /// The `Pubkey` of the sysvar.
    fn id() -> Pubkey;

    /// Returns `true` if the given pubkey is the program ID.
    fn check_id(pubkey: &Pubkey) -> bool;
}

/// A type that holds sysvar data.
pub trait Sysvar:
    SysvarId + Default + Sized + serde::Serialize + serde::de::DeserializeOwned
{
    /// The size in bytes of the sysvar as serialized account data.
    fn size_of() -> usize {
        bincode::serialized_size(&Self::default()).unwrap() as usize
    }

    /// Deserializes the sysvar from its `AccountInfo`.
    ///
    /// # Errors
    ///
    /// If `account_info` does not have the same ID as the sysvar this function
    /// returns [`ProgramError::InvalidArgument`].
    fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        if !Self::check_id(account_info.unsigned_key()) {
            return Err(ProgramError::InvalidArgument);
        }
        bincode::deserialize(&account_info.data.borrow()).map_err(|_| ProgramError::InvalidArgument)
    }

    /// Serializes the sysvar to `AccountInfo`.
    ///
    /// # Errors
    ///
    /// Returns `None` if serialization failed.
    fn to_account_info(&self, account_info: &mut AccountInfo) -> Option<()> {
        bincode::serialize_into(&mut account_info.data.borrow_mut()[..], self).ok()
    }

    /// Load the sysvar directly from the runtime.
    ///
    /// This is the preferred way to load a sysvar. Calling this method does not
    /// incur any deserialization overhead, and does not require the sysvar
    /// account to be passed to the program.
    ///
    /// Not all sysvars support this method. If not, it returns
    /// [`ProgramError::UnsupportedSysvar`].
    fn get() -> Result<Self, ProgramError> {
        Err(ProgramError::UnsupportedSysvar)
    }
}

/// Implements the [`Sysvar::get`] method for both SBF and host targets.
#[macro_export]
macro_rules! impl_sysvar_get {
    ($syscall_name:ident) => {
        fn get() -> Result<Self, ProgramError> {
            let mut var = Self::default();
            let var_addr = &mut var as *mut _ as *mut u8;

            #[cfg(target_os = "solana")]
            let result = unsafe { $crate::syscalls::$syscall_name(var_addr) };

            #[cfg(not(target_os = "solana"))]
            let result = $crate::program_stubs::$syscall_name(var_addr);

            match result {
                $crate::entrypoint::SUCCESS => Ok(var),
                e => Err(e.into()),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{clock::Epoch, program_error::ProgramError, pubkey::Pubkey},
        std::{cell::RefCell, rc::Rc},
    };

    #[repr(C)]
    #[derive(Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
    struct TestSysvar {
        something: Pubkey,
    }
    crate::declare_id!("TestSysvar111111111111111111111111111111111");
    impl crate::sysvar::SysvarId for TestSysvar {
        fn id() -> crate::pubkey::Pubkey {
            id()
        }

        fn check_id(pubkey: &crate::pubkey::Pubkey) -> bool {
            check_id(pubkey)
        }
    }
    impl Sysvar for TestSysvar {}

    #[test]
    fn test_sysvar_account_info_to_from() {
        let test_sysvar = TestSysvar::default();
        let key = crate::sysvar::tests::id();
        let wrong_key = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let mut lamports = 42;
        let mut data = vec![0_u8; TestSysvar::size_of()];
        let mut account_info = AccountInfo::new(
            &key,
            false,
            true,
            &mut lamports,
            &mut data,
            &owner,
            false,
            Epoch::default(),
        );

        test_sysvar.to_account_info(&mut account_info).unwrap();
        let new_test_sysvar = TestSysvar::from_account_info(&account_info).unwrap();
        assert_eq!(test_sysvar, new_test_sysvar);

        account_info.key = &wrong_key;
        assert_eq!(
            TestSysvar::from_account_info(&account_info),
            Err(ProgramError::InvalidArgument)
        );

        let mut small_data = vec![];
        account_info.data = Rc::new(RefCell::new(&mut small_data));
        assert_eq!(test_sysvar.to_account_info(&mut account_info), None);
    }
}
