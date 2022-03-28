pub use target_arch::*;

use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodRistrettoPoint(pub [u8; 32]);

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use super::*;
    use crate::curve25519::{
        curve_syscall_traits::{
            GroupOperations, MultiScalarMultiplication, PointValidation,
        },
        errors::Curve25519Error,
        scalar::PodScalar,
    };
    use curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
        traits::VartimeMultiscalarMul,
    };

    pub fn validate_ristretto(point: &PodRistrettoPoint) -> Option<bool> {
        Some(point.validate_point())
    }

    pub fn add_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        PodRistrettoPoint::add(left_point, right_point)
    }

    pub fn subtract_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        PodRistrettoPoint::subtract(left_point, right_point)
    }

    pub fn multiply_ristretto(
        scalar: &PodScalar,
        point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        PodRistrettoPoint::multiply(scalar, point)
    }

    pub fn multiscalar_multiply(
        scalars: Vec<&PodScalar>,
        points: Vec<&PodRistrettoPoint>,
    ) -> Option<PodRistrettoPoint> {
        PodRistrettoPoint::multiscalar_multiply(scalars, points)
    }

    impl From<&RistrettoPoint> for PodRistrettoPoint {
        fn from(point: &RistrettoPoint) -> Self {
            Self(point.compress().to_bytes())
        }
    }

    impl TryFrom<&PodRistrettoPoint> for RistrettoPoint {
        type Error = Curve25519Error;

        fn try_from(pod: &PodRistrettoPoint) -> Result<Self, Self::Error> {
            CompressedRistretto::from_slice(&pod.0)
                .decompress()
                .ok_or(Curve25519Error::PodConversion)
        }
    }

    impl PointValidation for PodRistrettoPoint {
        type Point = Self;

        fn validate_point(&self) -> bool {
            CompressedRistretto::from_slice(&self.0)
                .decompress()
                .is_some()
        }
    }

    impl GroupOperations for PodRistrettoPoint {
        type Scalar = PodScalar;
        type Point = Self;

        fn add(left_point: &Self, right_point: &Self) -> Option<Self> {
            let left_point: RistrettoPoint = left_point.try_into().ok()?;
            let right_point: RistrettoPoint = right_point.try_into().ok()?;

            let result = &left_point + &right_point;
            Some((&result).into())
        }

        fn subtract(left_point: &Self, right_point: &Self) -> Option<Self> {
            let left_point: RistrettoPoint = left_point.try_into().ok()?;
            let right_point: RistrettoPoint = right_point.try_into().ok()?;

            let result = &left_point - &right_point;
            Some((&result).into())
        }

        #[cfg(not(target_arch = "bpf"))]
        fn multiply(scalar: &PodScalar, point: &Self) -> Option<Self> {
            let scalar: Scalar = scalar.into();
            let point: RistrettoPoint = point.try_into().ok()?;

            let result = &scalar * &point;
            Some((&result).into())
        }
    }

    impl MultiScalarMultiplication for PodRistrettoPoint {
        type Scalar = PodScalar;
        type Point = Self;

        fn multiscalar_multiply(scalars: Vec<&PodScalar>, points: Vec<&Self>) -> Option<Self> {
            RistrettoPoint::optional_multiscalar_mul(
                scalars.into_iter().map(|scalar| Scalar::from(scalar)),
                points
                    .into_iter()
                    .map(|point| RistrettoPoint::try_from(point).ok()),
            )
            .map(|result| PodRistrettoPoint::from(&result))
        }
    }

}

#[cfg(target_arch = "bpf")]
#[allow(unused_variables)]
mod target_arch {
    use {
        super::*, crate::curve25519::curve_syscall_traits::{sol_validate_point, CURVE25519_RISTRETTO},
    };

    pub fn validate_ristretto(point: &PodRistrettoPoint) -> Option<bool> {
        let mut validate_result = 0u8;
        let result = unsafe {
            sol_validate_point(
                CURVE25519_RISTRETTO,
                &point.0 as *const u8,
                &mut validate_result,
            )
        };

        if result == 0 {
            Some(validate_result == 0)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        curve25519_dalek::{
            ristretto::RistrettoPoint, scalar::Scalar, traits::Identity,
        },
        rand::rngs::OsRng,
    };

    #[test]
    fn test_validate_ristretto() {

    }

    #[test]
    fn test_add_subtract_ristretto() {
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
    fn test_multiply_ristretto() {
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

    #[test]
    fn test_multiscalar_multiplication_ristretto() {

    }
}
