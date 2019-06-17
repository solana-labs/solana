//! @brief Solana Rust-based BPF program logging

use crate::entrypoint::{SolKeyedAccount, SolPubkey};

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
/// @param ka - A pointer to an array of `SolKeyedAccounts` to print
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
}
