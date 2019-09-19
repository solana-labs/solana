//! @brief Example Rust-based BPF program tests loop iteration

extern crate solana_sdk;
use solana_sdk::entrypoint::SUCCESS;

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u32 {
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

    assert_eq!(u128::from(1u32.to_le()), 1);
    assert_eq!(u128::from(1u32.to_be()), 0x1_000_000);

    assert_eq!(solana_bpf_rust_128bit_dep::uadd(10, 20), 30u128);
    assert_eq!(solana_bpf_rust_128bit_dep::usubtract(30, 20), 10u128);
    assert_eq!(solana_bpf_rust_128bit_dep::umultiply(30, 20), 600u128);
    assert_eq!(solana_bpf_rust_128bit_dep::udivide(20, 10), 2u128);
    assert_eq!(solana_bpf_rust_128bit_dep::umodulo(20, 3), 2u128);

    assert_eq!(solana_bpf_rust_128bit_dep::add(-10, -20), -30i128);
    assert_eq!(solana_bpf_rust_128bit_dep::subtract(-30, -20), -10i128);
    assert_eq!(solana_bpf_rust_128bit_dep::multiply(-30, -20), 600i128);
    assert_eq!(solana_bpf_rust_128bit_dep::divide(-20, -10), 2i128);
    assert_eq!(solana_bpf_rust_128bit_dep::modulo(-20, -3), -2i128);

    let x = u64::max_value();
    assert_eq!(u128::from(x) + u128::from(x), 36_893_488_147_419_103_230);

    let x = solana_bpf_rust_128bit_dep::uadd(
        u128::from(u64::max_value()),
        u128::from(u64::max_value()),
    );
    assert_eq!(x.wrapping_shr(64) as u64, 1);
    assert_eq!(
        x.wrapping_shl(64).wrapping_shr(64) as u64,
        0xffff_ffff_ffff_fffe
    );
    assert_eq!(x, 0x0001_ffff_ffff_ffff_fffe);

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
