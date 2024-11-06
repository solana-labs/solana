#![cfg(feature = "full")]
#[deprecated(since = "2.2.0", note = "Use `solana-presigner` crate instead")]
pub use solana_presigner as presigner;
#[deprecated(since = "2.2.0", note = "Use `solana-seed-derivable` crate instead")]
pub use solana_seed_derivable::SeedDerivable;
#[deprecated(since = "2.2.0", note = "Use `solana-signer` crate instead")]
pub use solana_signer::{
    null_signer, signers, unique_signers, EncodableKey, EncodableKeypair, Signer, SignerError,
};
pub mod keypair;
