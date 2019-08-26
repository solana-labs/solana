//! @brief Solana Rust-based BPF program utility functions and types

#![no_std]

extern crate solana_sdk_bpf_utils;

pub fn work(x: u128, y: u128) -> u128 {
    x + y
}

pub fn two_thirds(x: u128) -> u128 {
    2 * x / 3
}

#[cfg(test)]
mod test {
    extern crate std;
    use super::*;

    #[test]
    fn pull_in_externs() {
        // Rust on Linux excludes the solana_sdk_bpf_test library unless there is a
        // direct dependency, use this test to force the pull in of the library.
        // This is not necessary on macos and unfortunate on Linux
        // Issue #4972
        extern crate solana_sdk_bpf_test;
        use solana_sdk_bpf_test::*;
        unsafe { sol_log_("X".as_ptr(), 1) };
        sol_log_64_(1, 2, 3, 4, 5);
    }

    #[test]
    fn test_work() {
        assert_eq!(3, work(1, 2));
    }
}
