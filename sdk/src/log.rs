#![cfg(feature = "program")]

pub use solana_program::log::*;

#[macro_export]
#[deprecated(
    since = "1.4.3",
    note = "Please use `solana_program::log::info` instead"
)]
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
