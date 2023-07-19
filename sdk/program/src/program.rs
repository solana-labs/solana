//! Cross-program invocation.
//!
//! Solana programs may call other programs, termed [_cross-program
//! invocations_][cpi] (CPI), with the [`invoke`] and [`invoke_signed`]
//! functions.
//!
//! [`invoke`]: invoke
//! [`invoke_signed`]: invoke_signed
//! [cpi]: https://docs.solana.com/developing/programming-model/calling-between-programs

use crate::{
    account_info::AccountInfo, entrypoint::ProgramResult, instruction::Instruction, pubkey::Pubkey,
    stable_layout::stable_instruction::StableInstruction,
};

/// Invoke a cross-program instruction.
///
/// Invoking one program from another program requires an [`Instruction`]
/// containing the program ID of the other program, instruction data that
/// will be understood by the other program, and a list of [`AccountInfo`]s
/// corresponding to all of the accounts accessed by the other program. Because
/// the only way for a program to acquire `AccountInfo` values is by receiving
/// them from the runtime at the [program entrypoint][entrypoint!], any account
/// required by the callee program must transitively be required by the caller
/// program, and provided by _its_ caller. The same is true of the program ID of
/// the called program.
///
/// [entrypoint!]: crate::entrypoint!
///
/// The `Instruction` is usually built from within the calling program, but may
/// be deserialized from an external source.
///
/// This function will not return if the called program returns anything other
/// than success. If the callee returns an error or aborts then the entire
/// transaction will immediately fail. To return data as the result of a
/// cross-program invocation use the [`set_return_data`] / [`get_return_data`]
/// functions, or have the callee write to a dedicated account for that purpose.
///
/// A program may directly call itself recursively, but may not be indirectly
/// called recursively (reentered) by another program. Indirect reentrancy will
/// cause the transaction to immediately fail.
///
/// # Validation of shared data between programs
///
/// The `AccountInfo` structures passed to this function contain data that is
/// directly accessed by the runtime and is copied to and from the memory space
/// of the called program. Some of that data, the [`AccountInfo::lamports`] and
/// [`AccountInfo::data`] fields, may be mutated as a side-effect of the called
/// program, if that program has writable access to the given account.
///
/// These two fields are stored in [`RefCell`]s to enforce the aliasing
/// discipline for mutated values required by the Rust language. Prior to
/// invoking the runtime, this routine will test that each `RefCell` is
/// borrowable as required by the callee and return an error if not.
///
/// The CPU cost of these runtime checks can be avoided with the unsafe
/// [`invoke_unchecked`] function.
///
/// [`RefCell`]: std::cell::RefCell
///
/// # Errors
///
/// If the called program completes successfully and violates no runtime
/// invariants, then this function will return successfully. If the callee
/// completes and returns a [`ProgramError`], then the transaction will
/// immediately fail. Control will not return to the caller.
///
/// Various runtime invariants are checked before the callee is invoked and
/// before returning control to the caller. If any of these invariants are
/// violated then the transaction will immediately fail. A non-exhaustive list
/// of these invariants includes:
///
/// - The sum of lamports owned by all referenced accounts has not changed.
/// - A program has not debited lamports from an account it does not own.
/// - A program has not otherwise written to an account that it does not own.
/// - A program has not written to an account that is not writable.
/// - The size of account data has not exceeded applicable limits.
///
/// If the invoked program does not exist or is not executable then
/// the transaction will immediately fail.
///
/// If any of the `RefCell`s within the provided `AccountInfo`s cannot be
/// borrowed in accordance with the call's requirements, an error of
/// [`ProgramError::AccountBorrowFailed`] is returned.
///
/// [`ProgramError`]: crate::program_error::ProgramError
/// [`ProgramError::AccountBorrowFailed`]: crate::program_error::ProgramError::AccountBorrowFailed
///
/// # Examples
///
/// A simple example of transferring lamports via CPI:
///
/// ```
/// use solana_program::{
///     account_info::{next_account_info, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     program::invoke,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
/// };
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let account_info_iter = &mut accounts.iter();
///
///     let payer = next_account_info(account_info_iter)?;
///     let recipient = next_account_info(account_info_iter)?;
///     // The system program is a required account to invoke a system
///     // instruction, even though we don't use it directly.
///     let system_program_account = next_account_info(account_info_iter)?;
///
///     assert!(payer.is_writable);
///     assert!(payer.is_signer);
///     assert!(recipient.is_writable);
///     assert!(system_program::check_id(system_program_account.key));
///
///     let lamports = 1000000;
///
///     invoke(
///         &system_instruction::transfer(payer.key, recipient.key, lamports),
///         &[payer.clone(), recipient.clone(), system_program_account.clone()],
///     )
/// }
/// ```
pub fn invoke(instruction: &Instruction, account_infos: &[AccountInfo]) -> ProgramResult {
    invoke_signed(instruction, account_infos, &[])
}

/// Invoke a cross-program instruction but don't enforce Rust's aliasing rules.
///
/// This function is like [`invoke`] except that it does not check that
/// [`RefCell`]s within [`AccountInfo`]s are properly borrowable as described in
/// the documentation for that function. Those checks consume CPU cycles that
/// this function avoids.
///
/// [`RefCell`]: std::cell::RefCell
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

/// Invoke a cross-program instruction with program signatures.
///
/// This function is like [`invoke`] with the additional ability to virtually
/// sign an invocation on behalf of one or more [program derived addresses][pda] (PDAs)
/// controlled by the calling program, allowing the callee to mutate them, or
/// otherwise confirm that a PDA program key has authorized the actions of the
/// callee.
///
/// There is no cryptographic signing involved &mdash; PDA signing is a runtime
/// construct that allows the calling program to control accounts as if it could
/// cryptographically sign for them; and the callee to treat the account as if it
/// was cryptographically signed.
///
/// The `signer_seeds` parameter is a slice of `u8` slices where the inner
/// slices represent the seeds plus the _bump seed_ used to derive (with
/// [`Pubkey::find_program_address`]) one of the PDAs within the `account_infos`
/// slice of `AccountInfo`s. During invocation, the runtime will re-derive the
/// PDA from the seeds and the calling program's ID, and if it matches one of
/// the accounts in `account_info`, will consider that account "signed".
///
/// [pda]: https://docs.solana.com/developing/programming-model/calling-between-programs#program-derived-addresses
///
/// See the documentation for [`Pubkey::find_program_address`] for more
/// about program derived addresses.
///
/// # Examples
///
/// A simple example of creating an account for a PDA:
///
/// ```
/// use solana_program::{
///     account_info::{next_account_info, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     program::invoke_signed,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
/// };
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let account_info_iter = &mut accounts.iter();
///     let payer = next_account_info(account_info_iter)?;
///     let vault_pda = next_account_info(account_info_iter)?;
///     let system_program = next_account_info(account_info_iter)?;
///
///     assert!(payer.is_writable);
///     assert!(payer.is_signer);
///     assert!(vault_pda.is_writable);
///     assert_eq!(vault_pda.owner, &system_program::ID);
///     assert!(system_program::check_id(system_program.key));
///
///     let vault_bump_seed = instruction_data[0];
///     let vault_seeds = &[b"vault", payer.key.as_ref(), &[vault_bump_seed]];
///     let expected_vault_pda = Pubkey::create_program_address(vault_seeds, program_id)?;
///
///     assert_eq!(vault_pda.key, &expected_vault_pda);
///
///     let lamports = 10000000;
///     let vault_size = 16;
///
///     invoke_signed(
///         &system_instruction::create_account(
///             &payer.key,
///             &vault_pda.key,
///             lamports,
///             vault_size,
///             &program_id,
///         ),
///         &[
///             payer.clone(),
///             vault_pda.clone(),
///         ],
///         &[
///             &[
///                 b"vault",
///                 payer.key.as_ref(),
///                 &[vault_bump_seed],
///             ],
///         ]
///     )?;
///     Ok(())
/// }
/// ```
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

/// Invoke a cross-program instruction with signatures but don't enforce Rust's
/// aliasing rules.
///
/// This function is like [`invoke_signed`] except that it does not check that
/// [`RefCell`]s within [`AccountInfo`]s are properly borrowable as described in
/// the documentation for that function. Those checks consume CPU cycles that
/// this function avoids.
///
/// [`RefCell`]: std::cell::RefCell
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
        let instruction = StableInstruction::from(instruction.clone());
        let result = unsafe {
            crate::syscalls::sol_invoke_signed_rust(
                &instruction as *const _ as *const u8,
                account_infos as *const _ as *const u8,
                account_infos.len() as u64,
                signers_seeds as *const _ as *const u8,
                signers_seeds.len() as u64,
            )
        };
        match result {
            crate::entrypoint::SUCCESS => Ok(()),
            _ => Err(result.into()),
        }
    }

    #[cfg(not(target_os = "solana"))]
    crate::program_stubs::sol_invoke_signed(instruction, account_infos, signers_seeds)
}

/// Maximum size that can be set using [`set_return_data`].
pub const MAX_RETURN_DATA: usize = 1024;

/// Set the running program's return data.
///
/// Return data is a dedicated per-transaction buffer for data passed
/// from cross-program invoked programs back to their caller.
///
/// The maximum size of return data is [`MAX_RETURN_DATA`]. Return data is
/// retrieved by the caller with [`get_return_data`].
pub fn set_return_data(data: &[u8]) {
    #[cfg(target_os = "solana")]
    unsafe {
        crate::syscalls::sol_set_return_data(data.as_ptr(), data.len() as u64)
    };

    #[cfg(not(target_os = "solana"))]
    crate::program_stubs::sol_set_return_data(data)
}

/// Get the return data from an invoked program.
///
/// For every transaction there is a single buffer with maximum length
/// [`MAX_RETURN_DATA`], paired with a [`Pubkey`] representing the program ID of
/// the program that most recently set the return data. Thus the return data is
/// a global resource and care must be taken to ensure that it represents what
/// is expected: called programs are free to set or not set the return data; and
/// the return data may represent values set by programs multiple calls down the
/// call stack, depending on the circumstances of transaction execution.
///
/// Return data is set by the callee with [`set_return_data`].
///
/// Return data is cleared before every CPI invocation &mdash; a program that
/// has invoked no other programs can expect the return data to be `None`; if no
/// return data was set by the previous CPI invocation, then this function
/// returns `None`.
///
/// Return data is not cleared after returning from CPI invocations &mdash; a
/// program that has called another program may retrieve return data that was
/// not set by the called program, but instead set by a program further down the
/// call stack; or, if a program calls itself recursively, it is possible that
/// the return data was not set by the immediate call to that program, but by a
/// subsequent recursive call to that program. Likewise, an external RPC caller
/// may see return data that was not set by the program it is directly calling,
/// but by a program that program called.
///
/// For more about return data see the [documentation for the return data proposal][rdp].
///
/// [rdp]: https://docs.solana.com/proposals/return-data
pub fn get_return_data() -> Option<(Pubkey, Vec<u8>)> {
    #[cfg(target_os = "solana")]
    {
        use std::cmp::min;

        let mut buf = [0u8; MAX_RETURN_DATA];
        let mut program_id = Pubkey::default();

        let size = unsafe {
            crate::syscalls::sol_get_return_data(
                buf.as_mut_ptr(),
                buf.len() as u64,
                &mut program_id,
            )
        };

        if size == 0 {
            None
        } else {
            let size = min(size as usize, MAX_RETURN_DATA);
            Some((program_id, buf[..size as usize].to_vec()))
        }
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
        crate::{clock::Epoch, instruction::AccountMeta},
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
