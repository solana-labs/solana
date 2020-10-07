use curve25519_dalek::{ristretto::RistrettoPoint, scalar::Scalar};
use solana_sdk::{entrypoint::SUCCESS, program_error::ProgramError};

/// Multiply a ristretto point with a scalar
///
/// @param point - Ristretto point
/// @param scalar - Scalar to mulitply against
/// @return - result of the multiplication
#[inline]
pub fn ristretto_mul(
    point: &RistrettoPoint,
    scalar: &Scalar,
) -> Result<RistrettoPoint, ProgramError> {
    let mut result = RistrettoPoint::default();
    let status = unsafe {
        sol_ristretto_mul(
            point as *const _ as *const u8,
            scalar as *const _ as *const u8,
            &mut result as *const _ as *mut u8,
        )
    };
    match status {
        SUCCESS => Ok(result),
        _ => Err(status.into()),
    }
}
extern "C" {
    fn sol_ristretto_mul(
        point_addr: *const u8,
        scalar_addr: *const u8,
        result_addr: *mut u8,
    ) -> u64;
}
