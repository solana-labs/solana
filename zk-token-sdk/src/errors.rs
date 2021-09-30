//! Errors related to proving and verifying proofs.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProofError {
    /// This error occurs when a proof failed to verify.
    VerificationError,
    /// This error occurs when the proof encoding is malformed.
    FormatError,
    /// This error occurs during proving if the number of blinding
    /// factors does not match the number of values.
    WrongNumBlindingFactors,
    /// This error occurs when attempting to create a proof with
    /// bitsize other than \\(8\\), \\(16\\), \\(32\\), or \\(64\\).
    InvalidBitsize,
    /// This error occurs when there are insufficient generators for the proof.
    InvalidGeneratorsLength,
    /// This error occurs a `zk_token_elgamal::pod::ElGamalCT` contains invalid ElGamalCT ciphertext
    InconsistentCTData,
}
