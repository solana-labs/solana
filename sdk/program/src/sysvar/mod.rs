//! Access to special accounts with dynamically-updated data.
//!
//! For more details see the Solana [documentation on sysvars][sysvardoc].
//!
//! [sysvardoc]: https://docs.solana.com/developing/runtime-facilities/sysvars

use {
    crate::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey},
    lazy_static::lazy_static,
};

pub mod clock;
pub mod epoch_schedule;
pub mod fees;
pub mod instructions;
pub mod recent_blockhashes;
pub mod rent;
pub mod rewards;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake_history;

lazy_static! {
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

        #[cfg(test)]
        #[test]
        fn test_sysvar_id() {
            assert!($crate::sysvar::is_sysvar_id(&id()), "sysvar::is_sysvar_id() doesn't know about {}", $name);
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

        #[cfg(test)]
        #[test]
        fn test_sysvar_id() {
            assert!($crate::sysvar::is_sysvar_id(&id()), "sysvar::is_sysvar_id() doesn't know about {}", $name);
        }
    )
);

// Owner pubkey for sysvar accounts
crate::declare_id!("Sysvar1111111111111111111111111111111111111");

pub trait SysvarId {
    fn id() -> Pubkey;

    fn check_id(pubkey: &Pubkey) -> bool;
}

// Sysvar utilities
pub trait Sysvar:
    SysvarId + Default + Sized + serde::Serialize + serde::de::DeserializeOwned
{
    fn size_of() -> usize {
        bincode::serialized_size(&Self::default()).unwrap() as usize
    }

    /// Deserializes a sysvar from its `AccountInfo`.
    ///
    /// # Errors
    ///
    /// If `account_info` does not have the same ID as the sysvar
    /// this function returns [`ProgramError::InvalidArgument`].
    fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        if !Self::check_id(account_info.unsigned_key()) {
            return Err(ProgramError::InvalidArgument);
        }
        bincode::deserialize(&account_info.data.borrow()).map_err(|_| ProgramError::InvalidArgument)
    }
    fn to_account_info(&self, account_info: &mut AccountInfo) -> Option<()> {
        bincode::serialize_into(&mut account_info.data.borrow_mut()[..], self).ok()
    }
    fn get() -> Result<Self, ProgramError> {
        Err(ProgramError::UnsupportedSysvar)
    }
}

/// Implements the [`Sysvar::get`] method for both BPF and host targets.
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
