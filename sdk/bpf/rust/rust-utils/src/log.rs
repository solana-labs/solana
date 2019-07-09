//! @brief Solana Rust-based BPF program logging

use crate::entrypoint::{SolKeyedAccount, SolPubkey};

/// Prints a string
/// There are two forms and are fast
/// 1. Single string
/// 2. 5 integers
#[macro_export]
macro_rules! info {
    ($msg:expr) => {
        $crate::log::sol_log($msg)
    };
    ($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr) => {
        $crate::log::sol_log_64(
            $arg1 as u64,
            $arg2 as u64,
            $arg3 as u64,
            $arg4 as u64,
            $arg5 as u64,
        )
    }; // `format!()` is not supported yet, Issue #3099
       // `format!()` incurs a very large runtime overhead so it should be used with care
       // ($($arg:tt)*) => ($crate::log::sol_log(&format!($($arg)*)));
}

/// Prints a string to stdout
///
/// @param message - Message to print
pub fn sol_log(message: &str) {
    unsafe {
        sol_log_(message.as_ptr(), message.len() as u64);
    }
}
extern "C" {
    fn sol_log_(message: *const u8, length: u64);
}

/// Prints 64 bit values represented as hexadecimal to stdout
///
/// @param argx - integer arguments to print
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
    for (i, k) in key.iter().enumerate() {
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
pub fn sol_log_params(ka: &[SolKeyedAccount], data: &[u8]) {
    for (i, k) in ka.iter().enumerate() {
        sol_log("SolKeyedAccount");
        sol_log_64(0, 0, 0, 0, i as u64);
        sol_log("- Is signer");
        sol_log_64(0, 0, 0, 0, k.is_signer as u64);
        sol_log("- Key");
        sol_log_key(&k.key);
        sol_log("- Lamports");
        sol_log_64(0, 0, 0, 0, *k.lamports);
        sol_log("- AccountData");
        sol_log_slice(k.data);
        sol_log("- Owner");
        sol_log_key(&k.owner);
    }
    sol_log("Instruction data");
    sol_log_slice(data);
}
