//! Off-chain message container for storing non-transaction messages.

#![cfg(feature = "full")]

use {
    crate::{
        hash::Hash,
        pubkey::Pubkey,
        signature::Signature,
        signer::{Signer, SignerError},
    },
    num_enum::{IntoPrimitive, TryFromPrimitive},
    std::str::FromStr,
    thiserror::Error,
};

#[cfg(test)]
static_assertions::const_assert_eq!(OffchainMessage::HEADER_LEN, 17);
#[cfg(test)]
static_assertions::const_assert_eq!(v0::OffchainMessage::MAX_LEN, 65515);
#[cfg(test)]
static_assertions::const_assert_eq!(v0::OffchainMessage::MAX_LEN_LEDGER, 1212);

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("message is empty")]
    MessageEmpty,
    #[error("message is too long")]
    MessageTooLong,
    #[error("message candidate buffer is too short")]
    BufferTooShort,
    #[error("message candidate buffer is too long")]
    BufferTooLong,
    #[error("unexpected message format value: {0}")]
    UnexpectedFormatDiscriminant(u8),
    #[error("unexpected message encoding")]
    UnexpectedMessageEncoding,
    #[error("unexpected message version value: {0}")]
    UnexpectedVersionDiscriminant(u8),
    #[error("message is illegal for specified format")]
    IllegalMessageForFormat,
    #[error(transparent)]
    SignerError(#[from] SignerError),
}

/// Check if given bytes contain only printable ASCII characters
pub fn is_printable_ascii(message: &str) -> bool {
    message
        .chars()
        .all(|c| c.is_ascii() && !c.is_ascii_control())
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone, TryFromPrimitive, IntoPrimitive)]
pub enum MessageFormat {
    RestrictedAscii,
    LimitedUtf8,
    ExtendedUtf8,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
pub enum Version {
    V0 = 0,
}

#[derive(Debug, Error, Eq, PartialEq)]
#[error("invalid value for offchain message version: `{0}`")]
pub struct VersionFromStrError(String);
impl FromStr for Version {
    type Err = VersionFromStrError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(version) = s.parse::<<Self as TryFromPrimitive>::Primitive>() {
            if let Ok(version) = Version::try_from_primitive(version) {
                return Ok(version);
            }
        }
        Err(VersionFromStrError(s.to_string()))
    }
}

#[allow(clippy::arithmetic_side_effects)]
pub mod v0 {
    use {
        super::{is_printable_ascii, Error, MessageFormat, OffchainMessage as Base},
        crate::{
            hash::{Hash, Hasher},
            packet::PACKET_DATA_SIZE,
        },
    };

    /// OffchainMessage Version 0.
    /// Struct always contains a non-empty valid message.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct OffchainMessage {
        format: MessageFormat,
        message: String,
    }

    impl OffchainMessage {
        // Header Length = Message Format (1) + Message Length (2)
        pub const HEADER_LEN: usize = 3;
        // Max length of the OffchainMessage
        pub const MAX_LEN: usize = u16::MAX as usize - Base::HEADER_LEN - Self::HEADER_LEN;
        // Max Length of the OffchainMessage supported by the Ledger
        pub const MAX_LEN_LEDGER: usize = PACKET_DATA_SIZE - Base::HEADER_LEN - Self::HEADER_LEN;

        /// Construct a new OffchainMessage object from the given message
        pub fn new(message: &str) -> Result<Self, Error> {
            let format = if message.is_empty() {
                return Err(Error::MessageEmpty);
            } else if message.len() <= OffchainMessage::MAX_LEN_LEDGER {
                if is_printable_ascii(message) {
                    MessageFormat::RestrictedAscii
                } else {
                    MessageFormat::LimitedUtf8
                }
            } else if message.len() <= OffchainMessage::MAX_LEN {
                MessageFormat::ExtendedUtf8
            } else {
                return Err(Error::MessageTooLong);
            };
            Ok(Self {
                format,
                message: message.to_string(),
            })
        }

        /// Serialize the message to bytes, including the full header
        pub fn serialize(&self, data: &mut Vec<u8>) -> Result<(), Error> {
            // invalid messages shouldn't be possible, but a quick sanity check never hurts
            assert!(!self.message.is_empty() && self.message.len() <= Self::MAX_LEN);
            data.reserve(Self::HEADER_LEN.saturating_add(self.message.len()));
            // format
            data.push(self.format.into());
            // message length
            data.extend_from_slice(&(self.message.len() as u16).to_le_bytes());
            // message
            data.extend_from_slice(self.message.as_bytes());
            Ok(())
        }

        /// Deserialize the message from bytes that include a full header
        pub fn deserialize(data: &[u8]) -> Result<Self, Error> {
            // validate data length
            if data.len() <= Self::HEADER_LEN {
                return Err(Error::BufferTooShort);
            }
            if data.len() > Self::HEADER_LEN + Self::MAX_LEN {
                return Err(Error::BufferTooLong);
            }
            // decode header
            let format = MessageFormat::try_from(data[0])
                .map_err(|e| Error::UnexpectedFormatDiscriminant(e.number))?;
            let message_len = u16::from_le_bytes([data[1], data[2]]) as usize;
            // check header
            let expected_buffer_len = Self::HEADER_LEN.saturating_add(message_len);
            if expected_buffer_len > data.len() {
                return Err(Error::BufferTooLong);
            }
            if expected_buffer_len < data.len() {
                return Err(Error::BufferTooShort);
            }
            let message_bytes = &data[Self::HEADER_LEN..];
            let message =
                std::str::from_utf8(message_bytes).map_err(|_| Error::UnexpectedMessageEncoding)?;
            // check format
            match format {
                MessageFormat::RestrictedAscii => {
                    (message.len() <= Self::MAX_LEN_LEDGER) && is_printable_ascii(message)
                }
                MessageFormat::LimitedUtf8 => message.len() <= Self::MAX_LEN_LEDGER,
                MessageFormat::ExtendedUtf8 => message.len() <= Self::MAX_LEN,
            }
            .then_some(Self {
                format,
                message: message.to_string(),
            })
            .ok_or(Error::IllegalMessageForFormat)
        }

        /// Compute the SHA256 hash of the serialized off-chain message
        pub fn hash(serialized_message: &[u8]) -> Result<Hash, Error> {
            let mut hasher = Hasher::default();
            hasher.hash(serialized_message);
            Ok(hasher.result())
        }

        pub fn get_format(&self) -> MessageFormat {
            self.format
        }

        pub fn get_message(&self) -> &str {
            &self.message
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum OffchainMessage {
    V0(v0::OffchainMessage),
}

impl OffchainMessage {
    pub const SIGNING_DOMAIN: &'static [u8] = b"\xffsolana offchain";
    // Header Length = Signing Domain (16) + Header Version (1)
    pub const HEADER_LEN: usize = Self::SIGNING_DOMAIN.len() + 1;

    /// Construct a new OffchainMessage object from the given version and message
    pub fn new(version: Version, message: &str) -> Result<Self, Error> {
        match version {
            Version::V0 => Ok(Self::V0(v0::OffchainMessage::new(message)?)),
        }
    }

    /// Serialize the off-chain message to bytes including full header
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        // serialize signing domain
        let mut data = Self::SIGNING_DOMAIN.to_vec();

        // serialize version and call version specific serializer
        match self {
            Self::V0(msg) => {
                data.push(0);
                msg.serialize(&mut data)?;
            }
        }
        Ok(data)
    }

    /// Deserialize the off-chain message from bytes that include full header
    pub fn deserialize(data: &[u8]) -> Result<Self, Error> {
        if data.len() <= Self::HEADER_LEN {
            return Err(Error::BufferTooShort);
        }
        let version = Version::try_from_primitive(data[Self::SIGNING_DOMAIN.len()])
            .map_err(|e| Error::UnexpectedVersionDiscriminant(e.number))?;
        let data = &data[Self::SIGNING_DOMAIN.len().saturating_add(1)..];
        match version {
            Version::V0 => Ok(Self::V0(v0::OffchainMessage::deserialize(data)?)),
        }
    }

    /// Compute the hash of the off-chain message
    pub fn hash(&self) -> Result<Hash, Error> {
        match self {
            Self::V0(_) => v0::OffchainMessage::hash(&self.serialize()?),
        }
    }

    pub fn get_version(&self) -> Version {
        match self {
            Self::V0(_) => Version::V0,
        }
    }

    pub fn get_format(&self) -> MessageFormat {
        match self {
            Self::V0(msg) => msg.get_format(),
        }
    }

    pub fn get_message(&self) -> &str {
        match self {
            Self::V0(msg) => msg.get_message(),
        }
    }

    /// Sign the message with provided keypair
    pub fn sign(&self, signer: &dyn Signer) -> Result<Signature, Error> {
        Ok(signer.sign_message(&self.serialize()?))
    }

    /// Verify that the message signature is valid for the given public key
    pub fn verify(&self, signer: &Pubkey, signature: &Signature) -> Result<bool, Error> {
        Ok(signature.verify(signer.as_ref(), &self.serialize()?))
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::signature::Keypair, std::str::FromStr};

    #[test]
    fn test_offchain_message_ascii() {
        let message = OffchainMessage::new(Version::V0, "Test Message").unwrap();
        assert_eq!(message.get_version(), Version::V0);
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message(), "Test Message");
        assert!(
            matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == MessageFormat::RestrictedAscii)
        );
        let serialized = [
            255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 0, 12, 0,
            84, 101, 115, 116, 32, 77, 101, 115, 115, 97, 103, 101,
        ];
        let hash = Hash::from_str("HG5JydBGjtjTfD3sSn21ys5NTWPpXzmqifiGC2BVUjkD").unwrap();
        assert_eq!(message.serialize().unwrap(), serialized);
        assert_eq!(message.hash().unwrap(), hash);
        assert_eq!(message, OffchainMessage::deserialize(&serialized).unwrap());
    }

    #[test]
    fn test_offchain_message_utf8() {
        let message = OffchainMessage::new(Version::V0, "Тестовое сообщение").unwrap();
        assert_eq!(message.get_version(), Version::V0);
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), "Тестовое сообщение",);
        assert!(
            matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == MessageFormat::LimitedUtf8)
        );
        let serialized = [
            255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 1, 35, 0,
            208, 162, 208, 181, 209, 129, 209, 130, 208, 190, 208, 178, 208, 190, 208, 181, 32,
            209, 129, 208, 190, 208, 190, 208, 177, 209, 137, 208, 181, 208, 189, 208, 184, 208,
            181,
        ];
        let hash = Hash::from_str("6GXTveatZQLexkX4WeTpJ3E7uk1UojRXpKp43c4ArSun").unwrap();
        assert_eq!(message.serialize().unwrap(), serialized);
        assert_eq!(message.hash().unwrap(), hash);
        assert_eq!(message, OffchainMessage::deserialize(&serialized).unwrap());
    }

    #[test]
    fn test_offchain_message_sign_and_verify() {
        let message = OffchainMessage::new(Version::V0, "Test Message").unwrap();
        let keypair = Keypair::new();
        let signature = message.sign(&keypair).unwrap();
        assert!(message.verify(&keypair.pubkey(), &signature).unwrap());
    }

    #[test]
    fn test_version_from_str() {
        assert!(matches!(Version::from_str("0"), Ok(Version::V0)));
        assert_eq!(
            Version::from_str(""),
            Err(VersionFromStrError(String::new()))
        );
        assert_eq!(
            Version::from_str("1"),
            Err(VersionFromStrError(String::from("1")))
        );
        assert_eq!(
            Version::from_str("~"),
            Err(VersionFromStrError(String::from("~")))
        );
    }
}
