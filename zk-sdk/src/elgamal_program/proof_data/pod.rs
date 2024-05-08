use {
    crate::elgamal_program::proof_data::{errors::ProofDataError, ProofType},
    bytemuck::{Pod, Zeroable},
    num_traits::{FromPrimitive, ToPrimitive},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodProofType(u8);
impl From<ProofType> for PodProofType {
    fn from(proof_type: ProofType) -> Self {
        Self(ToPrimitive::to_u8(&proof_type).unwrap())
    }
}
impl TryFrom<PodProofType> for ProofType {
    type Error = ProofDataError;

    fn try_from(pod: PodProofType) -> Result<Self, Self::Error> {
        FromPrimitive::from_u8(pod.0).ok_or(Self::Error::InvalidProofType)
    }
}
