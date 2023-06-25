//! The native ZK Token proof program.
//!
//! The program verifies a number of zero-knowledge proofs that are tailored to work with Pedersen
//! commitments and ElGamal encryption over the elliptic curve curve25519. A general overview of
//! the program as well as the technical details of some of the proof instructions can be found in
//! the [`ZK Token proof`] documentation.
//!
//! [`ZK Token proof`]: https://edge.docs.solana.com/developing/runtime-facilities/zk-token-proof

// Program Id of the ZkToken Proof program
solana_program::declare_id!("ZkTokenProof1111111111111111111111111111111");
