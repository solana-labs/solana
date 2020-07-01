//! @brief Example Rust-based BPF program tests loop iteration

extern crate solana_sdk;
use solana_sdk::{entrypoint::SUCCESS, info};

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
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
<<<<<<< HEAD
    // Pulls in the stubs required for `info!()`
    solana_sdk_bpf_test::stubs!();
=======
    // Pull in syscall stubs when building for non-BPF targets
    solana_sdk::program_stubs!();
>>>>>>> 52526a9bc... Prevent stub inclusion when building shared objects (#10875)

    #[test]
    fn test_entrypoint() {
        assert_eq!(SUCCESS, entrypoint(std::ptr::null_mut()));
    }
}
