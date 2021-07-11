//! named accounts for synthesized data accounts for bank state, etc.
//!
use crate::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

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

pub fn is_sysvar_id(id: &Pubkey) -> bool {
    clock::check_id(id)
        || epoch_schedule::check_id(id)
        || fees::check_id(id)
        || recent_blockhashes::check_id(id)
        || rent::check_id(id)
        || rewards::check_id(id)
        || slot_hashes::check_id(id)
        || slot_history::check_id(id)
        || stake_history::check_id(id)
        || instructions::check_id(id)
}

#[macro_export]
macro_rules! declare_sysvar_id(
    ($name:expr, $type:ty) => (
        $crate::declare_id!($name);

        impl $crate::sysvar::SysvarId for $type {
            fn check_id(pubkey: &$crate::pubkey::Pubkey) -> bool {
                check_id(pubkey)
            }
        }

        #[cfg(test)]
        #[test]
        fn test_sysvar_id() {
            if !$crate::sysvar::is_sysvar_id(&id()) {
                panic!("sysvar::is_sysvar_id() doesn't know about {}", $name);
            }
        }
    )
);

// Owner pubkey for sysvar accounts
crate::declare_id!("Sysvar1111111111111111111111111111111111111");

pub trait SysvarId {
    fn check_id(pubkey: &Pubkey) -> bool;
}

// Sysvar utilities
pub trait Sysvar:
    SysvarId + Default + Sized + serde::Serialize + serde::de::DeserializeOwned
{
    fn size_of() -> usize {
        bincode::serialized_size(&Self::default()).unwrap() as usize
    }
    fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        if !Self::check_id(account_info.unsigned_key()) {
            return Err(ProgramError::InvalidArgument);
        }
        bincode::deserialize(&account_info.data.borrow()).map_err(|_| ProgramError::InvalidArgument)
    }
    fn get() -> Result<Self, ProgramError> {
        Err(ProgramError::UnsupportedSysvar)
    }
}

#[macro_export]
macro_rules! impl_sysvar_get {
    ($syscall_name:ident) => {
        fn get() -> Result<Self, ProgramError> {
            let mut var = Self::default();
            let var_addr = &mut var as *mut _ as *mut u8;

            #[cfg(target_arch = "bpf")]
            let result = unsafe {
                extern "C" {
                    fn $syscall_name(var_addr: *mut u8) -> u64;
                }
                $syscall_name(var_addr)
            };
            #[cfg(not(target_arch = "bpf"))]
            let result = crate::program_stubs::$syscall_name(var_addr);

            match result {
                crate::entrypoint::SUCCESS => Ok(var),
                e => Err(e.into()),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pubkey::Pubkey;

    #[repr(C)]
    #[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
    struct TestSysvar {
        something: Pubkey,
    }
    crate::declare_id!("TestSysvar111111111111111111111111111111111");
    impl crate::sysvar::SysvarId for TestSysvar {
        fn check_id(pubkey: &crate::pubkey::Pubkey) -> bool {
            check_id(pubkey)
        }
    }
    impl Sysvar for TestSysvar {}
}
