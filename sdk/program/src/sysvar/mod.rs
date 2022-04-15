//! named accounts for synthesized data accounts for bank state, etc.
//!
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

#[derive(Clone, Debug, PartialEq)]
pub enum SysvarType {
    Clock,
    EpochSchedule,
    #[deprecated]
    Fees,
    #[deprecated]
    RecentBlockhashes,
    Rent,
    Rewards,
    SlotHashes,
    SlotHistory,
    StakeHistory,
    Instructions,
}
impl SysvarType {
    pub fn from_pubkey(pubkey: &Pubkey) -> Option<Self> {
        #[allow(deprecated)]
        if pubkey == &clock::id() {
            Some(Self::Clock)
        } else if pubkey == &epoch_schedule::id() {
            Some(Self::EpochSchedule)
        } else if pubkey == &fees::id() {
            Some(Self::Fees)
        } else if pubkey == &recent_blockhashes::id() {
            Some(Self::RecentBlockhashes)
        } else if pubkey == &rent::id() {
            Some(Self::Rent)
        } else if pubkey == &rewards::id() {
            Some(Self::Rewards)
        } else if pubkey == &slot_hashes::id() {
            Some(Self::SlotHashes)
        } else if pubkey == &slot_history::id() {
            Some(Self::SlotHistory)
        } else if pubkey == &stake_history::id() {
            Some(Self::StakeHistory)
        } else if pubkey == &instructions::id() {
            Some(Self::Instructions)
        } else {
            None
        }
    }

    pub fn id(&self) -> Pubkey {
        match self {
            Self::Clock => clock::id(),
            Self::EpochSchedule => epoch_schedule::id(),
            #[allow(deprecated)]
            Self::Fees => fees::id(),
            #[allow(deprecated)]
            Self::RecentBlockhashes => recent_blockhashes::id(),
            Self::Rent => rent::id(),
            Self::Rewards => rewards::id(),
            Self::SlotHashes => slot_hashes::id(),
            Self::SlotHistory => slot_history::id(),
            Self::StakeHistory => stake_history::id(),
            Self::Instructions => instructions::id(),
        }
    }
}

pub fn is_sysvar_id(id: &Pubkey) -> bool {
    ALL_IDS.iter().any(|key| key == id)
}

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
    const TYPE: SysvarType;
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
    #[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
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
    impl Sysvar for TestSysvar {
        const TYPE: SysvarType = SysvarType::Instructions;
    }

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

    #[test]
    fn test_sysvar_type_from_pubkey() {
        assert_eq!(
            SysvarType::from_pubkey(&clock::id()).unwrap(),
            SysvarType::Clock
        );
        assert_eq!(
            SysvarType::from_pubkey(&epoch_schedule::id()).unwrap(),
            SysvarType::EpochSchedule
        );
        assert_eq!(
            SysvarType::from_pubkey(&rent::id()).unwrap(),
            SysvarType::Rent
        );
        assert_eq!(
            SysvarType::from_pubkey(&rewards::id()).unwrap(),
            SysvarType::Rewards
        );
        assert_eq!(
            SysvarType::from_pubkey(&slot_hashes::id()).unwrap(),
            SysvarType::SlotHashes
        );
        assert_eq!(
            SysvarType::from_pubkey(&slot_history::id()).unwrap(),
            SysvarType::SlotHistory
        );
        assert_eq!(
            SysvarType::from_pubkey(&stake_history::id()).unwrap(),
            SysvarType::StakeHistory
        );
        assert_eq!(
            SysvarType::from_pubkey(&instructions::id()).unwrap(),
            SysvarType::Instructions
        );
        #[allow(deprecated)]
        {
            assert_eq!(
                SysvarType::from_pubkey(&fees::id()).unwrap(),
                SysvarType::Fees
            );
            assert_eq!(
                SysvarType::from_pubkey(&recent_blockhashes::id()).unwrap(),
                SysvarType::RecentBlockhashes
            );
        }
        assert_eq!(SysvarType::from_pubkey(&Pubkey::new_unique()), None);
    }
}
