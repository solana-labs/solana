//! Wrappers around [`solana-cpi`] with support for overwriting
//! syscall stubs
//!
//! Solana programs may call other programs, termed [_cross-program
//! invocations_][cpi] (CPI), with the [`invoke`] and [`invoke_signed`]
//! functions.
//!
//! [`solana-cpi`]: https://docs.rs/solana-cpi/latest/solana_cpi/
//! [`invoke`]: invoke
//! [`invoke_signed`]: invoke_signed
//! [cpi]: https://solana.com/docs/core/cpi

pub use solana_cpi::MAX_RETURN_DATA;
use {
    crate::{
        account_info::AccountInfo, entrypoint::ProgramResult, instruction::Instruction,
        pubkey::Pubkey, stable_layout::stable_instruction::StableInstruction,
    },
    solana_clock::Epoch,
};

/// Like [`solana_cpi::invoke`], but with support
/// for overwriting the `sol_invoke_signed` syscall stub.
///
/// [`solana_cpi::invoke`]: https://docs.rs/solana-cpi/latest/solana_cpi/fn.invoke.html
pub fn invoke(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed(instruction, account_infos, &[])
}

/// Like [`solana_cpi::invoke_unchecked`], but with support
/// for overwriting the `sol_invoke_signed` syscall stub.
///
/// [`solana_cpi::invoke_unchecked`]: https://docs.rs/solana-cpi/latest/solana_cpi/fn.invoke_unchecked.html
///
/// # Safety
///
/// __This function is incorrectly missing an `unsafe` declaration.__
///
/// If any of the writable accounts passed to the callee contain data that is
/// borrowed within the calling program, and that data is written to by the
/// callee, then Rust's aliasing rules will be violated and cause undefined
/// behavior.
pub fn invoke_unchecked(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed_unchecked(instruction, account_infos, &[])
}

/// Like [`solana_cpi::invoke_signed`], but with support
/// for overwriting the `sol_invoke_signed` syscall stub.
///
/// [`solana_cpi::invoke_signed`]: https://docs.rs/solana-cpi/latest/solana_cpi/fn.invoke_signed.html
pub fn invoke_signed(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    // Check that the account RefCells are consistent with the request
    for account_meta in instruction.accounts.iter() {
        for account_info in account_infos.iter() {
            if account_meta.pubkey == *account_info.key {
                if account_meta.is_writable {
                    let _ = account_info.try_borrow_mut_lamports()?;
                    let _ = account_info.try_borrow_mut_data()?;
                } else {
                    let _ = account_info.try_borrow_lamports()?;
                    let _ = account_info.try_borrow_data()?;
                }
                break;
            }
        }
    }

    invoke_signed_unchecked(instruction, account_infos, signers_seeds)
}

/// Like [`solana_cpi::invoke_signed_unchecked`], but with support
/// for overwriting the `sol_invoke_signed` syscall stub.
///
/// [`solana_cpi::invoke_signed_unchecked`]: https://docs.rs/solana-cpi/latest/solana_cpi/fn.invoke_signed_unchecked.html
///
/// # Safety
///
/// __This function is incorrectly missing an `unsafe` declaration.__
///
/// If any of the writable accounts passed to the callee contain data that is
/// borrowed within the calling program, and that data is written to by the
/// callee, then Rust's aliasing rules will be violated and cause undefined
/// behavior.
pub fn invoke_signed_unchecked(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    signers_seeds: &[&[&[u8]]],
) -> ProgramResult {
    #[cfg(target_os = "solana")]
    {
        solana_cpi::invoke_signed_unchecked(instruction, account_infos, signers_seeds)
    }

    #[cfg(not(target_os = "solana"))]
    crate::program_stubs::sol_invoke_signed(instruction, account_infos, signers_seeds)
}

/// Like [`solana_cpi::set_return_data`], but with support
/// for overwriting the `sol_set_return_data` syscall stub.
///
/// [`solana_cpi::set_return_data`]: https://docs.rs/solana-cpi/latest/solana_cpi/fn.set_return_data.html
pub fn set_return_data(data: &[u8]) {
    #[cfg(target_os = "solana")]
    {
        solana_cpi::set_return_data(data);
    }

    #[cfg(not(target_os = "solana"))]
    crate::program_stubs::sol_set_return_data(data)
}

/// Like [`solana_cpi::get_return_data`], but with support
/// for overwriting the `sol_get_return_data` syscall stub.
///
/// [`solana_cpi::get_return_data`]: https://docs.rs/solana-cpi/latest/solana_cpi/fn.get_return_data.html
pub fn get_return_data() -> Option<(Pubkey, Vec<u8>)> {
    #[cfg(target_os = "solana")]
    {
        solana_cpi::get_return_data()
    }

    #[cfg(not(target_os = "solana"))]
    crate::program_stubs::sol_get_return_data()
}

/// Do sanity checks of type layout.
#[doc(hidden)]
#[allow(clippy::arithmetic_side_effects)]
pub fn check_type_assumptions() {
    extern crate memoffset;
    use {
        crate::instruction::AccountMeta,
        memoffset::offset_of,
        std::{
            cell::RefCell,
            mem::{align_of, size_of},
            rc::Rc,
            str::FromStr,
        },
    };

    // Code in this file assumes that u64 and usize are the same
    assert_eq!(size_of::<u64>(), size_of::<usize>());
    // Code in this file assumes that u8 is byte aligned
    assert_eq!(1, align_of::<u8>());

    // Enforce Instruction layout
    {
        assert_eq!(size_of::<AccountMeta>(), 32 + 1 + 1);

        let pubkey1 = Pubkey::from_str("J9PYCcoKusHyKRMXnBL17VTXC3MVETyqBG2KyLXVv6Ai").unwrap();
        let pubkey2 = Pubkey::from_str("Hvy4GHgPToZNoENTKjC4mJqpzWWjgTwXrFufKfxYiKkV").unwrap();
        let pubkey3 = Pubkey::from_str("JDMyRL8rCkae7maCSv47upNuBMFd3Mgos1fz2AvYzVzY").unwrap();
        let account_meta1 = AccountMeta {
            pubkey: pubkey2,
            is_signer: true,
            is_writable: false,
        };
        let account_meta2 = AccountMeta {
            pubkey: pubkey3,
            is_signer: false,
            is_writable: true,
        };
        let data = vec![1, 2, 3, 4, 5];
        let instruction = Instruction {
            program_id: pubkey1,
            accounts: vec![account_meta1.clone(), account_meta2.clone()],
            data: data.clone(),
        };
        let instruction = StableInstruction::from(instruction);
        let instruction_addr = &instruction as *const _ as u64;

        // program id
        assert_eq!(offset_of!(StableInstruction, program_id), 48);
        let pubkey_ptr = (instruction_addr + 48) as *const Pubkey;
        unsafe {
            assert_eq!(*pubkey_ptr, pubkey1);
        }

        // accounts
        assert_eq!(offset_of!(StableInstruction, accounts), 0);
        let accounts_ptr = (instruction_addr) as *const *const AccountMeta;
        let accounts_cap = (instruction_addr + 8) as *const usize;
        let accounts_len = (instruction_addr + 16) as *const usize;
        unsafe {
            assert_eq!(*accounts_cap, 2);
            assert_eq!(*accounts_len, 2);
            let account_meta_ptr = *accounts_ptr;
            assert_eq!(*account_meta_ptr, account_meta1);
            assert_eq!(*(account_meta_ptr.offset(1)), account_meta2);
        }

        // data
        assert_eq!(offset_of!(StableInstruction, data), 24);
        let data_ptr = (instruction_addr + 24) as *const *const [u8; 5];
        let data_cap = (instruction_addr + 24 + 8) as *const usize;
        let data_len = (instruction_addr + 24 + 16) as *const usize;
        unsafe {
            assert_eq!(*data_cap, 5);

            assert_eq!(*data_len, 5);
            let u8_ptr = *data_ptr;
            assert_eq!(*u8_ptr, data[..]);
        }
    }

    // Enforce AccountInfo layout
    {
        let key = Pubkey::from_str("6o8R9NsUxNskF1MfWM1f265y4w86JYbEwqCmTacdLkHp").unwrap();
        let mut lamports = 31;
        let mut data = vec![1, 2, 3, 4, 5];
        let owner = Pubkey::from_str("2tjK4XyNU54XdN9jokx46QzLybbLVGwQQvTfhcuBXAjR").unwrap();
        let account_info = AccountInfo {
            key: &key,
            is_signer: true,
            is_writable: false,
            lamports: Rc::new(RefCell::new(&mut lamports)),
            data: Rc::new(RefCell::new(&mut data)),
            owner: &owner,
            executable: true,
            rent_epoch: 42,
        };
        let account_info_addr = &account_info as *const _ as u64;

        // key
        assert_eq!(offset_of!(AccountInfo, key), 0);
        let key_ptr = (account_info_addr) as *const &Pubkey;
        unsafe {
            assert_eq!(**key_ptr, key);
        }

        // lamports
        assert_eq!(offset_of!(AccountInfo, lamports), 8);
        let lamports_ptr = (account_info_addr + 8) as *const Rc<RefCell<&mut u64>>;
        unsafe {
            assert_eq!(**(*lamports_ptr).as_ptr(), 31);
        }

        // data
        assert_eq!(offset_of!(AccountInfo, data), 16);
        let data_ptr = (account_info_addr + 16) as *const Rc<RefCell<&mut [u8]>>;
        unsafe {
            assert_eq!((*(*data_ptr).as_ptr())[..], data[..]);
        }

        // owner
        assert_eq!(offset_of!(AccountInfo, owner), 24);
        let owner_ptr = (account_info_addr + 24) as *const &Pubkey;
        unsafe {
            assert_eq!(**owner_ptr, owner);
        }

        // rent_epoch
        assert_eq!(offset_of!(AccountInfo, rent_epoch), 32);
        let renbt_epoch_ptr = (account_info_addr + 32) as *const Epoch;
        unsafe {
            assert_eq!(*renbt_epoch_ptr, 42);
        }

        // is_signer
        assert_eq!(offset_of!(AccountInfo, is_signer), 40);
        let is_signer_ptr = (account_info_addr + 40) as *const bool;
        unsafe {
            assert!(*is_signer_ptr);
        }

        // is_writable
        assert_eq!(offset_of!(AccountInfo, is_writable), 41);
        let is_writable_ptr = (account_info_addr + 41) as *const bool;
        unsafe {
            assert!(!*is_writable_ptr);
        }

        // executable
        assert_eq!(offset_of!(AccountInfo, executable), 42);
        let executable_ptr = (account_info_addr + 42) as *const bool;
        unsafe {
            assert!(*executable_ptr);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_check_type_assumptions() {
        super::check_type_assumptions()
    }
}
