//! @brief Solana Rust-based BPF program logging

use crate::account_info::AccountInfo;

#[macro_export]
#[deprecated(since = "1.4.14", note = "Please use `msg` macro instead")]
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
    };
}

/// Print a message to the log
///
/// There are two fast forms:
/// 1. Single string: `msg!("hi")`
/// 2. 5 integers: `msg!(1, 2, 3, 4, 5)`
///
/// The third form is more generic and incurs a very large runtime overhead so it should be used
/// with care:
/// 3. Generalized format string: `msg!("Hello {}: 1, 2, {}", "World", 3)`
///
#[macro_export]
macro_rules! msg {
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
    };
    ($($arg:tt)*) => ($crate::log::sol_log(&format!($($arg)*)));
}

/// Print a string to the log
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

/// Print 64-bit values represented as hexadecimal to the log
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

/// Print some slices as base64
///
/// @param data - The slices to print
pub fn sol_log_data(data: &[&[u8]]) {
    #[cfg(target_arch = "bpf")]
    {
        extern "C" {
            fn sol_log_data(data: *const u8, data_len: u64);
        }

        unsafe { sol_log_data(data as *const _ as *const u8, data.len() as u64) };
    }

    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_log_data(data);
}

/// Print the hexadecimal representation of a slice
///
/// @param slice - The array to print
#[allow(dead_code)]
pub fn sol_log_slice(slice: &[u8]) {
    for (i, s) in slice.iter().enumerate() {
        msg!(0, 0, 0, i, *s);
    }
}

/// Print the hexadecimal representation of the program's input parameters
///
/// @param ka - A pointer to an array of `AccountInfo` to print
/// @param data - A pointer to the instruction data to print
#[allow(dead_code)]
pub fn sol_log_params(accounts: &[AccountInfo], data: &[u8]) {
    for (i, account) in accounts.iter().enumerate() {
        msg!("AccountInfo");
        msg!(0, 0, 0, 0, i);
        msg!("- Is signer");
        msg!(0, 0, 0, 0, account.is_signer);
        msg!("- Key");
        account.key.log();
        msg!("- Lamports");
        msg!(0, 0, 0, 0, account.lamports());
        msg!("- Account data length");
        msg!(0, 0, 0, 0, account.data_len());
        msg!("- Owner");
        account.owner.log();
    }
    msg!("Instruction data");
    sol_log_slice(data);
}

/// Print the remaining compute units the program may consume
#[inline]
pub fn sol_log_compute_units() {
    #[cfg(target_arch = "bpf")]
    unsafe {
        sol_log_compute_units_();
    }
    #[cfg(not(target_arch = "bpf"))]
    crate::program_stubs::sol_log_compute_units();
}

#[cfg(target_arch = "bpf")]
extern "C" {
    fn sol_log_compute_units_();
}
