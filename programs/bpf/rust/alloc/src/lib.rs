//! @brief Example Rust-based BPF program that test dynamic memory allocation

#[macro_use]
extern crate alloc;
extern crate solana_sdk;
use solana_sdk::entrypoint::SUCCESS;
use solana_sdk::info;
use std::alloc::Layout;
use std::mem;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    unsafe {
        // Confirm large allocation fails

        let layout = Layout::from_size_align(std::usize::MAX, mem::align_of::<u8>()).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        if !ptr.is_null() {
            info!("Error: Alloc of very larger buffer should fail");
            panic!();
        }
    }

    unsafe {
        // Test modest allocation and deallocation

        let layout = Layout::from_size_align(100, mem::align_of::<u8>()).unwrap();
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            info!("Error: Alloc of 100 bytes failed");
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
            info!("Error: Alloc failed");
            alloc::alloc::handle_alloc_error(layout);
        }
        for i in 0..ITERS {
            *ptr.add(i) = i as u8;
        }
        for i in 0..ITERS {
            assert_eq!(*ptr.add(i as usize), i as u8);
        }
        info!(0x3, 0, 0, 0, u64::from(*ptr.add(42)));
        assert_eq!(*ptr.add(42), 42);
        alloc::alloc::dealloc(ptr, layout);
    }

    // TODO not supported bump allocator
    // unsafe {
    //     // Test alloc all bytes and one more (assumes heap size of 2048)

    //     let layout = Layout::from_size_align(2048, mem::align_of::<u8>()).unwrap();
    //     let ptr = alloc::alloc::alloc(layout);
    //     if ptr.is_null() {
    //         info!("Error: Alloc of 2048 bytes failed");
    //         alloc::alloc::handle_alloc_error(layout);
    //     }
    //     let layout = Layout::from_size_align(1, mem::align_of::<u8>()).unwrap();
    //     let ptr_fail = alloc::alloc::alloc(layout);
    //     if !ptr_fail.is_null() {
    //         info!("Error: Able to alloc 1 more then max");
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
        info!(0x0, 0, 0, 0, sum as u64);
        assert_eq!(sum, ITERS);
    }

    {
        // test Vec::new()

        const ITERS: usize = 100;
        let mut v = Vec::new();

        for i in 0..ITERS {
            v.push(i);
        }
        info!(0x4, 0, 0, 0, v.len() as u64);
        assert_eq!(v.len(), ITERS);
    }

    SUCCESS
}

#[cfg(test)]
mod test {
    use super::*;
    // Pulls in the stubs requried for `info!()`
    solana_sdk_bpf_test::stubs!();

    #[test]
    fn test_entrypoint() {
        assert_eq!(SUCCESS, entrypoint(std::ptr::null_mut()));
    }
}
