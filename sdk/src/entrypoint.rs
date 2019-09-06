//! @brief Solana Rust-based BPF program entrypoint and its parameter types
extern crate alloc;

use crate::pubkey::Pubkey;
use alloc::vec::Vec;
use core::mem::size_of;
use core::slice::{from_raw_parts, from_raw_parts_mut};

/// Keyed Account
pub struct SolKeyedAccount<'a> {
    /// Public key of the account
    pub key: &'a Pubkey,
    /// Public key of the account
    pub is_signer: bool,
    /// Number of lamports owned by this account
    pub lamports: &'a mut u64,
    /// On-chain data within this account
    pub data: &'a mut [u8],
    /// Program that owns this account
    pub owner: &'a Pubkey,
}

/// User implemented program entrypoint
///
/// program_id: Program ID of the currently executing program
/// accounts: Accounts passed as part of the instruction
/// data: Instruction data
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &mut [SolKeyedAccount], data: &[u8]) -> bool;

/// Programs indicate success with a return value of 0
pub const SUCCESS: u32 = 0;

/// Declare entrypoint of the program.
///
/// Deserialize the program input parameters and call
/// the user defined `ProcessInstruction`.  Users must call
/// this function otherwise an entrypoint for
/// their program will not be created.
#[macro_export]
macro_rules! entrypoint {
    ($process_instruction:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u32 {
               unsafe {
                let (program_id, mut kas, data) = $crate::entrypoint::deserialize(input);
                $process_instruction(&program_id, &mut kas, &data)
            }
        }
    };
}

/// Deserialize the input parameters
#[allow(clippy::type_complexity)]
pub unsafe fn deserialize<'a>(
    input: *mut u8,
) -> (&'a Pubkey, Vec<SolKeyedAccount<'a>>, &'a [u8]) {
    let mut offset: usize = 0;

    // Number of KeyedAccounts present

    #[allow(clippy::cast_ptr_alignment)]
    let num_ka = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    // KeyedAccounts

    let mut kas = Vec::with_capacity(num_ka);
    for _ in 0..num_ka {
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

        kas.push(SolKeyedAccount {
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

    (program_id, kas, data)
}
