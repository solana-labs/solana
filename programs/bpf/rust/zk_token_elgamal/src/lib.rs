//! @brief zk_token_elgamal syscall tests

extern crate solana_program;
use {
    solana_program::{custom_heap_default, custom_panic_default, msg},
    solana_zk_token_sdk::zk_token_elgamal::{
        ops,
        pod::{ElGamalCiphertext, Zeroable},
    },
};

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    let zero = ElGamalCiphertext::zeroed();

    msg!("add_to");
    let one = ops::add_to(&zero, 1).expect("add_to");

    msg!("subtract_from");
    assert_eq!(zero, ops::subtract_from(&one, 1).expect("subtract_from"));

    msg!("add");
    assert_eq!(one, ops::add(&zero, &one).expect("add"));

    msg!("subtract");
    assert_eq!(zero, ops::subtract(&one, &one).expect("subtract"));

    msg!("add_with_lo_hi");
    assert_eq!(
        one,
        ops::add_with_lo_hi(
            &one,
            &ElGamalCiphertext::zeroed(),
            &ElGamalCiphertext::zeroed()
        )
        .expect("add_with_lo_hi")
    );

    msg!("subtract_with_lo_hi");
    assert_eq!(
        one,
        ops::subtract_with_lo_hi(
            &one,
            &ElGamalCiphertext::zeroed(),
            &ElGamalCiphertext::zeroed()
        )
        .expect("subtract_with_lo_hi")
    );

    0
}

custom_heap_default!();
custom_panic_default!();
