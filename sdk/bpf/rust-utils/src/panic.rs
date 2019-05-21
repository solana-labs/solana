//! @brief Solana Rust-based BPF program panic handling

// #[cfg(not(test))]
use crate::log::*;
use core::panic::PanicInfo;

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    sol_log("Panic!");
    // TODO crashes! sol_log(_info.payload().downcast_ref::<&str>().unwrap());
    if let Some(location) = info.location() {
        if !location.file().is_empty() {
            // TODO location.file() returns empty str, if we get here its been fixed
            sol_log(location.file());
            sol_log("location.file() is fixed!!");
            unsafe {
                sol_panic_();
            }
        }
        sol_log_64(0, 0, 0, u64::from(location.line()), u64::from(location.column()));
    } else {
        sol_log("Panic! but could not get location information");
    }
    unsafe {
        sol_panic_();
    }
}
extern "C" {
    pub fn sol_panic_() -> !;
}
