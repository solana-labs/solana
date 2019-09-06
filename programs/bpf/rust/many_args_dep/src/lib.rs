//! @brief Solana Rust-based BPF program utility functions and types

extern crate solana_sdk;
use solana_sdk::info;

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
    info!("Another package");
    info!(arg1, arg2, arg3, arg4, arg5);
    info!(arg6, arg7, arg8, arg9, 0);
    arg1 + arg2 + arg3 + arg4 + arg5 + arg6 + arg7 + arg8 + arg9
}

#[derive(Debug, PartialEq)]
pub struct Ret {
    pub group1: u128,
    pub group2: u128,
    pub group3: u128,
}

pub fn many_args_sret(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
    arg7: u64,
    arg8: u64,
    arg9: u64,
) -> Ret {
    info!("Another package");
    info!(arg1, arg2, arg3, arg4, arg5);
    info!(arg6, arg7, arg8, arg9, 0);
    Ret {
        group1: u128::from(arg1) + u128::from(arg2) + u128::from(arg3),
        group2: u128::from(arg4) + u128::from(arg5) + u128::from(arg6),
        group3: u128::from(arg7) + u128::from(arg8) + u128::from(arg9),
    }
}

#[cfg(test)]
mod test {
    extern crate std;
    use super::*;

    #[test]
    fn pull_in_externs() {
        // Rust on Linux excludes the externs unless there is a
        // direct dependency, use this test to force the pull in of the library.
        // This is not necessary on macos and unfortunate on Linux
        // Issue #4972
        use solana_sdk::program_test::*;
        unsafe { sol_log_("X".as_ptr(), 1) };
        sol_log_64_(1, 2, 3, 4, 5);
    }

    #[test]
    fn test_many_args() {
        assert_eq!(45, many_args(1, 2, 3, 4, 5, 6, 7, 8, 9));
    }
}
