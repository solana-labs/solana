//! @brief Example Rust-based BPF program that performs a ristretto multiply

pub mod ristretto;

extern crate solana_sdk;
use crate::ristretto::ristretto_mul;
use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_POINT, scalar::Scalar};
use solana_sdk::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, info, pubkey::Pubkey,
};

entrypoint!(process_instruction);
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    info!("Ristretto multiply");

    let point = RISTRETTO_BASEPOINT_POINT;
    let scalar = Scalar::zero();
    let result = ristretto_mul(&point, &scalar)?;
    assert_ne!(point, result);

    let point = RISTRETTO_BASEPOINT_POINT;
    let scalar = Scalar::one();
    let result = ristretto_mul(&point, &scalar)?;
    assert_eq!(point, result);

    Ok(())
}
