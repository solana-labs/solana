use bytemuck::{Pod, Zeroable};
pub use target_arch::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodRistrettoPoint(pub [u8; 32]);

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use {
        super::*,
        crate::curve25519::{
            curve_syscall_traits::{GroupOperations, MultiScalarMultiplication, PointValidation},
            errors::Curve25519Error,
            scalar::PodScalar,
        },
        curve25519_dalek::{
            ristretto::{CompressedRistretto, RistrettoPoint},
            scalar::Scalar,
            traits::VartimeMultiscalarMul,
        },
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

    pub fn multiscalar_multiply_ristretto(
        scalars: &[PodScalar],
        points: &[PodRistrettoPoint],
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

        fn multiscalar_multiply(scalars: &[PodScalar], points: &[Self]) -> Option<Self> {
            RistrettoPoint::optional_multiscalar_mul(
                scalars.iter().map(Scalar::from),
                points
                    .iter()
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
        super::*,
        crate::curve25519::curve_syscall_traits::{sol_curve_validate_point, CurveId},
    };

    pub fn validate_ristretto(point: &PodRistrettoPoint) -> Option<bool> {
        let mut validate_result = 0u8;
        let result = unsafe {
            sol_curve_validate_point(
                CurveId::Curve25519Ristretto as u64,
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
        crate::curve25519::scalar::PodScalar,
        curve25519_dalek::{
            constants::RISTRETTO_BASEPOINT_POINT as G, ristretto::RistrettoPoint, scalar::Scalar,
            traits::Identity,
        },
        rand::rngs::OsRng,
    };

    #[test]
    fn test_validate_ristretto() {
        let pod = PodRistrettoPoint(G.compress().to_bytes());
        assert!(validate_ristretto(&pod).unwrap());

        let invalid_bytes = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];

        assert!(!validate_ristretto(&PodRistrettoPoint(invalid_bytes)).unwrap());
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

        let ax = multiply_ristretto(&scalar_x, &point_a).unwrap();
        let bx = multiply_ristretto(&scalar_x, &point_b).unwrap();

        assert_eq!(
            add_ristretto(&ax, &bx),
            multiply_ristretto(&scalar_x, &add_ristretto(&point_a, &point_b).unwrap()),
        );
    }

    #[test]
    fn test_multiscalar_multiplication_ristretto() {
        let scalar = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let point = PodRistrettoPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());

        let basic_product = multiply_ristretto(&scalar, &point).unwrap();
        let msm_product = multiscalar_multiply_ristretto(vec![&scalar], vec![&point]).unwrap();

        assert_eq!(basic_product, msm_product);

        let scalar_a = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let scalar_b = PodScalar(Scalar::random(&mut OsRng).to_bytes());
        let point_x = PodRistrettoPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());
        let point_y = PodRistrettoPoint((Scalar::random(&mut OsRng) * G).compress().to_bytes());

        let ax = multiply_ristretto(&scalar_a, &point_x).unwrap();
        let by = multiply_ristretto(&scalar_b, &point_y).unwrap();
        let basic_product = add_ristretto(&ax, &by).unwrap();
        let msm_product =
            multiscalar_multiply_ristretto(vec![&scalar_a, &scalar_b], vec![&point_x, &point_y])
                .unwrap();

        assert_eq!(basic_product, msm_product);
    }
}
