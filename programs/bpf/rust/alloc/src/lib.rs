//! @brief Example Rust-based BPF program that test dynamic memory allocation
#![no_std]
#![allow(unused_attributes)]

#[macro_use]
extern crate alloc;
extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::log::*;

use alloc::vec::Vec;
use core::alloc::Layout;
use core::mem;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    unsafe {
        // Confirm large allocation fails

        let layout = Layout::from_size_align(core::usize::MAX, mem::align_of::<u8>()).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        if !ptr.is_null() {
            sol_log("Error: Alloc of very larger buffer should fail");
            panic!();
        }
    }

    unsafe {
        // Test modest allocation and deallocation

        let layout = Layout::from_size_align(100, mem::align_of::<u8>()).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            sol_log("Error: Alloc of 100 bytes failed");
            alloc::alloc::handle_alloc_error(layout);
        }
        alloc::alloc::dealloc(ptr, layout);
    }

    unsafe {
        // Test allocated memory read and write

        const ITERS: usize = 100;
        let layout = Layout::from_size_align(100, mem::align_of::<u8>()).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            sol_log("Error: Alloc failed");
            alloc::alloc::handle_alloc_error(layout);
        }
        for i in 0..ITERS {
            *ptr.add(i) = i as u8;
        }
        for i in 0..ITERS {
            assert_eq!(*ptr.add(i as usize), i as u8);
        }
        sol_log_64(0x3, 0, 0, 0, u64::from(*ptr.add(42)));
        assert_eq!(*ptr.add(42), 42);
        alloc::alloc::dealloc(ptr, layout);
    }

    // // TODO not supported for system or bump allocator
    // unsafe {
    //     // Test alloc all bytes and one more (assumes heap size of 2048)

    //     let layout = Layout::from_size_align(2048, mem::align_of::<u8>()).unwrap();
    //     let ptr = alloc::alloc::alloc(layout);
    //     if ptr.is_null() {
    //         sol_log("Error: Alloc of 2048 bytes failed");
    //         alloc::alloc::handle_alloc_error(layout);
    //     }
    //     let layout = Layout::from_size_align(1, mem::align_of::<u8>()).unwrap();
    //     let ptr_fail = alloc::alloc::alloc(layout);
    //     if !ptr_fail.is_null() {
    //         sol_log("Error: Able to alloc 1 more then max");
    //         panic!();
    //     }
    //     alloc::alloc::dealloc(ptr, layout);
    // }

    {
        // Test allocated vector

        const ITERS: usize = 100;
        let ones = vec![1_usize; ITERS];
        let mut sum: usize = 0;

        for v in ones.iter() {
            sum += ones[*v];
        }
        sol_log_64(0x0, 0, 0, 0, sum as u64);
        assert_eq!(sum, ITERS);
    }

    {
        // TODO test Vec::new()

        const ITERS: usize = 100;
        let mut v = Vec::new();

        for i in 0..ITERS {
            sol_log_64(i as u64, 0, 0, 0, 0);
            v.push(i);
        }
        sol_log_64(0x4, 0, 0, 0, v.len() as u64);
        assert_eq!(v.len(), ITERS);
    }

    sol_log("Success");
    true
}
