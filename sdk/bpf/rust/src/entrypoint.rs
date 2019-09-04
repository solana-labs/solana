//! @brief Solana Rust-based BPF program entrypoint and its parameter types

extern crate alloc;

use crate::account::Account;
use crate::pubkey::Pubkey;

use alloc::vec::Vec;
use std::mem::size_of;
use std::slice::{from_raw_parts, from_raw_parts_mut};

pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &mut [Account], data: &[u8]) -> bool;

/// Declare entrypoint of the program.
///
/// Deserialize the program input parameters and call
/// a user defined entrypoint.  Users must call
/// this function otherwise an entrypoint for
/// their program will not be created.
#[macro_export]
macro_rules! entrypoint {
    ($process_instruction:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> bool {
            unsafe {
                if let Ok((program_id, mut accounts, data)) = $crate::entrypoint::deserialize(input)
                {
                    $process_instruction(&program_id, &mut accounts, &data)
                } else {
                    false
                }
            }
        }
    };
}

/// Deserialize the input parameters
#[allow(clippy::type_complexity)]
pub unsafe fn deserialize<'a>(
    input: *mut u8,
) -> Result<(&'a Pubkey, Vec<Account<'a>>, &'a [u8]), ()> {
    let mut offset: usize = 0;

    // Number of Accounts present

    #[allow(clippy::cast_ptr_alignment)]
    let num_accounts = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    // Accounts

    let mut accounts = Vec::with_capacity(num_accounts);
    for _ in 0..num_accounts {
        let is_signer = {
            #[allow(clippy::cast_ptr_alignment)]
            let is_signer_val = *(input.add(offset) as *const u64);
            (is_signer_val != 0)
        };
        offset += size_of::<u64>();

        let key: &Pubkey = &*(input.add(offset) as *const Pubkey);
        offset += size_of::<Pubkey>();

        #[allow(clippy::cast_ptr_alignment)]
        let lamports = &mut *(input.add(offset) as *mut u64);
        offset += size_of::<u64>();

        #[allow(clippy::cast_ptr_alignment)]
        let data_length = *(input.add(offset) as *const u64) as usize;
        offset += size_of::<u64>();

        let data = { from_raw_parts_mut(input.add(offset), data_length) };
        offset += data_length;

        let owner: &Pubkey = &*(input.add(offset) as *const Pubkey);
        offset += size_of::<Pubkey>();

        accounts.push(Account {
            key,
            is_signer,
            lamports,
            data,
            owner,
        });
    }

    // Instruction data

    #[allow(clippy::cast_ptr_alignment)]
    let data_length = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    let data = { from_raw_parts(input.add(offset), data_length) };
    offset += data_length;

    // Program Id

    let program_id: &Pubkey = &*(input.add(offset) as *const Pubkey);

    Ok((program_id, accounts, data))
}
