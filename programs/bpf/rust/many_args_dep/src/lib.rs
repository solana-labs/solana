//! @brief Solana Rust-based BPF program utility functions and types

#![no_std]

extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::info;

pub fn many_args(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
    arg7: u64,
    arg8: u64,
    arg9: u64,
) -> u64 {
    info!("another package");
    info!(arg1, arg2, arg3, arg4, arg5);
    info!(arg6, arg7, arg8, arg9, 0);
    arg1 + arg2 + arg3 + arg4 + arg5 + arg6 + arg7 + arg8 + arg9
}

#[cfg(test)]
mod test {
    extern crate solana_sdk_bpf_test;
    extern crate std;
    use super::*;

    #[no_mangle]
    pub unsafe fn sol_log_(message: *const u8, length: u64) {
        let slice = std::slice::from_raw_parts(message, length as usize);
        let string = std::str::from_utf8(&slice).unwrap();
        std::println!("{}", string);
    }

    #[no_mangle]
    pub fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
        std::println!("{} {} {} {} {}", arg1, arg2, arg3, arg4, arg5);
    }

    #[test]
    fn test_many_args() {
        assert_eq!(45, many_args(1, 2, 3, 4, 5, 6, 7, 8, 9));
    }
}
