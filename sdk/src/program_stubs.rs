//! @brief Syscall stubs when building for non-BPF targets

#![cfg(feature = "program")]

#[cfg(not(target_arch = "bpf"))]
fn print_line_to_stdout(_message: &str) {
    #[cfg(not(feature = "program"))]
    {
        use std::io::{self, Write};
        io::stdout()
            .write_all(format!("{}\n", _message).as_bytes())
            .unwrap();
        io::stdout().flush().unwrap();
    }
}

#[cfg(not(target_arch = "bpf"))]
#[no_mangle]
/// # Safety
pub unsafe fn sol_log_(message: *const u8, length: u64) {
    let slice = std::slice::from_raw_parts(message, length as usize);
    let string = std::str::from_utf8(&slice).unwrap();
    print_line_to_stdout(string);
}

#[cfg(not(target_arch = "bpf"))]
#[no_mangle]
pub fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    print_line_to_stdout(&format!("{} {} {} {} {}", arg1, arg2, arg3, arg4, arg5));
}

#[cfg(not(target_arch = "bpf"))]
#[no_mangle]
pub fn sol_invoke_signed_rust() {
    print_line_to_stdout("sol_invoke_signed_rust()");
}

#[macro_export]
macro_rules! program_stubs {
    () => {
        #[cfg(not(target_arch = "bpf"))]
        #[test]
        fn pull_in_externs() {
            use solana_sdk::program_stubs::{sol_invoke_signed_rust, sol_log_, sol_log_64_};
            unsafe { sol_log_("sol_log_".as_ptr(), 8) };
            sol_log_64_(1, 2, 3, 4, 5);
            sol_invoke_signed_rust();
        }
    };
}
