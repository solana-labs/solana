//! @brief Solana Rust-based BPF program utility functions and types

#[no_mangle]
/// # Safety
pub unsafe fn sol_log_(message: *const u8, length: u64) {
    let slice = std::slice::from_raw_parts(message, length as usize);
    let string = std::str::from_utf8(&slice).unwrap();
    std::println!("{}", string);
}

#[no_mangle]
pub fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    std::println!("{} {} {} {} {}", arg1, arg2, arg3, arg4, arg5);
}

#[macro_export]
macro_rules! stubs {
    () => {
        #[test]
        fn pull_in_externs() {
            use $crate::*;
            unsafe { sol_log_("sol_log_".as_ptr(), 8) };
            sol_log_64_(1, 2, 3, 4, 5);
        }
    };
}
