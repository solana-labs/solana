//! @brief Solana Rust-based BPF program entry point and its parameter types

extern crate alloc;
use crate::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
use alloc::vec::Vec;
use std::{
    cell::RefCell,
    mem::size_of,
    rc::Rc,
    slice::{from_raw_parts, from_raw_parts_mut},
};

/// User implemented function to process an instruction
///
/// program_id: Program ID of the currently executing program
/// accounts: Accounts passed as part of the instruction
/// instruction_data: Instruction data
pub type ProcessInstruction = fn(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> Result<(), ProgramError>;

/// Programs indicate success with a return value of 0
pub const SUCCESS: u32 = 0;

/// Declare the entry point of the program.
///
/// Deserialize the program input arguments and call
/// the user defined `ProcessInstruction` function.
/// Users must call this macro otherwise an entry point for
/// their program will not be created.
#[macro_export]
macro_rules! entrypoint {
    ($process_instruction:ident) => {
        /// # Safety
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u32 {
            let (program_id, accounts, instruction_data) =
                unsafe { $crate::entrypoint::deserialize(input) };
            match $process_instruction(&program_id, &accounts, &instruction_data) {
                Ok(()) => $crate::entrypoint::SUCCESS,
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
        let dup_info = *(input.add(offset) as *const u8) as usize;
        offset += size_of::<u8>();
        if dup_info == 0 {
            let is_signer = {
                #[allow(clippy::cast_ptr_alignment)]
                let is_signer = *(input.add(offset) as *const u64);
                (is_signer != 0)
            };
            offset += size_of::<u64>();

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

            accounts.push(AccountInfo {
                is_signer,
                key,
                lamports,
                data,
                owner,
            });
        } else {
            // Duplicate account, clone the original
            accounts.push(accounts[dup_info].clone());
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
