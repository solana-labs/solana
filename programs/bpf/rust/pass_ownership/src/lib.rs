//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

#[cfg(not(test))]
extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::info;

struct Foo {
    one: u64,
    two: u64,
    three: u64,
    four: u64,
    five: u64,
}

#[inline(never)]
fn print_foo(foo: Foo, one: u64, two: u64, three: u64, four: u64) {
    info!(foo.one, foo.two, foo.three, foo.four, foo.five);
    info!(one, two, three, four, 0);
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    let foo = Foo {
        one: 1,
        two: 2,
        three: 3,
        four: 4,
        five: 5,
    };

    print_foo(foo, 1, 2, 3, 4);

    info!("Success");
    true
}
