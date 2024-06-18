use thiserror::Error;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum Curve25519Error {
    #[error("pod conversion failed")]
    PodConversion,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ElGamalError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,
    #[error("seed length too short for derivation")]
    SeedLengthTooShort,
    #[error("seed length too long for derivation")]
    SeedLengthTooLong,
    #[error("failed to deserialize ciphertext")]
    CiphertextDeserialization,
    #[error("failed to deserialize public key")]
    PubkeyDeserialization,
    #[error("failed to deserialize keypair")]
    KeypairDeserialization,
    #[error("failed to deserialize secret key")]
    SecretKeyDeserialization,
}
