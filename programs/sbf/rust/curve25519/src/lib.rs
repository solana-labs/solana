//! @brief curve25519 syscall tests

extern crate solana_program;
use {
    solana_program::{custom_heap_default, custom_panic_default, msg},
    solana_zk_token_sdk::curve25519::{edwards, ristretto, scalar},
};

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    let scalar_one = scalar::PodScalar([
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ]);

    let edwards_identity = edwards::PodEdwardsPoint([
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ]);

    let edwards_generator = edwards::PodEdwardsPoint([
        88, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
        102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
    ]);

    msg!("validate_edwards");
    assert!(edwards::validate_edwards(&edwards_generator));

    msg!("add_edwards");
    assert_eq!(
        edwards_generator,
        edwards::add_edwards(&edwards_generator, &edwards_identity).expect("add_edwards")
    );

    msg!("multiply_edwards");
    assert_eq!(
        edwards_generator,
        edwards::multiply_edwards(&scalar_one, &edwards_generator).expect("multiply_edwards")
    );

    msg!("multiscalar_multiply_edwards");
    assert_eq!(
        edwards_generator,
        edwards::multiscalar_multiply_edwards(&[scalar_one], &[edwards_generator])
            .expect("multiscalar_multiply_edwards"),
    );

    let ristretto_identity = ristretto::PodRistrettoPoint([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ]);

    let ristretto_generator = ristretto::PodRistrettoPoint([
        226, 242, 174, 10, 106, 188, 78, 113, 168, 132, 169, 97, 197, 0, 81, 95, 88, 227, 11, 106,
        165, 130, 221, 141, 182, 166, 89, 69, 224, 141, 45, 118,
    ]);

    msg!("validate_ristretto");
    assert!(ristretto::validate_ristretto(&ristretto_generator));

    msg!("add_ristretto");
    assert_eq!(
        ristretto_generator,
        ristretto::add_ristretto(&ristretto_generator, &ristretto_identity).expect("add_ristretto")
    );

    msg!("multiply_ristretto");
    assert_eq!(
        ristretto_generator,
        ristretto::multiply_ristretto(&scalar_one, &ristretto_generator)
            .expect("multiply_ristretto")
    );

    msg!("multiscalar_multiply_ristretto");
    assert_eq!(
        ristretto_generator,
        ristretto::multiscalar_multiply_ristretto(&[scalar_one], &[ristretto_generator])
            .expect("multiscalar_multiply_ristretto"),
    );

    0
}

custom_heap_default!();
custom_panic_default!();
