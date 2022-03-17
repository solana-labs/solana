pub use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodEdwardsPoint([u8; 32]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodRistrettoPoint([u8; 32]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodScalar([u8; 32]);

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use {
        super::{PodEdwardsPoint, PodRistrettoPoint, PodScalar},
        crate::curve25519::errors::Curve25519Error,
        curve25519_dalek::{
            edwards::{CompressedEdwardsY, EdwardsPoint},
            ristretto::{CompressedRistretto, RistrettoPoint},
            scalar::Scalar,
        },
        std::convert::TryInto,
    };

    impl From<EdwardsPoint> for PodEdwardsPoint {
        fn from(point: EdwardsPoint) -> Self {
            Self(point.compress().to_bytes())
        }
    }

    impl TryFrom<PodEdwardsPoint> for EdwardsPoint {
        type Error = Curve25519Error;

        fn try_from(pod: PodEdwardsPoint) -> Result<Self, Self::Error> {
            CompressedEdwardsY::from_slice(&pod.0)
                .decompress()
                .ok_or(Curve25519Error::PodConversion)
        }
    }

    fn add_edwards(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let left_point: RistrettoPoint = (*left_point).try_into().ok()?;
        let right_point: RistrettoPoint = (*right_point).try_into().ok()?;

        Some((left_point + right_point).into())
    }

    fn subtract_edwards(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let left_point: RistrettoPoint = (*left_point).try_into().ok()?;
        let right_point: RistrettoPoint = (*right_point).try_into().ok()?;

        Some((left_point - right_point).into())
    }


    impl From<RistrettoPoint> for PodRistrettoPoint {
        fn from(point: RistrettoPoint) -> Self {
            Self(point.compress().to_bytes())
        }
    }

    impl TryFrom<PodRistrettoPoint> for RistrettoPoint {
        type Error = Curve25519Error;

        fn try_from(pod: PodRistrettoPoint) -> Result<Self, Self::Error> {
            CompressedRistretto::from_slice(&pod.0)
                .decompress()
                .ok_or(Curve25519Error::PodConversion)
        }
    }

    fn add_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let left_point: RistrettoPoint = (*left_point).try_into().ok()?;
        let right_point: RistrettoPoint = (*right_point).try_into().ok()?;

        Some((left_point + right_point).into())
    }

    fn subtract_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let left_point: RistrettoPoint = (*left_point).try_into().ok()?;
        let right_point: RistrettoPoint = (*right_point).try_into().ok()?;

        Some((left_point - right_point).into())
    }


    impl From<Scalar> for PodScalar {
        fn from(scalar: Scalar) -> Self {
            Self(scalar.to_bytes())
        }
    }

    impl From<PodScalar> for Scalar {
        fn from(pod: PodScalar) -> Self {
            Scalar::from_bits(pod.0)
        }
    }

    fn add_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar + right_scalar).into())
    }

    fn subtract_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar - right_scalar).into())
    }

    fn multiply_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar * right_scalar).into())
    }

    fn divide_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar * right_scalar.invert()).into())
    }


    fn multiply_edwards(point: &PodEdwardsPoint, scalar: &PodScalar) -> Option<PodEdwardsPoint> {
        let point: EdwardsPoint = (*point).try_into().ok()?;
        let scalar: Scalar = (*scalar).into();

        Some((point * scalar).into())
    }

    fn multiply_ristretto(point: &PodRistrettoPoint, scalar: &PodScalar) -> Option<PodRistrettoPoint> {
        let point: RistrettoPoint = (*point).try_into().ok()?;
        let scalar: Scalar = (*scalar).into();

        Some((point * scalar).into())
    }
}

#[cfg(target_arch = "bpf")]
mod target_arch {
    use super::*;

    fn op(
        op: u64,
        left: &[u8; 32],
        right: &[u8; 32],
    ) -> Option<[u8; 32]> {
        let mut op_result = [0u8; 32];
        let result = unsafe {
            curve25519_basic_op(
                op,
                left as *const u8,
                right as *const u8,
                &mut op_result as *mut u8,
            )
        };

        if result == 0 {
            Some(op_result)
        } else {
            None
        }
    }
}

pub const OP_EDWARDS_ADD: u64 = 0;
pub const OP_EDWARDS_SUB: u64 = 1;

pub const OP_RISTRETTO_ADD: u64 = 2;
pub const OP_RISTRETTO_SUB: u64 = 3;

pub const OP_SCALAR_ADD: u64 = 4;
pub const OP_SCALAR_SUB: u64 = 5;
pub const OP_SCALAR_MUL: u64 = 6;
pub const OP_SCALAR_DIV: u64 = 7;

pub const OP_EDWARDS_MUL: u64 = 8;
pub const OP_RISTRETTO_MUL: u64 = 9;

extern "C" {
    pub fn curve25519_basic_op(op: u64, left: *const u8, right: *const u8, result: *mut u8) -> u64;
}

#[cfg(test)]
mod tests {}
