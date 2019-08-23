//! @brief Solana Rust-based BPF program panic handling

use core::panic::PanicInfo;
use core::ptr;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Message is ignored for now to avoid incurring formatting overhead
    match info.location() {
        Some(location) => {
            let mut file: [u8; 128] = [0; 128];
            for (i, c) in location.file().as_bytes().iter().enumerate() {
                if i > 127 {
                    break;
                }
                file[i] = *c;
            }
            unsafe {
                sol_panic_(
                    file.as_ptr(),
                    file.len() as u64,
                    u64::from(location.line()),
                    u64::from(location.column()),
                );
            }
        }
        None => unsafe { sol_panic_(ptr::null(), 0, 0, 0) },
    }
}
extern "C" {
    pub fn sol_panic_(file: *const u8, len: u64, line: u64, column: u64) -> !;
}
