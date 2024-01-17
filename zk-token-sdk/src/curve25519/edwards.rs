use bytemuck::{Pod, Zeroable};
pub use target_arch::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodEdwardsPoint(pub [u8; 32]);

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
            edwards::{CompressedEdwardsY, EdwardsPoint},
            scalar::Scalar,
            traits::VartimeMultiscalarMul,
        },
    };

    pub fn validate_edwards(point: &PodEdwardsPoint) -> bool {
        point.validate_point()
    }

    pub fn add_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        PodEdwardsPoint::add(left_point, right_point)
    }

    pub fn subtract_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        PodEdwardsPoint::subtract(left_point, right_point)
    }

    pub fn multiply_edwards(
        scalar: &PodScalar,
        point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        PodEdwardsPoint::multiply(scalar, point)
    }

    pub fn multiscalar_multiply_edwards(
        scalars: &[PodScalar],
        points: &[PodEdwardsPoint],
    ) -> Option<PodEdwardsPoint> {
        PodEdwardsPoint::multiscalar_multiply(scalars, points)
    }

    impl From<&EdwardsPoint> for PodEdwardsPoint {
        fn from(point: &EdwardsPoint) -> Self {
            Self(point.compress().to_bytes())
        }
    }

    impl TryFrom<&PodEdwardsPoint> for EdwardsPoint {
        type Error = Curve25519Error;

        fn try_from(pod: &PodEdwardsPoint) -> Result<Self, Self::Error> {
            CompressedEdwardsY::from_slice(&pod.0)
                .decompress()
                .ok_or(Curve25519Error::PodConversion)
        }
    }

    impl PointValidation for PodEdwardsPoint {
        type Point = Self;

        fn validate_point(&self) -> bool {
            CompressedEdwardsY::from_slice(&self.0)
                .decompress()
                .is_some()
        }
    }

    impl GroupOperations for PodEdwardsPoint {
        type Scalar = PodScalar;
        type Point = Self;

        fn add(left_point: &Self, right_point: &Self) -> Option<Self> {
            let left_point: EdwardsPoint = left_point.try_into().ok()?;
            let right_point: EdwardsPoint = right_point.try_into().ok()?;

            let result = &left_point + &right_point;
            Some((&result).into())
        }

        fn subtract(left_point: &Self, right_point: &Self) -> Option<Self> {
            let left_point: EdwardsPoint = left_point.try_into().ok()?;
            let right_point: EdwardsPoint = right_point.try_into().ok()?;

            let result = &left_point - &right_point;
            Some((&result).into())
        }

        #[cfg(not(target_os = "solana"))]
        fn multiply(scalar: &PodScalar, point: &Self) -> Option<Self> {
            let scalar: Scalar = scalar.try_into().ok()?;
            let point: EdwardsPoint = point.try_into().ok()?;

            let result = &scalar * &point;
            Some((&result).into())
        }
    }

    impl MultiScalarMultiplication for PodEdwardsPoint {
        type Scalar = PodScalar;
        type Point = Self;

        fn multiscalar_multiply(scalars: &[PodScalar], points: &[Self]) -> Option<Self> {
            let scalars = scalars
                .iter()
                .map(|scalar| Scalar::try_from(scalar).ok())
                .collect::<Option<Vec<_>>>()?;

            EdwardsPoint::optional_multiscalar_mul(
                scalars,
                points
                    .iter()
                    .map(|point| EdwardsPoint::try_from(point).ok()),
            )
            .map(|result| PodEdwardsPoint::from(&result))
        }
    }
}

#[cfg(target_os = "solana")]
mod target_arch {
    use {
        super::*,
        crate::curve25519::{
            curve_syscall_traits::{ADD, CURVE25519_EDWARDS, MUL, SUB},
            scalar::PodScalar,
        },
    };

    pub fn validate_edwards(point: &PodEdwardsPoint) -> bool {
        let mut validate_result = 0u8;
        let result = unsafe {
            solana_program::syscalls::sol_curve_validate_point(
                CURVE25519_EDWARDS,
                &point.0 as *const u8,
                &mut validate_result,
            )
        };
        result == 0
    }

    pub fn add_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        let mut result_point = PodEdwardsPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_group_op(
                CURVE25519_EDWARDS,
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

    pub fn subtract_edwards(
        left_point: &PodEdwardsPoint,
        right_point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        let mut result_point = PodEdwardsPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_group_op(
                CURVE25519_EDWARDS,
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

    pub fn multiply_edwards(
        scalar: &PodScalar,
        point: &PodEdwardsPoint,
    ) -> Option<PodEdwardsPoint> {
        let mut result_point = PodEdwardsPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_group_op(
                CURVE25519_EDWARDS,
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

    pub fn multiscalar_multiply_edwards(
        scalars: &[PodScalar],
        points: &[PodEdwardsPoint],
    ) -> Option<PodEdwardsPoint> {
        let mut result_point = PodEdwardsPoint::zeroed();
        let result = unsafe {
            solana_program::syscalls::sol_curve_multiscalar_mul(
                CURVE25519_EDWARDS,
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
            constants::ED25519_BASEPOINT_POINT as G, edwards::EdwardsPoint, traits::Identity,
        },
    };

    #[test]
    fn test_validate_edwards() {
        let pod = PodEdwardsPoint(G.compress().to_bytes());
        assert!(validate_edwards(&pod));

        let invalid_bytes = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];

        assert!(!validate_edwards(&PodEdwardsPoint(invalid_bytes)));
    }

    #[test]
    fn test_edwards_add_subtract() {
        // identity
        let identity = PodEdwardsPoint(EdwardsPoint::identity().compress().to_bytes());
        let point = PodEdwardsPoint([
            201, 179, 241, 122, 180, 185, 239, 50, 183, 52, 221, 0, 153, 195, 43, 18, 22, 38, 187,
            206, 179, 192, 210, 58, 53, 45, 150, 98, 89, 17, 158, 11,
        ]);

        assert_eq!(add_edwards(&point, &identity).unwrap(), point);
        assert_eq!(subtract_edwards(&point, &identity).unwrap(), point);

        // associativity
        let point_a = PodEdwardsPoint([
            33, 124, 71, 170, 117, 69, 151, 247, 59, 12, 95, 125, 133, 166, 64, 5, 2, 27, 90, 27,
            200, 167, 59, 164, 52, 54, 52, 200, 29, 13, 34, 213,
        ]);
        let point_b = PodEdwardsPoint([
            70, 222, 137, 221, 253, 204, 71, 51, 78, 8, 124, 1, 67, 200, 102, 225, 122, 228, 111,
            183, 129, 14, 131, 210, 212, 95, 109, 246, 55, 10, 159, 91,
        ]);
        let point_c = PodEdwardsPoint([
            72, 60, 66, 143, 59, 197, 111, 36, 181, 137, 25, 97, 157, 201, 247, 215, 123, 83, 220,
            250, 154, 150, 180, 192, 196, 28, 215, 137, 34, 247, 39, 129,
        ]);

        assert_eq!(
            add_edwards(&add_edwards(&point_a, &point_b).unwrap(), &point_c),
            add_edwards(&point_a, &add_edwards(&point_b, &point_c).unwrap()),
        );

        assert_eq!(
            subtract_edwards(&subtract_edwards(&point_a, &point_b).unwrap(), &point_c),
            subtract_edwards(&point_a, &add_edwards(&point_b, &point_c).unwrap()),
        );

        // commutativity
        assert_eq!(
            add_edwards(&point_a, &point_b).unwrap(),
            add_edwards(&point_b, &point_a).unwrap(),
        );

        // subtraction
        let point = PodEdwardsPoint(G.compress().to_bytes());
        let point_negated = PodEdwardsPoint((-G).compress().to_bytes());

        assert_eq!(point_negated, subtract_edwards(&identity, &point).unwrap(),)
    }

    #[test]
    fn test_edwards_mul() {
        let scalar_a = PodScalar([
            72, 191, 131, 55, 85, 86, 54, 60, 116, 10, 39, 130, 180, 3, 90, 227, 47, 228, 252, 99,
            151, 71, 118, 29, 34, 102, 117, 114, 120, 50, 57, 8,
        ]);
        let point_x = PodEdwardsPoint([
            176, 121, 6, 191, 108, 161, 206, 141, 73, 14, 235, 97, 49, 68, 48, 112, 98, 215, 145,
            208, 44, 188, 70, 10, 180, 124, 230, 15, 98, 165, 104, 85,
        ]);
        let point_y = PodEdwardsPoint([
            174, 86, 89, 208, 236, 123, 223, 128, 75, 54, 228, 232, 220, 100, 205, 108, 237, 97,
            105, 79, 74, 192, 67, 224, 185, 23, 157, 116, 216, 151, 223, 81,
        ]);

        let ax = multiply_edwards(&scalar_a, &point_x).unwrap();
        let bx = multiply_edwards(&scalar_a, &point_y).unwrap();

        assert_eq!(
            add_edwards(&ax, &bx),
            multiply_edwards(&scalar_a, &add_edwards(&point_x, &point_y).unwrap()),
        );
    }

    #[test]
    fn test_multiscalar_multiplication_edwards() {
        let scalar = PodScalar([
            205, 73, 127, 173, 83, 80, 190, 66, 202, 3, 237, 77, 52, 223, 238, 70, 80, 242, 24, 87,
            111, 84, 49, 63, 194, 76, 202, 108, 62, 240, 83, 15,
        ]);
        let point = PodEdwardsPoint([
            222, 174, 184, 139, 143, 122, 253, 96, 0, 207, 120, 157, 112, 38, 54, 189, 91, 144, 78,
            111, 111, 122, 140, 183, 65, 250, 191, 133, 6, 42, 212, 93,
        ]);

        let basic_product = multiply_edwards(&scalar, &point).unwrap();
        let msm_product = multiscalar_multiply_edwards(&[scalar], &[point]).unwrap();

        assert_eq!(basic_product, msm_product);

        let scalar_a = PodScalar([
            246, 154, 34, 110, 31, 185, 50, 1, 252, 194, 163, 56, 211, 18, 101, 192, 57, 225, 207,
            69, 19, 84, 231, 118, 137, 175, 148, 218, 106, 212, 69, 9,
        ]);
        let scalar_b = PodScalar([
            27, 58, 126, 136, 253, 178, 176, 245, 246, 55, 15, 202, 35, 183, 66, 199, 134, 187,
            169, 154, 66, 120, 169, 193, 75, 4, 33, 241, 126, 227, 59, 3,
        ]);
        let point_x = PodEdwardsPoint([
            252, 31, 230, 46, 173, 95, 144, 148, 158, 157, 63, 10, 8, 68, 58, 176, 142, 192, 168,
            53, 61, 105, 194, 166, 43, 56, 246, 236, 28, 146, 114, 133,
        ]);
        let point_y = PodEdwardsPoint([
            10, 111, 8, 236, 97, 189, 124, 69, 89, 176, 222, 39, 199, 253, 111, 11, 248, 186, 128,
            90, 120, 128, 248, 210, 232, 183, 93, 104, 111, 150, 7, 241,
        ]);

        let ax = multiply_edwards(&scalar_a, &point_x).unwrap();
        let by = multiply_edwards(&scalar_b, &point_y).unwrap();
        let basic_product = add_edwards(&ax, &by).unwrap();
        let msm_product =
            multiscalar_multiply_edwards(&[scalar_a, scalar_b], &[point_x, point_y]).unwrap();

        assert_eq!(basic_product, msm_product);
    }
}
