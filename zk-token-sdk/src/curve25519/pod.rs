pub use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodEdwardsPoint(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodRistrettoPoint(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodScalar(pub [u8; 32]);

#[cfg(not(target_arch = "bpf"))]
mod target_arch {
    use {
        super::*,
        crate::curve25519::errors::Curve25519Error,
        curve25519_dalek::{
            edwards::{CompressedEdwardsY, EdwardsPoint},
            ristretto::{CompressedRistretto, RistrettoPoint},
            scalar::Scalar,
        },
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
}
