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

    pub fn multiply_edwards(
        point: &PodEdwardsPoint,
        scalar: &PodScalar,
    ) -> Option<PodEdwardsPoint> {
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

    pub fn multiply_edwards(
        point: &PodEdwardsPoint,
        scalar: &PodScalar,
    ) -> Option<PodEdwardsPoint> {
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
mod tests {
    use {
        super::*,
        crate::curve25519::pod::*,
        curve25519_dalek::{
            constants::ED25519_BASEPOINT_POINT as G, edwards::EdwardsPoint,
            ristretto::RistrettoPoint, scalar::Scalar, traits::Identity,
        },
        rand::rngs::OsRng,
    };

    #[test]
    fn test_edwards_ops() {
        // identity
        let identity = PodEdwardsPoint(EdwardsPoint::identity().compress().to_bytes());
        let random_point = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());

        assert_eq!(add_edwards(&random_point, &identity).unwrap(), random_point);
        assert_eq!(
            subtract_edwards(&random_point, &identity).unwrap(),
            random_point
        );

        // associativity
        let point_a = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());
        let point_b = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());
        let point_c = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());

        assert_eq!(
            add_edwards(&add_edwards(&point_a, &point_b).unwrap(), &point_c),
            add_edwards(&point_a, &add_edwards(&point_b, &point_c).unwrap()),
        );

        assert_eq!(
            subtract_edwards(&subtract_edwards(&point_a, &point_b).unwrap(), &point_c),
            subtract_edwards(&point_a, &add_edwards(&point_b, &point_c).unwrap()),
        );

        // commutativity
        let point_a = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());
        let point_b = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());

        assert_eq!(
            add_edwards(&point_a, &point_b).unwrap(),
            add_edwards(&point_b, &point_a).unwrap(),
        );

        // subtraction
        let identity = PodEdwardsPoint(EdwardsPoint::identity().compress().to_bytes());
        let random_edwards = Scalar::random(&mut OsRng) * G;
        let random_point = PodEdwardsPoint(random_edwards.compress().to_bytes());
        let random_point_negated = PodEdwardsPoint((-random_edwards).compress().to_bytes());

        assert_eq!(
            random_point_negated,
            subtract_edwards(&identity, &random_point).unwrap(),
        )
    }

    #[test]
    fn test_ristretto_ops() {
        // identity
        let identity = PodRistrettoPoint(RistrettoPoint::identity().compress().to_bytes());
        let random_point =
            PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());

        assert_eq!(
            add_ristretto(&random_point, &identity).unwrap(),
            random_point
        );
        assert_eq!(
            subtract_ristretto(&random_point, &identity).unwrap(),
            random_point
        );

        // associativity
        let point_a = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());
        let point_b = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());
        let point_c = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());

        assert_eq!(
            add_ristretto(&add_ristretto(&point_a, &point_b).unwrap(), &point_c),
            add_ristretto(&point_a, &add_ristretto(&point_b, &point_c).unwrap()),
        );

        assert_eq!(
            subtract_ristretto(&subtract_ristretto(&point_a, &point_b).unwrap(), &point_c),
            subtract_ristretto(&point_a, &add_ristretto(&point_b, &point_c).unwrap()),
        );

        // commutativity
        let point_a = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());
        let point_b = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());

        assert_eq!(
            add_ristretto(&point_a, &point_b).unwrap(),
            add_ristretto(&point_b, &point_a).unwrap(),
        );

        // subtraction
        let identity = PodRistrettoPoint(RistrettoPoint::identity().compress().to_bytes());
        let random_ristretto = RistrettoPoint::random(&mut OsRng);
        let random_point = PodRistrettoPoint(random_ristretto.compress().to_bytes());
        let random_point_negated = PodRistrettoPoint((-random_ristretto).compress().to_bytes());

        assert_eq!(
            random_point_negated,
            subtract_ristretto(&identity, &random_point).unwrap(),
        )
    }

    #[test]
    fn test_scalar_ops() {
        // identity
        let zero = PodScalar(Scalar::zero().to_bytes());
        let random_scalar = PodScalar(Scalar::random(&mut OsRng).to_bytes());

        assert_eq!(add_scalar(&random_scalar, &zero).unwrap(), random_scalar);
        assert_eq!(
            subtract_scalar(&random_scalar, &zero).unwrap(),
            random_scalar
        );
        assert_eq!(multiply_scalar(&zero, &random_scalar).unwrap(), zero);
        assert_eq!(divide_scalar(&zero, &random_scalar).unwrap(), zero);

        // associativity
        let scalar_a = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let scalar_b = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let scalar_c = PodScalar(Scalar::random(&mut OsRng).to_bytes());

        assert_eq!(
            add_scalar(&add_scalar(&scalar_a, &scalar_b).unwrap(), &scalar_c),
            add_scalar(&scalar_a, &add_scalar(&scalar_b, &scalar_c).unwrap()),
        );

        assert_eq!(
            subtract_scalar(&subtract_scalar(&scalar_a, &scalar_b).unwrap(), &scalar_c),
            subtract_scalar(&scalar_a, &add_scalar(&scalar_b, &scalar_c).unwrap()),
        );

        assert_eq!(
            multiply_scalar(&multiply_scalar(&scalar_a, &scalar_b).unwrap(), &scalar_c),
            multiply_scalar(&scalar_a, &multiply_scalar(&scalar_b, &scalar_c).unwrap()),
        );

        assert_eq!(
            divide_scalar(&divide_scalar(&scalar_a, &scalar_b).unwrap(), &scalar_c),
            divide_scalar(&scalar_a, &multiply_scalar(&scalar_b, &scalar_c).unwrap()),
        );

        // commutativity
        let scalar_a = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let scalar_b = PodScalar(Scalar::random(&mut OsRng).to_bytes());

        assert_eq!(
            add_scalar(&scalar_a, &scalar_b).unwrap(),
            add_scalar(&scalar_b, &scalar_a).unwrap(),
        );

        assert_eq!(
            multiply_scalar(&scalar_a, &scalar_b).unwrap(),
            multiply_scalar(&scalar_b, &scalar_a).unwrap(),
        );

        // subtraction and division
        let zero = PodScalar(Scalar::zero().to_bytes());
        let one = PodScalar(Scalar::one().to_bytes());
        let random_scalar = Scalar::random(&mut OsRng);
        let random_scalar_pod = PodScalar(random_scalar.to_bytes());
        let random_scalar_negated_pod = PodScalar((-random_scalar).to_bytes());
        let random_scalar_inverted_pod = PodScalar(random_scalar.invert().to_bytes());

        assert_eq!(
            random_scalar_negated_pod,
            subtract_scalar(&zero, &random_scalar_pod).unwrap(),
        );

        assert_eq!(
            random_scalar_inverted_pod,
            divide_scalar(&one, &random_scalar_pod).unwrap(),
        );
    }

    #[test]
    fn test_edwards_mul() {
        let scalar_x = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let point_a = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());
        let point_b = PodEdwardsPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());

        let ax = multiply_edwards(&point_a, &scalar_x).unwrap();
        let bx = multiply_edwards(&point_b, &scalar_x).unwrap();

        assert_eq!(
            add_edwards(&ax, &bx),
            multiply_edwards(&add_edwards(&point_a, &point_b).unwrap(), &scalar_x),
        );
    }

    #[test]
    fn test_ristretto_mul() {
        let scalar_x = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let point_a = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());
        let point_b = PodRistrettoPoint(RistrettoPoint::random(&mut OsRng).compress().to_bytes());

        let ax = multiply_ristretto(&point_a, &scalar_x).unwrap();
        let bx = multiply_ristretto(&point_b, &scalar_x).unwrap();

        assert_eq!(
            add_ristretto(&ax, &bx),
            multiply_ristretto(&add_ristretto(&point_a, &point_b).unwrap(), &scalar_x),
        );
    }
}
