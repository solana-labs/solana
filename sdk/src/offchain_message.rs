//! Off-chain message container for storing non-transaction messages.

#![cfg(feature = "full")]

use {
    crate::{
        hash::Hash,
        pubkey::Pubkey,
        sanitize::SanitizeError,
        signature::{Signature, Signer},
    },
    num_enum::{IntoPrimitive, TryFromPrimitive},
};

#[cfg(test)]
static_assertions::const_assert_eq!(OffchainMessage::HEADER_LEN, 17);
#[cfg(test)]
static_assertions::const_assert_eq!(v0::OffchainMessage::MAX_LEN, 65515);
#[cfg(test)]
static_assertions::const_assert_eq!(v0::OffchainMessage::MAX_LEN_LEDGER, 1212);

/// Check if given bytes contain only printable ASCII characters
pub fn is_printable_ascii(data: &[u8]) -> bool {
    for &char in data {
        if !(0x20..=0x7e).contains(&char) {
            return false;
        }
    }
    true
}

/// Check if given bytes contain valid UTF8 string
pub fn is_utf8(data: &[u8]) -> bool {
    std::str::from_utf8(data).is_ok()
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone, TryFromPrimitive, IntoPrimitive)]
pub enum MessageFormat {
    RestrictedAscii,
    LimitedUtf8,
    ExtendedUtf8,
}

#[allow(clippy::integer_arithmetic)]
pub mod v0 {
    use {
        super::{is_printable_ascii, is_utf8, MessageFormat, OffchainMessage as Base},
        crate::{
            hash::{Hash, Hasher},
            packet::PACKET_DATA_SIZE,
            sanitize::SanitizeError,
        },
    };

    /// OffchainMessage Version 0.
    /// Struct always contains a non-empty valid message.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct OffchainMessage {
        format: MessageFormat,
        message: Vec<u8>,
    }

    impl OffchainMessage {
        // Header Length = Message Format (1) + Message Length (2)
        pub const HEADER_LEN: usize = 3;
        // Max length of the OffchainMessage
        pub const MAX_LEN: usize = u16::MAX as usize - Base::HEADER_LEN - Self::HEADER_LEN;
        // Max Length of the OffchainMessage supported by the Ledger
        pub const MAX_LEN_LEDGER: usize = PACKET_DATA_SIZE - Base::HEADER_LEN - Self::HEADER_LEN;

        /// Construct a new OffchainMessage object from the given message
        pub fn new(message: &[u8]) -> Result<Self, SanitizeError> {
            let format = if message.is_empty() {
                return Err(SanitizeError::InvalidValue);
            } else if message.len() <= OffchainMessage::MAX_LEN_LEDGER {
                if is_printable_ascii(message) {
                    MessageFormat::RestrictedAscii
                } else if is_utf8(message) {
                    MessageFormat::LimitedUtf8
                } else {
                    return Err(SanitizeError::InvalidValue);
                }
            } else if message.len() <= OffchainMessage::MAX_LEN {
                if is_utf8(message) {
                    MessageFormat::ExtendedUtf8
                } else {
                    return Err(SanitizeError::InvalidValue);
                }
            } else {
                return Err(SanitizeError::ValueOutOfBounds);
            };
            Ok(Self {
                format,
                message: message.to_vec(),
            })
        }

        /// Serialize the message to bytes, including the full header
        pub fn serialize(&self, data: &mut Vec<u8>) -> Result<(), SanitizeError> {
            // invalid messages shouldn't be possible, but a quick sanity check never hurts
            assert!(!self.message.is_empty() && self.message.len() <= Self::MAX_LEN);
            data.reserve(Self::HEADER_LEN.saturating_add(self.message.len()));
            // format
            data.push(self.format.into());
            // message length
            data.extend_from_slice(&(self.message.len() as u16).to_le_bytes());
            // message
            data.extend_from_slice(&self.message);
            Ok(())
        }

        /// Deserialize the message from bytes that include a full header
        pub fn deserialize(data: &[u8]) -> Result<Self, SanitizeError> {
            // validate data length
            if data.len() <= Self::HEADER_LEN || data.len() > Self::HEADER_LEN + Self::MAX_LEN {
                return Err(SanitizeError::ValueOutOfBounds);
            }
            // decode header
            let format =
                MessageFormat::try_from(data[0]).map_err(|_| SanitizeError::InvalidValue)?;
            let message_len = u16::from_le_bytes([data[1], data[2]]) as usize;
            // check header
            if Self::HEADER_LEN.saturating_add(message_len) != data.len() {
                return Err(SanitizeError::InvalidValue);
            }
            let message = &data[Self::HEADER_LEN..];
            // check format
            let is_valid = match format {
                MessageFormat::RestrictedAscii => {
                    (message.len() <= Self::MAX_LEN_LEDGER) && is_printable_ascii(message)
                }
                MessageFormat::LimitedUtf8 => {
                    (message.len() <= Self::MAX_LEN_LEDGER) && is_utf8(message)
                }
                MessageFormat::ExtendedUtf8 => (message.len() <= Self::MAX_LEN) && is_utf8(message),
            };

            if is_valid {
                Ok(Self {
                    format,
                    message: message.to_vec(),
                })
            } else {
                Err(SanitizeError::InvalidValue)
            }
        }

        /// Compute the SHA256 hash of the serialized off-chain message
        pub fn hash(serialized_message: &[u8]) -> Result<Hash, SanitizeError> {
            let mut hasher = Hasher::default();
            hasher.hash(serialized_message);
            Ok(hasher.result())
        }

        pub fn get_format(&self) -> MessageFormat {
            self.format
        }

        pub fn get_message(&self) -> &Vec<u8> {
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
    pub fn new(version: u8, message: &[u8]) -> Result<Self, SanitizeError> {
        match version {
            0 => Ok(Self::V0(v0::OffchainMessage::new(message)?)),
            _ => Err(SanitizeError::ValueOutOfBounds),
        }
    }

    /// Serialize the off-chain message to bytes including full header
    pub fn serialize(&self) -> Result<Vec<u8>, SanitizeError> {
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
    pub fn deserialize(data: &[u8]) -> Result<Self, SanitizeError> {
        if data.len() <= Self::HEADER_LEN {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        let version = data[Self::SIGNING_DOMAIN.len()];
        let data = &data[Self::SIGNING_DOMAIN.len().saturating_add(1)..];
        match version {
            0 => Ok(Self::V0(v0::OffchainMessage::deserialize(data)?)),
            _ => Err(SanitizeError::ValueOutOfBounds),
        }
    }

    /// Compute the hash of the off-chain message
    pub fn hash(&self) -> Result<Hash, SanitizeError> {
        match self {
            Self::V0(_) => v0::OffchainMessage::hash(&self.serialize()?),
        }
    }

    pub fn get_version(&self) -> u8 {
        match self {
            Self::V0(_) => 0,
        }
    }

    pub fn get_format(&self) -> MessageFormat {
        match self {
            Self::V0(msg) => msg.get_format(),
        }
    }

    pub fn get_message(&self) -> &Vec<u8> {
        match self {
            Self::V0(msg) => msg.get_message(),
        }
    }

    /// Sign the message with provided keypair
    pub fn sign(&self, signer: &dyn Signer) -> Result<Signature, SanitizeError> {
        Ok(signer.sign_message(&self.serialize()?))
    }

    /// Verify that the message signature is valid for the given public key
    pub fn verify(&self, signer: &Pubkey, signature: &Signature) -> Result<bool, SanitizeError> {
        Ok(signature.verify(signer.as_ref(), &self.serialize()?))
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::signature::Keypair, std::str::FromStr};

    #[test]
    fn test_offchain_message_ascii() {
        let message = OffchainMessage::new(0, b"Test Message").unwrap();
        assert_eq!(message.get_version(), 0);
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message().as_slice(), b"Test Message");
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
        let message = OffchainMessage::new(0, "Тестовое сообщение".as_bytes()).unwrap();
        assert_eq!(message.get_version(), 0);
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(
            message.get_message().as_slice(),
            "Тестовое сообщение".as_bytes()
        );
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
        let message = OffchainMessage::new(0, b"Test Message").unwrap();
        let keypair = Keypair::new();
        let signature = message.sign(&keypair).unwrap();
        assert!(message.verify(&keypair.pubkey(), &signature).unwrap());
    }
}
