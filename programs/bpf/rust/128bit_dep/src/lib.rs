//! @brief Solana Rust-based BPF program utility functions and types

#![no_std]

extern crate solana_sdk_bpf_utils;

pub fn work(x: u128, y: u128) -> u128 {
    x + y
}

#[cfg(test)]
mod test {
    extern crate std;
    use super::*;

    #[test]
    fn test_work() {
        assert_eq!(3, work(1, 2));
    }
}
