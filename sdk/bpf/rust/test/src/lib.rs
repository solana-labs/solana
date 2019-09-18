  //! @brief Solana Rust-based BPF program utility functions and types

#[allow(dead_code)]
#[no_mangle]
pub unsafe extern "C" fn sol_log_(message: *const u8, length: u64) {
    let slice = std::slice::from_raw_parts(message, length as usize);
    let string = std::str::from_utf8(&slice).unwrap();
    std::println!("{}", string);
}

#[allow(dead_code)]
#[no_mangle]
pub extern "C" fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    std::println!("{} {} {} {} {}", arg1, arg2, arg3, arg4, arg5);
}
