use bytemuck::{Pod, Zeroable};
pub use target_arch::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodRistrettoPoint(pub [u8; 32]);

#[cfg(not(target_os = "solana"))]
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

    pub fn validate_ristretto(point: &PodRistrettoPoint) -> bool {
        point.validate_point()
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

        #[cfg(not(target_os = "solana"))]
        fn multiply(scalar: &PodScalar, point: &Self) -> Option<Self> {
            let scalar: Scalar = scalar.try_into().ok()?;
            let point: RistrettoPoint = point.try_into().ok()?;

            let result = &scalar * &point;
            Some((&result).into())
        }
    }

    impl MultiScalarMultiplication for PodRistrettoPoint {
        type Scalar = PodScalar;
        type Point = Self;

        fn multiscalar_multiply(scalars: &[PodScalar], points: &[Self]) -> Option<Self> {
            let scalars = scalars
                .iter()
                .map(|scalar| Scalar::try_from(scalar).ok())
                .collect::<Option<Vec<_>>>()?;

            RistrettoPoint::optional_multiscalar_mul(
                scalars,
                points
                    .iter()
                    .map(|point| RistrettoPoint::try_from(point).ok()),
            )
            .map(|result| PodRistrettoPoint::from(&result))
        }
    }
}

#[cfg(target_os = "solana")]
#[allow(unused_variables)]
mod target_arch {
    use {
        super::*,
        crate::curve25519::{
            curve_syscall_traits::{ADD, CURVE25519_RISTRETTO, MUL, SUB},
            scalar::PodScalar,
        },
    };

    pub fn validate_ristretto(point: &PodRistrettoPoint) -> bool {
        let mut validate_result = 0u8;
        let result = unsafe {
            solana_program::syscalls::sol_curve_validate_point(
                CURVE25519_RISTRETTO,
                &point.0 as *const u8,
                &mut validate_result,
            )
        };

        result == 0
    }

    pub fn add_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let mut result_point = PodRistrettoPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_group_op(
                CURVE25519_RISTRETTO,
                ADD,
                &left_point.0 as *const u8,
                &right_point.0 as *const u8,
                &mut result_point.0 as *mut u8,
            )
        };

        if result == 0 {
            Some(result_point)
        } else {
            None
        }
    }

    pub fn subtract_ristretto(
        left_point: &PodRistrettoPoint,
        right_point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let mut result_point = PodRistrettoPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_group_op(
                CURVE25519_RISTRETTO,
                SUB,
                &left_point.0 as *const u8,
                &right_point.0 as *const u8,
                &mut result_point.0 as *mut u8,
            )
        };

        if result == 0 {
            Some(result_point)
        } else {
            None
        }
    }

    pub fn multiply_ristretto(
        scalar: &PodScalar,
        point: &PodRistrettoPoint,
    ) -> Option<PodRistrettoPoint> {
        let mut result_point = PodRistrettoPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_group_op(
                CURVE25519_RISTRETTO,
                MUL,
                &scalar.0 as *const u8,
                &point.0 as *const u8,
                &mut result_point.0 as *mut u8,
            )
        };

        if result == 0 {
            Some(result_point)
        } else {
            None
        }
    }

    pub fn multiscalar_multiply_ristretto(
        scalars: &[PodScalar],
        points: &[PodRistrettoPoint],
    ) -> Option<PodRistrettoPoint> {
        let mut result_point = PodRistrettoPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_multiscalar_mul(
                CURVE25519_RISTRETTO,
                scalars.as_ptr() as *const u8,
                points.as_ptr() as *const u8,
                points.len() as u64,
                &mut result_point.0 as *mut u8,
            )
        };

        if result == 0 {
            Some(result_point)
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
            constants::RISTRETTO_BASEPOINT_POINT as G, ristretto::RistrettoPoint, traits::Identity,
        },
    };

    #[test]
    fn test_validate_ristretto() {
        let pod = PodRistrettoPoint(G.compress().to_bytes());
        assert!(validate_ristretto(&pod));

        let invalid_bytes = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];

        assert!(!validate_ristretto(&PodRistrettoPoint(invalid_bytes)));
    }

    #[test]
    fn test_add_subtract_ristretto() {
        // identity
        let identity = PodRistrettoPoint(RistrettoPoint::identity().compress().to_bytes());
        let point = PodRistrettoPoint([
            210, 174, 124, 127, 67, 77, 11, 114, 71, 63, 168, 136, 113, 20, 141, 228, 195, 254,
            232, 229, 220, 249, 213, 232, 61, 238, 152, 249, 83, 225, 206, 16,
        ]);

        assert_eq!(add_ristretto(&point, &identity).unwrap(), point);
        assert_eq!(subtract_ristretto(&point, &identity).unwrap(), point);

        // associativity
        let point_a = PodRistrettoPoint([
            208, 165, 125, 204, 2, 100, 218, 17, 170, 194, 23, 9, 102, 156, 134, 136, 217, 190, 98,
            34, 183, 194, 228, 153, 92, 11, 108, 103, 28, 57, 88, 15,
        ]);
        let point_b = PodRistrettoPoint([
            208, 241, 72, 163, 73, 53, 32, 174, 54, 194, 71, 8, 70, 181, 244, 199, 93, 147, 99,
            231, 162, 127, 25, 40, 39, 19, 140, 132, 112, 212, 145, 108,
        ]);
        let point_c = PodRistrettoPoint([
            250, 61, 200, 25, 195, 15, 144, 179, 24, 17, 252, 167, 247, 44, 47, 41, 104, 237, 49,
            137, 231, 173, 86, 106, 121, 249, 245, 247, 70, 188, 31, 49,
        ]);

        assert_eq!(
            add_ristretto(&add_ristretto(&point_a, &point_b).unwrap(), &point_c),
            add_ristretto(&point_a, &add_ristretto(&point_b, &point_c).unwrap()),
        );

        assert_eq!(
            subtract_ristretto(&subtract_ristretto(&point_a, &point_b).unwrap(), &point_c),
            subtract_ristretto(&point_a, &add_ristretto(&point_b, &point_c).unwrap()),
        );

        // commutativity
        assert_eq!(
            add_ristretto(&point_a, &point_b).unwrap(),
            add_ristretto(&point_b, &point_a).unwrap(),
        );

        // subtraction
        let point = PodRistrettoPoint(G.compress().to_bytes());
        let point_negated = PodRistrettoPoint((-G).compress().to_bytes());

        assert_eq!(
            point_negated,
            subtract_ristretto(&identity, &point).unwrap(),
        )
    }

    #[test]
    fn test_multiply_ristretto() {
        let scalar_x = PodScalar([
            254, 198, 23, 138, 67, 243, 184, 110, 236, 115, 236, 205, 205, 215, 79, 114, 45, 250,
            78, 137, 3, 107, 136, 237, 49, 126, 117, 223, 37, 191, 88, 6,
        ]);
        let point_a = PodRistrettoPoint([
            68, 80, 232, 181, 241, 77, 60, 81, 154, 51, 173, 35, 98, 234, 149, 37, 1, 39, 191, 201,
            193, 48, 88, 189, 97, 126, 63, 35, 144, 145, 203, 31,
        ]);
        let point_b = PodRistrettoPoint([
            200, 236, 1, 12, 244, 130, 226, 214, 28, 125, 43, 163, 222, 234, 81, 213, 201, 156, 31,
            4, 167, 132, 240, 76, 164, 18, 45, 20, 48, 85, 206, 121,
        ]);

        let ax = multiply_ristretto(&scalar_x, &point_a).unwrap();
        let bx = multiply_ristretto(&scalar_x, &point_b).unwrap();

        assert_eq!(
            add_ristretto(&ax, &bx),
            multiply_ristretto(&scalar_x, &add_ristretto(&point_a, &point_b).unwrap()),
        );
    }

    #[test]
    fn test_multiscalar_multiplication_ristretto() {
        let scalar = PodScalar([
            123, 108, 109, 66, 154, 185, 88, 122, 178, 43, 17, 154, 201, 223, 31, 238, 59, 215, 71,
            154, 215, 143, 177, 158, 9, 136, 32, 223, 139, 13, 133, 5,
        ]);
        let point = PodRistrettoPoint([
            158, 2, 130, 90, 148, 36, 172, 155, 86, 196, 74, 139, 30, 98, 44, 225, 155, 207, 135,
            111, 238, 167, 235, 67, 234, 125, 0, 227, 146, 31, 24, 113,
        ]);

        let basic_product = multiply_ristretto(&scalar, &point).unwrap();
        let msm_product = multiscalar_multiply_ristretto(&[scalar], &[point]).unwrap();

        assert_eq!(basic_product, msm_product);

        let scalar_a = PodScalar([
            8, 161, 219, 155, 192, 137, 153, 26, 27, 40, 30, 17, 124, 194, 26, 41, 32, 7, 161, 45,
            212, 198, 212, 81, 133, 185, 164, 85, 95, 232, 106, 10,
        ]);
        let scalar_b = PodScalar([
            135, 207, 106, 208, 107, 127, 46, 82, 66, 22, 136, 125, 105, 62, 69, 34, 213, 210, 17,
            196, 120, 114, 238, 237, 149, 170, 5, 243, 54, 77, 172, 12,
        ]);
        let point_x = PodRistrettoPoint([
            130, 35, 97, 25, 18, 199, 33, 239, 85, 143, 119, 111, 49, 51, 224, 40, 167, 185, 240,
            179, 25, 194, 213, 41, 14, 155, 104, 18, 181, 197, 15, 112,
        ]);
        let point_y = PodRistrettoPoint([
            152, 156, 155, 197, 152, 232, 92, 206, 219, 159, 193, 134, 121, 128, 139, 36, 56, 191,
            51, 143, 72, 204, 87, 76, 110, 124, 101, 96, 238, 158, 42, 108,
        ]);

        let ax = multiply_ristretto(&scalar_a, &point_x).unwrap();
        let by = multiply_ristretto(&scalar_b, &point_y).unwrap();
        let basic_product = add_ristretto(&ax, &by).unwrap();
        let msm_product =
            multiscalar_multiply_ristretto(&[scalar_a, scalar_b], &[point_x, point_y]).unwrap();

        assert_eq!(basic_product, msm_product);
    }
}
