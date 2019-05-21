//! @brief Example Rust-based BPF program that prints out the parameters passed to it

#![no_std]
#![feature(allocator_api)]
#![feature(alloc_error_handler)]

#[macro_use]
extern crate alloc;
extern crate solana_sdk_bpf_utils;

use core::alloc::{GlobalAlloc, Layout};
use solana_sdk_bpf_utils::*;

#[global_allocator]
static A: MyAllocator = MyAllocator;

struct MyAllocator;
unsafe impl GlobalAlloc for MyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        sol_alloc_(layout.size() as u64)
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // TODO
    }
}
extern "C" {
    fn sol_alloc_(size: u64) -> *mut u8;
}

#[alloc_error_handler]
fn my_alloc_error_handler(_: core::alloc::Layout) -> ! {
    sol_log("aloc_error_handler");
    panic!();
}

#[no_mangle]
pub extern "C" fn entrypoint() -> bool {
    const ITERS: usize = 500;
    let ones = vec![1_u64; ITERS];
    let mut sum: u64 = 0;

    for v in ones.iter() {
        sum += *v;
    }
    sol_log_64(0xff, 0, 0, 0, sum);
    assert_eq!(sum, ITERS as u64);

    sol_log("Success");
    true
}
