//! @brief Solana Rust-based BPF program utility functions and types

#![no_std]

use core::mem::size_of;
#[cfg(not(test))]
use core::panic::PanicInfo;
use core::slice::{from_raw_parts, from_raw_parts_mut};

/// Max number of accounts supported
pub const MAX_ACCOUNTS: usize = 10;

/// Size in bytes of a public key
pub const SIZE_PUBKEY: usize = 32;

/// Public key
pub struct SolPubkey<'a> {
    pub key: &'a [u8; SIZE_PUBKEY],
}

/// Keyed Account
pub struct SolKeyedAccount<'a> {
    /// Public key of the account
    pub key: SolPubkey<'a>,
    /// Public key of the account
    pub is_signer: bool,
    /// Number of lamports owned by this account
    pub lamports: u64,
    /// On-chain data within this account
    pub data: &'a mut [u8],
    /// Program that owns this account
    pub owner: SolPubkey<'a>,
}

/// Information about the state of the cluster immediately before the program
/// started executing the current instruction
pub struct SolClusterInfo<'a> {
    /// Current ledger tick
    pub tick_height: u64,
    ///program_id of the currently executing program
    pub program_id: SolPubkey<'a>,
}

/// Declare entrypoint of the program.
///
/// Deserialize the program input parameters and call
/// a user defined entrypoint.  Users must call
/// this function otherwise an entrypoint for
/// their program will not be created.
#[macro_export]
macro_rules! solana_entrypoint {
    ($process_instruction:ident) => {
        #[no_mangle]
        pub extern "C" fn entrypoint(input: *mut u8) -> bool {
            if let Ok((mut ka, info, data)) = $crate::deserialize(input) {
                // Call use function
                $process_instruction(&mut ka, &info, &data)
            } else {
                false
            }
        }
    };
}

/// Checks if a public key is default, default implies not set and is
/// represented as all zeros
///
/// @param key - Public key to check
pub fn sol_key_default(key_bytes: &[u8]) -> bool {
    for (_, k) in key_bytes.iter().enumerate() {
        if *k != 0 {
            return false;
        }
    }
    true
}

/// Prints a string to stdout
#[inline(never)] // stack intensive, prevent inline so everyone does not incur cost
pub fn sol_log(message: &str) {
    // TODO This is extremely slow, do something better
    let mut buf: [u8; 128] = [0; 128];
    for (i, b) in message.as_bytes().iter().enumerate() {
        if i >= 126 {
            break;
        }
        buf[i] = *b;
    }
    unsafe {
        sol_log_(buf.as_ptr());
    }
}
extern "C" {
    fn sol_log_(message: *const u8);
}

/// Prints 64 bit values represented as hexadecimal to stdout
pub fn sol_log_64(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    unsafe {
        sol_log_64_(arg1, arg2, arg3, arg4, arg5);
    }
}
extern "C" {
    fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64);
}

/// Prints the hexadecimal representation of a public key
///
/// @param - key The public key to print
#[allow(dead_code)]
pub fn sol_log_key(key: &SolPubkey) {
    for (i, k) in key.key.iter().enumerate() {
        sol_log_64(0, 0, 0, i as u64, u64::from(*k));
    }
}

/// Prints the hexadecimal representation of a slice
///
/// @param slice - The array to print
#[allow(dead_code)]
pub fn sol_log_slice(slice: &[u8]) {
    for (i, s) in slice.iter().enumerate() {
        sol_log_64(0, 0, 0, i as u64, u64::from(*s));
    }
}

/// Prints the hexadecimal representation of the program's input parameters
///
/// @param ka - A pointer to an array of SolKeyedAccounts to print
/// @param data - A pointer to the instruction data to print
#[allow(dead_code)]
pub fn sol_log_params(ka: &[Option<SolKeyedAccount>], data: &[u8]) {
    for (i, k) in ka.iter().enumerate() {
        if let Some(k) = k {
            sol_log("SolKeyedAccount");
            sol_log_64(0, 0, 0, 0, i as u64);
            sol_log("- Is signer");
            sol_log_64(0, 0, 0, 0, k.is_signer as u64);
            sol_log("- Key");
            sol_log_key(&k.key);
            sol_log("- Lamports");
            sol_log_64(0, 0, 0, 0, k.lamports);
            sol_log("- AccountData");
            sol_log_slice(k.data);
            sol_log("- Owner");
            sol_log_key(&k.owner);
        }
    }
    sol_log("Instruction data");
    sol_log_slice(data);
}

/// Deserialize the input parameters
pub fn deserialize<'a>(
    input: *mut u8,
) -> Result<
    (
        [Option<SolKeyedAccount<'a>>; MAX_ACCOUNTS],
        SolClusterInfo<'a>,
        &'a [u8],
    ),
    (),
> {
    let mut offset: usize = 0;

    // Number of KeyedAccounts present

    let num_ka = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let num_ka_ptr: *const u64 = input.add(offset) as *const u64;
        *num_ka_ptr as usize
    };
    offset += 8;

    // KeyedAccounts

    if num_ka > MAX_ACCOUNTS {
        sol_log("Error: Too many accounts");
        return Err(());
    }

    let mut ka: [Option<SolKeyedAccount>; MAX_ACCOUNTS] =
        [None, None, None, None, None, None, None, None, None, None];
    let iter = 0..num_ka; // This weirdness due to #issue $#4271
    for (i, _) in iter.enumerate() {
        let is_signer = unsafe {
            #[allow(clippy::cast_ptr_alignment)]
            let is_signer_ptr: *const u64 = input.add(offset) as *const u64;
            if *is_signer_ptr == 0 {
                false
            } else {
                true
            }
        };
        offset += size_of::<u64>();

        // let key_slice = unsafe { from_raw_parts(input.add(offset), SIZE_PUBKEY) };
        // let key = SolPubkey {
        //     key: key_slice.try_into().unwrap(),
        // };
        let key = unsafe {
            SolPubkey {
                key: &*(input.add(offset) as *mut [u8; SIZE_PUBKEY]),
            }
        };
        offset += SIZE_PUBKEY;

        let lamports = unsafe {
            #[allow(clippy::cast_ptr_alignment)]
            let lamports_ptr: *const u64 = input.add(offset) as *const u64;
            *lamports_ptr
        };
        offset += size_of::<u64>();

        let data_length = unsafe {
            #[allow(clippy::cast_ptr_alignment)]
            let data_length_ptr: *const u64 = input.add(offset) as *const u64;
            *data_length_ptr
        } as usize;
        offset += size_of::<u64>();

        let data = unsafe { from_raw_parts_mut(input.add(offset), data_length) };
        offset += data_length;

        // let owner_slice = unsafe { input.add(offset) as *mut [u8; SIZE_PUBKEY] };
        let owner = unsafe {
            SolPubkey {
                key: &*(input.add(offset) as *mut [u8; SIZE_PUBKEY]),
            }
        };
        offset += SIZE_PUBKEY;

        ka[i] = Some(SolKeyedAccount {
            key,
            is_signer,
            lamports,
            data,
            owner,
        });
    }

    // Instruction data

    let data_length = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let data_length_ptr: *const u64 = input.add(offset) as *const u64;
        *data_length_ptr
    } as usize;
    offset += size_of::<u64>();

    let data = unsafe { from_raw_parts(input.add(offset), data_length) };
    offset += data_length;

    // Tick height

    let tick_height = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let tick_height_ptr: *const u64 = input.add(offset) as *const u64;
        *tick_height_ptr
    };
    offset += size_of::<u64>();

    // Id

    let program_id = unsafe {
        SolPubkey {
            key: &*(input.add(offset) as *mut [u8; SIZE_PUBKEY]),
        }
    };

    let info = SolClusterInfo {
        tick_height,
        program_id,
    };

    Ok((ka, info, data))
}

// Panic handling
extern "C" {
    pub fn sol_panic_() -> !;
}
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    sol_log("Panic!");
    // TODO crashes! sol_log(_info.payload().downcast_ref::<&str>().unwrap());
    if let Some(location) = _info.location() {
        if !location.file().is_empty() {
            // TODO location.file() returns empty str, if we get here its been fixed
            sol_log(location.file());
            sol_log("location.file() is fixed!!");
            unsafe {
                sol_panic_();
            }
        }
        sol_log_64(0, 0, 0, location.line() as u64, location.column() as u64);
    } else {
        sol_log("Panic! but could not get location information");
    }
    unsafe {
        sol_panic_();
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use self::std::ffi::CStr;
    use self::std::println;
    use self::std::string::String;
    use super::*;
    use core::mem;

    static mut _LOG_SCENARIO: u64 = 4;
    fn get_log_scenario() -> u64 {
        unsafe { _LOG_SCENARIO }
    }
    fn set_log_scenario(test: u64) {
        unsafe { _LOG_SCENARIO = test };
    }

    #[no_mangle]
    fn sol_log_(message: *const u8) {
        let scenario = get_log_scenario();
        let c_str = unsafe { CStr::from_ptr(message as *const i8) };
        let string = c_str.to_str().unwrap();
        match scenario {
            1 => assert_eq!(string, "This is a test message"),
            2 => assert_eq!(string, "Attempted to log a string that is too long"),
            3 => {
                let s: String = ['a'; 126].iter().collect();
                assert_eq!(string, s);
            }
            4 => println!("{:?}", string),
            _ => panic!("Unknown sol_log test"),
        }
    }

    static mut _LOG_64_SCENARIO: u64 = 4;
    fn get_log_64_scenario() -> u64 {
        unsafe { _LOG_64_SCENARIO }
    }
    fn set_log_64_scenario(test: u64) {
        unsafe { _LOG_64_SCENARIO = test };
    }

    #[no_mangle]
    fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
        let scenario = get_log_64_scenario();
        match scenario {
            1 => {
                assert_eq!(1, arg1);
                assert_eq!(2, arg2);
                assert_eq!(3, arg3);
                assert_eq!(4, arg4);
                assert_eq!(5, arg5);
            }
            2 => {
                assert_eq!(0, arg1);
                assert_eq!(0, arg2);
                assert_eq!(0, arg3);
                assert_eq!(arg4 + 1, arg5);
            }
            3 => {
                assert_eq!(0, arg1);
                assert_eq!(0, arg2);
                assert_eq!(0, arg3);
                assert_eq!(arg4 + 1, arg5);
            }
            4 => println!("{:?} {:?} {:?} {:?} {:?}", arg1, arg2, arg3, arg4, arg5),
            _ => panic!("Unknown sol_log_64 test"),
        }
    }

    #[test]
    fn test_sol_log() {
        set_log_scenario(1);
        sol_log("This is a test message");
    }

    #[test]
    fn test_sol_log_long() {
        set_log_scenario(3);
        let s: String = ['a'; 256].iter().collect();
        sol_log(&s);
    }

    #[test]
    fn test_sol_log_max_length() {
        set_log_scenario(3);
        let s: String = ['a'; 126].iter().collect();
        sol_log(&s);
    }

    #[test]
    fn test_sol_log_64() {
        set_log_64_scenario(1);
        sol_log_64(1, 2, 3, 4, 5);
    }

    #[test]
    fn test_sol_log_key() {
        set_log_64_scenario(2);
        let key_array = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let key = SolPubkey { key: &key_array };
        sol_log_key(&key);
    }

    #[test]
    fn test_sol_log_slice() {
        set_log_64_scenario(3);
        let array = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        sol_log_slice(&array);
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
