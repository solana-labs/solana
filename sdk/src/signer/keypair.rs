#[deprecated(since = "2.2.0", note = "Use `solana-keypair` crate instead")]
pub use solana_keypair::{
    keypair_from_seed, keypair_from_seed_phrase_and_passphrase, read_keypair, read_keypair_file,
    seed_derivable::keypair_from_seed_and_derivation_path, write_keypair, write_keypair_file,
    Keypair,
};
#[deprecated(since = "2.2.0", note = "Use `solana-seed-phrase` crate instead")]
pub use solana_seed_phrase::generate_seed_from_seed_phrase_and_passphrase;
#[deprecated(since = "2.2.0", note = "Use `solana-signer` crate instead")]
pub use solana_signer::*;
