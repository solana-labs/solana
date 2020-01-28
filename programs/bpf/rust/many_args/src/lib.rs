//! @brief Example Rust-based BPF program tests loop iteration

mod helper;
extern crate solana_sdk;
use solana_sdk::{entrypoint::SUCCESS, info};

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    info!("Call same package");
    assert_eq!(crate::helper::many_args(1, 2, 3, 4, 5, 6, 7, 8, 9), 45);

    info!("Call another package");
    assert_eq!(
        solana_bpf_rust_many_args_dep::many_args(1, 2, 3, 4, 5, 6, 7, 8, 9),
        45
    );
    assert_eq!(
        solana_bpf_rust_many_args_dep::many_args_sret(1, 2, 3, 4, 5, 6, 7, 8, 9),
        solana_bpf_rust_many_args_dep::Ret {
            group1: 6,
            group2: 15,
            group3: 24
        }
    );

    SUCCESS
}

#[cfg(test)]
mod test {
    use super::*;
    // Pulls in the stubs required for `info!()`
    solana_sdk_bpf_test::stubs!();

    #[test]
    fn test_entrypoint() {
        assert_eq!(SUCCESS, entrypoint(std::ptr::null_mut()));
    }
}
