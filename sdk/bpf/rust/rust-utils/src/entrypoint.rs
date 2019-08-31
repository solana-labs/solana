//! @brief Solana Rust-based BPF program entrypoint and its parameter types
extern crate alloc;

use alloc::vec::Vec;
use core::mem::size_of;
use core::slice::{from_raw_parts, from_raw_parts_mut};

/// Public key
pub type SolPubkey = [u8; 32];

/// Keyed Account
pub struct SolKeyedAccount<'a> {
    /// Public key of the account
    pub key: &'a SolPubkey,
    /// Public key of the account
    pub is_signer: bool,
    /// Number of lamports owned by this account
    pub lamports: &'a mut u64,
    /// On-chain data within this account
    pub data: &'a mut [u8],
    /// Program that owns this account
    pub owner: &'a SolPubkey,
}

/// Information about the state of the cluster immediately before the program
/// started executing the current instruction
pub struct SolClusterInfo<'a> {
    /// program_id of the currently executing program
    pub program_id: &'a SolPubkey,
}

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
                if let Ok((mut kas, info, data)) = $crate::entrypoint::deserialize(input) {
                    $process_instruction(&mut kas, &info, &data)
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
) -> Result<(Vec<SolKeyedAccount<'a>>, SolClusterInfo<'a>, &'a [u8]), ()> {
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

        let key: &SolPubkey = &*(input.add(offset) as *const [u8; size_of::<SolPubkey>()]);
        offset += size_of::<SolPubkey>();

        #[allow(clippy::cast_ptr_alignment)]
        let lamports = &mut *(input.add(offset) as *mut u64);
        offset += size_of::<u64>();

        #[allow(clippy::cast_ptr_alignment)]
        let data_length = *(input.add(offset) as *const u64) as usize;
        offset += size_of::<u64>();

        let data = { from_raw_parts_mut(input.add(offset), data_length) };
        offset += data_length;

        let owner: &SolPubkey = &*(input.add(offset) as *const [u8; size_of::<SolPubkey>()]);
        offset += size_of::<SolPubkey>();

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

    let program_id: &SolPubkey = &*(input.add(offset) as *const [u8; size_of::<SolPubkey>()]);
    let info = SolClusterInfo { program_id };

    Ok((kas, info, data))
}
