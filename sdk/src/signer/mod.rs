//! Abstractions and implementations for transaction signers.

#![cfg(feature = "full")]

use {
    crate::{
        derivation_path::DerivationPath,
        pubkey::Pubkey,
        signature::{PresignerError, Signature},
        transaction::TransactionError,
    },
    itertools::Itertools,
    std::{
        error,
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        ops::Deref,
        path::Path,
    },
    thiserror::Error,
};

pub mod keypair;
pub mod null_signer;
pub mod presigner;
pub mod signers;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignerError {
    #[error("keypair-pubkey mismatch")]
    KeypairPubkeyMismatch,

    #[error("not enough signers")]
    NotEnoughSigners,

    #[error("transaction error")]
    TransactionError(#[from] TransactionError),

    #[error("custom error: {0}")]
    Custom(String),

    // Presigner-specific Errors
    #[error("presigner error")]
    PresignerError(#[from] PresignerError),

    // Remote Keypair-specific Errors
    #[error("connection error: {0}")]
    Connection(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("no device found")]
    NoDeviceFound,

    #[error("{0}")]
    Protocol(String),

    #[error("{0}")]
    UserCancel(String),

    #[error("too many signers")]
    TooManySigners,
}

/// The `Signer` trait declares operations that all digital signature providers
/// must support. It is the primary interface by which signers are specified in
/// `Transaction` signing interfaces
pub trait Signer {
    /// Infallibly gets the implementor's public key. Returns the all-zeros
    /// `Pubkey` if the implementor has none.
    fn pubkey(&self) -> Pubkey {
        self.try_pubkey().unwrap_or_default()
    }
    /// Fallibly gets the implementor's public key
    fn try_pubkey(&self) -> Result<Pubkey, SignerError>;
    /// Infallibly produces an Ed25519 signature over the provided `message`
    /// bytes. Returns the all-zeros `Signature` if signing is not possible.
    fn sign_message(&self, message: &[u8]) -> Signature {
        self.try_sign_message(message).unwrap_or_default()
    }
    /// Fallibly produces an Ed25519 signature over the provided `message` bytes.
    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError>;
    /// Whether the impelmentation requires user interaction to sign
    fn is_interactive(&self) -> bool;
}

impl<T> From<T> for Box<dyn Signer>
where
    T: Signer + 'static,
{
    fn from(signer: T) -> Self {
        Box::new(signer)
    }
}

impl<Container: Deref<Target = impl Signer>> Signer for Container {
    #[inline]
    fn pubkey(&self) -> Pubkey {
        self.deref().pubkey()
    }

    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        self.deref().try_pubkey()
    }

    fn sign_message(&self, message: &[u8]) -> Signature {
        self.deref().sign_message(message)
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        self.deref().try_sign_message(message)
    }

    fn is_interactive(&self) -> bool {
        self.deref().is_interactive()
    }
}

impl PartialEq for dyn Signer {
    fn eq(&self, other: &dyn Signer) -> bool {
        self.pubkey() == other.pubkey()
    }
}

impl Eq for dyn Signer {}

impl std::fmt::Debug for dyn Signer {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "Signer: {:?}", self.pubkey())
    }
}

/// Removes duplicate signers while preserving order. O(n²)
pub fn unique_signers(signers: Vec<&dyn Signer>) -> Vec<&dyn Signer> {
    signers.into_iter().unique_by(|s| s.pubkey()).collect()
}

/// The `EncodableKey` trait defines the interface by which cryptographic keys/keypairs are read,
/// written, and derived from sources.
pub trait EncodableKey: Sized {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Box<dyn error::Error>>;
    fn read_from_file<F: AsRef<Path>>(path: F) -> Result<Self, Box<dyn error::Error>> {
        let mut file = File::open(path.as_ref())?;
        Self::read(&mut file)
    }
    fn write<W: Write>(&self, writer: &mut W) -> Result<String, Box<dyn error::Error>>;
    fn write_to_file<F: AsRef<Path>>(&self, outfile: F) -> Result<String, Box<dyn error::Error>> {
        let outfile = outfile.as_ref();

        if let Some(outdir) = outfile.parent() {
            fs::create_dir_all(outdir)?;
        }

        let mut f = {
            #[cfg(not(unix))]
            {
                OpenOptions::new()
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                OpenOptions::new().mode(0o600)
            }
        }
        .write(true)
        .truncate(true)
        .create(true)
        .open(outfile)?;

        self.write(&mut f)
    }
}

/// The `SeedDerivable` trait defines the interface by which cryptographic keys/keypairs are
/// derived from byte seeds, derivation paths, and passphrases.
pub trait SeedDerivable: Sized {
    fn from_seed(seed: &[u8]) -> Result<Self, Box<dyn error::Error>>;
    fn from_seed_and_derivation_path(
        seed: &[u8],
        derivation_path: Option<DerivationPath>,
    ) -> Result<Self, Box<dyn error::Error>>;
    fn from_seed_phrase_and_passphrase(
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<Self, Box<dyn error::Error>>;
}

/// The `EncodableKeypair` trait extends `EncodableKey` for asymmetric keypairs, i.e. have
/// associated public keys.
pub trait EncodableKeypair: EncodableKey {
    type Pubkey: ToString;

    /// Returns an encodable representation of the associated public key.
    fn encodable_pubkey(&self) -> Self::Pubkey;
}

#[cfg(test)]
mod tests {
    use {super::*, crate::signer::keypair::Keypair};

    fn pubkeys(signers: &[&dyn Signer]) -> Vec<Pubkey> {
        signers.iter().map(|x| x.pubkey()).collect()
    }

    #[test]
    fn test_unique_signers() {
        let alice = Keypair::new();
        let bob = Keypair::new();
        assert_eq!(
            pubkeys(&unique_signers(vec![&alice, &bob, &alice])),
            pubkeys(&[&alice, &bob])
        );
    }

    #[test]
    fn test_containers() {
        use std::{rc::Rc, sync::Arc};

        struct Foo<S: Signer> {
            #[allow(unused)]
            signer: S,
        }

        fn foo(_s: impl Signer) {}

        let _arc_signer = Foo {
            signer: Arc::new(Keypair::new()),
        };
        foo(Arc::new(Keypair::new()));

        let _rc_signer = Foo {
            signer: Rc::new(Keypair::new()),
        };
        foo(Rc::new(Keypair::new()));

        let _ref_signer = Foo {
            signer: &Keypair::new(),
        };
        foo(&Keypair::new());

        let _box_signer = Foo {
            signer: Box::new(Keypair::new()),
        };
        foo(Box::new(Keypair::new()));

        let _signer = Foo {
            signer: Keypair::new(),
        };
        foo(Keypair::new());
    }
}
