//! @brief Example Rust-based BPF program tests loop iteration

extern crate solana_sdk;
use solana_sdk::entrypoint::SUCCESS;
use solana_sdk::info;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    const ITERS: usize = 100;
    let ones = [1_u64; ITERS];
    let mut sum: u64 = 0;

    for v in ones.iter() {
        sum += *v;
    }
    info!(0xff, 0, 0, 0, sum);
    assert_eq!(sum, ITERS as u64);

    SUCCESS
}

#[cfg(test)]
mod test {
    use super::*;
    // Pulls in the stubs requried for `info!()`
    solana_sdk_bpf_test::stubs!();

    #[test]
    fn test_entrypoint() {
        assert_eq!(SUCCESS, entrypoint(std::ptr::null_mut()));
    }
}
