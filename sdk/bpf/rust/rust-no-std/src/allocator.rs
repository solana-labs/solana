//! @brief Solana Rust-based BPF program memory allocator shim

use core::alloc::{GlobalAlloc, Layout};
use solana_sdk_bpf_utils::log::*;

pub struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        sol_alloc_free_(layout.size() as u64, 0)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        sol_alloc_free_(layout.size() as u64, ptr as u64);
    }
}
extern "C" {
    fn sol_alloc_free_(size: u64, ptr: u64) -> *mut u8;
}

#[alloc_error_handler]
fn my_alloc_error_handler(_: core::alloc::Layout) -> ! {
    sol_log("alloc_error_handler");
    panic!();
}
