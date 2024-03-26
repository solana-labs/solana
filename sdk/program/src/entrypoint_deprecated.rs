//! The Rust-based BPF program entrypoint supported by the original BPF loader.
//!
//! The original BPF loader is deprecated and exists for backwards-compatibility
//! reasons. This module should not be used by new programs.
//!
//! For more information see the [`bpf_loader_deprecated`] module.
//!
//! [`bpf_loader_deprecated`]: crate::bpf_loader_deprecated

#![allow(clippy::arithmetic_side_effects)]

extern crate alloc;
use {
    crate::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey},
    alloc::vec::Vec,
    std::{
        cell::RefCell,
        mem::size_of,
        rc::Rc,
        result::Result as ResultGeneric,
        slice::{from_raw_parts, from_raw_parts_mut},
    },
};

pub type ProgramResult = ResultGeneric<(), ProgramError>;

/// User implemented function to process an instruction
///
/// program_id: Program ID of the currently executing program
/// accounts: Accounts passed as part of the instruction
/// instruction_data: Instruction data
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;

/// Programs indicate success with a return value of 0
pub const SUCCESS: u64 = 0;

/// Declare the program entrypoint.
///
/// Deserialize the program input arguments and call
/// the user defined `process_instruction` function.
/// Users must call this macro otherwise an entrypoint for
/// their program will not be created.
#[macro_export]
macro_rules! entrypoint_deprecated {
    ($process_instruction:ident) => {
        /// # Safety
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
            let (program_id, accounts, instruction_data) =
                unsafe { $crate::entrypoint_deprecated::deserialize(input) };
            match $process_instruction(&program_id, &accounts, &instruction_data) {
                Ok(()) => $crate::entrypoint_deprecated::SUCCESS,
                Err(error) => error.into(),
            }
        }
    };
}

/// Deserialize the input arguments
///
/// # Safety
#[allow(clippy::type_complexity)]
pub unsafe fn deserialize<'a>(input: *mut u8) -> (&'a Pubkey, Vec<AccountInfo<'a>>, &'a [u8]) {
    let mut offset: usize = 0;

    // Number of accounts present

    #[allow(clippy::cast_ptr_alignment)]
    let num_accounts = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    // Account Infos

    let mut accounts = Vec::with_capacity(num_accounts);
    for _ in 0..num_accounts {
        let dup_info = *(input.add(offset) as *const u8);
        offset += size_of::<u8>();
        if dup_info == std::u8::MAX {
            #[allow(clippy::cast_ptr_alignment)]
            let is_signer = *(input.add(offset) as *const u8) != 0;
            offset += size_of::<u8>();

            #[allow(clippy::cast_ptr_alignment)]
            let is_writable = *(input.add(offset) as *const u8) != 0;
            offset += size_of::<u8>();

            let key: &Pubkey = &*(input.add(offset) as *const Pubkey);
            offset += size_of::<Pubkey>();

            #[allow(clippy::cast_ptr_alignment)]
            let lamports = Rc::new(RefCell::new(&mut *(input.add(offset) as *mut u64)));
            offset += size_of::<u64>();

            #[allow(clippy::cast_ptr_alignment)]
            let data_len = *(input.add(offset) as *const u64) as usize;
            offset += size_of::<u64>();

            let data = Rc::new(RefCell::new({
                from_raw_parts_mut(input.add(offset), data_len)
            }));
            offset += data_len;

            let owner: &Pubkey = &*(input.add(offset) as *const Pubkey);
            offset += size_of::<Pubkey>();

            #[allow(clippy::cast_ptr_alignment)]
            let executable = *(input.add(offset) as *const u8) != 0;
            offset += size_of::<u8>();

            #[allow(clippy::cast_ptr_alignment)]
            let rent_epoch = *(input.add(offset) as *const u64);
            offset += size_of::<u64>();

            accounts.push(AccountInfo {
                key,
                is_signer,
                is_writable,
                lamports,
                data,
                owner,
                executable,
                rent_epoch,
            });
        } else {
            // Duplicate account, clone the original
            accounts.push(accounts[dup_info as usize].clone());
        }
    }

    // Instruction data

    #[allow(clippy::cast_ptr_alignment)]
    let instruction_data_len = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    let instruction_data = { from_raw_parts(input.add(offset), instruction_data_len) };
    offset += instruction_data_len;

    // Program Id

    let program_id: &Pubkey = &*(input.add(offset) as *const Pubkey);

    (program_id, accounts, instruction_data)
}
