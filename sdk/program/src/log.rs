//! @brief Solana Rust-based BPF program logging

use crate::account_info::AccountInfo;

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
#[inline]
pub fn sol_log(message: &str) {
    #[cfg(target_arch = "bpf")]
    unsafe {
        sol_log_(message.as_ptr(), message.len() as u64);
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_log(message);
}

#[cfg(target_arch = "bpf")]
extern "C" {
    fn sol_log_(message: *const u8, len: u64);
}

/// Prints 64 bit values represented as hexadecimal to stdout
///
/// @param argx - integer arguments to print

#[inline]
pub fn sol_log_64(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    #[cfg(target_arch = "bpf")]
    unsafe {
        sol_log_64_(arg1, arg2, arg3, arg4, arg5);
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_log_64(arg1, arg2, arg3, arg4, arg5);
}

#[cfg(target_arch = "bpf")]
extern "C" {
    fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64);
}

/// Prints the hexadecimal representation of a slice
///
/// @param slice - The array to print
#[allow(dead_code)]
pub fn sol_log_slice(slice: &[u8]) {
    for (i, s) in slice.iter().enumerate() {
        info!(0, 0, 0, i, *s);
    }
}

/// Prints the hexadecimal representation of the program's input parameters
///
/// @param ka - A pointer to an array of `AccountInfo` to print
/// @param data - A pointer to the instruction data to print
#[allow(dead_code)]
pub fn sol_log_params(accounts: &[AccountInfo], data: &[u8]) {
    for (i, account) in accounts.iter().enumerate() {
        info!("AccountInfo");
        info!(0, 0, 0, 0, i);
        info!("- Is signer");
        info!(0, 0, 0, 0, account.is_signer);
        info!("- Key");
        account.key.log();
        info!("- Lamports");
        info!(0, 0, 0, 0, account.lamports());
        info!("- Account data length");
        info!(0, 0, 0, 0, account.data_len());
        info!("- Owner");
        account.owner.log();
    }
    info!("Instruction data");
    sol_log_slice(data);
}
