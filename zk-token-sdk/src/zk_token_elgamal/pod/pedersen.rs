use {
    crate::zk_token_elgamal::pod::{Pod, Zeroable},
    std::fmt,
};

#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct PedersenCommitment(pub [u8; 32]);

impl fmt::Debug for PedersenCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct DecryptHandle(pub [u8; 32]);

impl fmt::Debug for DecryptHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
