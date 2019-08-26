//! @brief Example Rust-based BPF program tests loop iteration

#![no_std]
#![allow(unused_attributes)]

#[cfg(not(test))]
extern crate solana_sdk_bpf_no_std;
extern crate solana_sdk_bpf_utils;

use solana_sdk_bpf_utils::info;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> bool {
    let x: u128 = 1;
    let y = x.rotate_right(1);
    assert_eq!(y, 170_141_183_460_469_231_731_687_303_715_884_105_728);

    assert_eq!(
        u128::max_value(),
        340_282_366_920_938_463_463_374_607_431_768_211_455
    );

    let mut z = u128::max_value();
    z -= 1;
    assert_eq!(z, 340_282_366_920_938_463_463_374_607_431_768_211_454);

    // ISSUE: https://github.com/solana-labs/solana/issues/5600
    // assert_eq!(u128::from(1u32.to_be()), 1);

    // ISSUE: https://github.com/solana-labs/solana/issues/5619
    // solana_bpf_rust_128bit_dep::two_thirds(10);

    let x = u64::max_value();
    assert_eq!(u128::from(x) + u128::from(x), 36_893_488_147_419_103_230);

    let x = solana_bpf_rust_128bit_dep::work(
        u128::from(u64::max_value()),
        u128::from(u64::max_value()),
    );
    assert_eq!(x.wrapping_shr(64) as u64, 1);
    assert_eq!(
        x.wrapping_shl(64).wrapping_shr(64) as u64,
        0xffff_ffff_ffff_fffe
    );
    assert_eq!(x, 0x0001_ffff_ffff_ffff_fffe);

    info!("Success");
    true
}
