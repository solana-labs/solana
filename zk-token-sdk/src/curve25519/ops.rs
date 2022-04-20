pub use target_arch::*;

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use {
        crate::curve25519::pod::*,
        curve25519_dalek::{edwards::EdwardsPoint, ristretto::RistrettoPoint, scalar::Scalar},
        std::convert::TryInto,
    };

    pub fn add_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        let left_point: EdwardsPoint = (*left_point).try_into().ok()?;
        let right_point: EdwardsPoint = (*right_point).try_into().ok()?;

        Some((left_point + right_point).into())
    }

    pub fn subtract_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        let left_point: EdwardsPoint = (*left_point).try_into().ok()?;
        let right_point: EdwardsPoint = (*right_point).try_into().ok()?;

        Some((left_point - right_point).into())
    }

    pub fn add_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let left_point: RistrettoPoint = (*left_point).try_into().ok()?;
        let right_point: RistrettoPoint = (*right_point).try_into().ok()?;

        Some((left_point + right_point).into())
    }

    pub fn subtract_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let left_point: RistrettoPoint = (*left_point).try_into().ok()?;
        let right_point: RistrettoPoint = (*right_point).try_into().ok()?;

        Some((left_point - right_point).into())
    }

    pub fn add_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar + right_scalar).into())
    }

    pub fn subtract_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar - right_scalar).into())
    }

    pub fn multiply_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar * right_scalar).into())
    }

    pub fn divide_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        let left_scalar: Scalar = (*left_scalar).into();
        let right_scalar: Scalar = (*right_scalar).into();

        Some((left_scalar * right_scalar.invert()).into())
    }

    pub fn multiply_edwards(point: &PodEdwardsPoint, scalar: &PodScalar) -> Option<PodEdwardsPoint> {
        let point: EdwardsPoint = (*point).try_into().ok()?;
        let scalar: Scalar = (*scalar).into();

        Some((point * scalar).into())
    }

    pub fn multiply_ristretto(
        point: &PodRistrettoPoint,
        scalar: &PodScalar,
    ) -> Option<PodRistrettoPoint> {
        let point: RistrettoPoint = (*point).try_into().ok()?;
        let scalar: Scalar = (*scalar).into();

        Some((point * scalar).into())
    }
}

#[cfg(target_arch = "bpf")]
mod target_arch {
    use super::{
        OP_EDWARDS_MUL, OP_RISTRETTO_ADD, OP_RISTRETTO_MUL, OP_RISTRETTO_SUB, OP_SCALAR_ADD,
        OP_SCALAR_SUB,
    };
    use {super::*, crate::curve25519::pod::*};

    fn op(op: u64, left: &[u8; 32], right: &[u8; 32]) -> Option<[u8; 32]> {
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

    pub fn add_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        op(OP_EDWARDS_ADD, &left_point.0, &right_point.0).map(|bytes| PodEdwardsPoint(bytes))
    }

    pub fn subtract_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        op(OP_EDWARDS_SUB, &left_point.0, &right_point.0).map(|bytes| PodEdwardsPoint(bytes))
    }

    pub fn add_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        op(OP_RISTRETTO_ADD, &left_point.0, &right_point.0).map(|bytes| PodRistrettoPoint(bytes))
    }

    pub fn subtract_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        op(OP_RISTRETTO_SUB, &left_point.0, &right_point.0).map(|bytes| PodRistrettoPoint(bytes))
    }

    pub fn add_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        op(OP_SCALAR_ADD, &left_scalar.0, &right_scalar.0).map(|bytes| PodScalar(bytes))
    }

    pub fn subtract_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        op(OP_SCALAR_SUB, &left_scalar.0, &right_scalar.0).map(|bytes| PodScalar(bytes))
    }

    pub fn multiply_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        op(OP_SCALAR_MUL, &left_scalar.0, &right_scalar.0).map(|bytes| PodScalar(bytes))
    }

    pub fn divide_scalar(left_scalar: &PodScalar, right_scalar: &PodScalar) -> Option<PodScalar> {
        op(OP_SCALAR_DIV, &left_scalar.0, &right_scalar.0).map(|bytes| PodScalar(bytes))
    }

    pub fn multiply_edwards(point: &PodEdwardsPoint, scalar: &PodScalar) -> Option<PodEdwardsPoint> {
        op(OP_EDWARDS_MUL, &point.0, &scalar.0).map(|bytes| PodEdwardsPoint(bytes))
    }

    pub fn multiply_ristretto(
        point: &PodRistrettoPoint,
        scalar: &PodScalar,
    ) -> Option<PodRistrettoPoint> {
        op(OP_RISTRETTO_MUL, &point.0, &scalar.0).map(|bytes| PodRistrettoPoint(bytes))
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
