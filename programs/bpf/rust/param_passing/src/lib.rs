//! @brief Example Rust-based BPF program tests loop iteration

extern crate solana_sdk;
use solana_bpf_rust_param_passing_dep::{Data, TestDep};
use solana_sdk::entrypoint::SUCCESS;
use solana_sdk::info;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
    let array = [0xA, 0xB, 0xC, 0xD, 0xE, 0xF];
    let data = Data {
        twentyone: 21u64,
        twentytwo: 22u64,
        twentythree: 23u64,
        twentyfour: 24u64,
        twentyfive: 25u32,
        array: &array,
    };

    let test_dep = TestDep::new(&data, 1, 2, 3, 4, 5);
    info!(0, 0, 0, 0, test_dep.thirty);
    assert!(test_dep.thirty == 30);

    SUCCESS
}
