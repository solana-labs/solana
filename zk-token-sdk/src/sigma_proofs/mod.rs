//! Collection of sigma proofs (more precisely, "arguments") that are used in the Solana zk-token
//! protocol.
//!
//! The module contains implementations of the following proof systems that work on Pedersen
//! commitments and twisted ElGamal ciphertexts:
//! - Equality proof: can be used to certify that a twisted ElGamal ciphertext encrypts the same
//! message as either a Pedersen commitment or another ElGamal ciphertext.
//! - Validity proof: can be used to certify that a twisted ElGamal ciphertext is a properly-formed
//! ciphertext with respect to a pair of ElGamal public keys.
//! - Zero-balance proof: can be used to certify that a twisted ElGamal ciphertext encrypts the
//! message 0.
//! - Fee proof: can be used to certify that an ElGamal ciphertext properly encrypts a transfer
//! fee.
//!
//! We refer to the zk-token paper for the formal details and security proofs of these argument
//! systems.

pub mod equality_proof;
pub mod errors;
pub mod fee_proof;
pub mod validity_proof;
pub mod zero_balance_proof;
