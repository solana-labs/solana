use crate::pod::{elgamal::*, pedersen::*, PodU16, PodU64};
pub use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct CompressedRistretto(pub [u8; 32]);
