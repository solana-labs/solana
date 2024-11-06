//! Abstractions and implementations for transaction signers.
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
use {
    core::fmt,
    solana_pubkey::Pubkey,
    solana_signature::Signature,
    solana_transaction_error::TransactionError,
    std::{
        error,
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        ops::Deref,
        path::Path,
    },
};

pub mod null_signer;
pub mod signers;

#[derive(Debug, PartialEq, Eq)]
pub enum PresignerError {
    VerificationFailure,
}

impl std::error::Error for PresignerError {}

impl fmt::Display for PresignerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::VerificationFailure => f.write_str("pre-generated signature cannot verify data"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SignerError {
    KeypairPubkeyMismatch,
    NotEnoughSigners,
    TransactionError(TransactionError),
    Custom(String),
    // Presigner-specific Errors
    PresignerError(PresignerError),
    // Remote Keypair-specific Errors
    Connection(String),
    InvalidInput(String),
    NoDeviceFound,
    Protocol(String),
    UserCancel(String),
    TooManySigners,
}

impl std::error::Error for SignerError {
    fn source(&self) -> ::core::option::Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::KeypairPubkeyMismatch => None,
            Self::NotEnoughSigners => None,
            Self::TransactionError(e) => Some(e),
            Self::Custom(_) => None,
            Self::PresignerError(e) => Some(e),
            Self::Connection(_) => None,
            Self::InvalidInput(_) => None,
            Self::NoDeviceFound => None,
            Self::Protocol(_) => None,
            Self::UserCancel(_) => None,
            Self::TooManySigners => None,
        }
    }
}
impl fmt::Display for SignerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SignerError::KeypairPubkeyMismatch => f.write_str("keypair-pubkey mismatch"),
            SignerError::NotEnoughSigners => f.write_str("not enough signers"),
            SignerError::TransactionError(_) => f.write_str("transaction error"),
            SignerError::Custom(e) => write!(f, "custom error: {e}",),
            SignerError::PresignerError(_) => f.write_str("presigner error"),
            SignerError::Connection(e) => write!(f, "connection error: {e}",),
            SignerError::InvalidInput(s) => write!(f, "invalid input: {s}",),
            SignerError::NoDeviceFound => f.write_str("no device found"),
            SignerError::Protocol(s) => {
                write!(f, "{s}")
            }
            SignerError::UserCancel(s) => {
                write!(f, "{s}")
            }
            SignerError::TooManySigners => f.write_str("too many signers"),
        }
    }
}

impl From<TransactionError> for SignerError {
    fn from(source: TransactionError) -> Self {
        SignerError::TransactionError(source)
    }
}

impl From<PresignerError> for SignerError {
    fn from(source: PresignerError) -> Self {
        SignerError::PresignerError(source)
    }
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
    /// Whether the implementation requires user interaction to sign
    fn is_interactive(&self) -> bool;
}

/// This implements `Signer` for all ptr types - `Box/Rc/Arc/&/&mut` etc
impl<Container: Deref<Target = impl Signer + ?Sized>> Signer for Container {
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
    let capacity = signers.len();
    let mut out = Vec::with_capacity(capacity);
    let mut seen = std::collections::HashSet::with_capacity(capacity);
    for signer in signers {
        let pubkey = signer.pubkey();
        if !seen.contains(&pubkey) {
            seen.insert(pubkey);
            out.push(signer);
        }
    }
    out
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

/// The `EncodableKeypair` trait extends `EncodableKey` for asymmetric keypairs, i.e. have
/// associated public keys.
pub trait EncodableKeypair: EncodableKey {
    type Pubkey: ToString;

    /// Returns an encodable representation of the associated public key.
    fn encodable_pubkey(&self) -> Self::Pubkey;
}
