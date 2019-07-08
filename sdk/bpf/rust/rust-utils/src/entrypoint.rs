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

    let mut kas = Vec::new();
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

#[cfg(test)]
mod tests {
    extern crate std;

    use self::std::ffi::CStr;
    use self::std::println;
    use self::std::string::String;
    use super::*;
    use core::mem;

    #[no_mangle]
    fn sol_log_(message: *const u8) {
        let scenario = get_log_scenario();
        let c_str = unsafe { CStr::from_ptr(message as *const i8) };
        let string = c_str.to_str().unwrap();
        println!("{:?}", string);
    }

    #[no_mangle]
    fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
        println!("{:?} {:?} {:?} {:?} {:?}", arg1, arg2, arg3, arg4, arg5);
    }

    #[test]
    fn test_entrypoint() {
        set_log_scenario(4);
        set_log_64_scenario(4);
        let mut input: [u8; 154] = [
            1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 151, 116, 3, 85, 181, 39, 151, 99, 155,
            29, 208, 191, 255, 191, 11, 161, 4, 43, 104, 189, 202, 240, 231, 111, 146, 255, 199,
            71, 67, 34, 254, 68, 48, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 190, 103, 191,
            69, 193, 202, 38, 193, 95, 62, 131, 135, 105, 13, 142, 240, 155, 120, 177, 90, 212, 54,
            10, 118, 40, 33, 192, 8, 54, 141, 187, 63,
        ];

        if let Ok((mut ka, info, data)) = deserialize(&mut input[0] as *mut u8) {
            let account0 = match mem::replace(&mut ka[0], None) {
                Some(mut account0) => account0,
                None => {
                    panic!("Error: account not found");
                }
            };

            for k in ka[1..].iter() {
                if let Some(_) = k {
                    panic!("Too many keyed accounts found");
                }
            }
            assert_eq!(true, account0.is_signer);
            let key: &[u8; SIZE_PUBKEY] = &[
                151, 116, 3, 85, 181, 39, 151, 99, 155, 29, 208, 191, 255, 191, 11, 161, 4, 43,
                104, 189, 202, 240, 231, 111, 146, 255, 199, 71, 67, 34, 254, 68,
            ];
            assert_eq!(SIZE_PUBKEY, account0.key.key.len());
            assert_eq!(key, account0.key.key);
            assert_eq!(48, account0.lamports);
            assert_eq!(1, account0.data.len());
            let owner = &[0; SIZE_PUBKEY];
            assert_eq!(SIZE_PUBKEY, account0.owner.key.len());
            assert_eq!(owner, account0.owner.key);
            let d = [1, 0, 0, 0, 0, 0, 0, 0, 1];
            assert_eq!(9, data.len());
            assert_eq!(d, data);
            assert_eq!(1, info.tick_height);
            let program_id: &[u8; SIZE_PUBKEY] = &[
                190, 103, 191, 69, 193, 202, 38, 193, 95, 62, 131, 135, 105, 13, 142, 240, 155,
                120, 177, 90, 212, 54, 10, 118, 40, 33, 192, 8, 54, 141, 187, 63,
            ];
            assert_eq!(program_id, info.program_id.key);
        } else {
            panic!("Failed to deserialize");
        }
    }
}
