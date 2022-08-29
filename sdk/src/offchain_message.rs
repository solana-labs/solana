//! Off-Chain Message container for storing non-transaction messages.

#![cfg(feature = "full")]

use crate::{
    hash::Hash,
    pubkey::Pubkey,
    sanitize::SanitizeError,
    signature::{Signature, Signer},
};

#[allow(clippy::integer_arithmetic)]
pub mod v0 {
    use {
        super::OffchainMessage as Base,
        crate::{
            hash::{Hash, Hasher},
            sanitize::SanitizeError,
        },
    };

    /// OffchainMessage Version 0.
    /// Struct always contains a non-empty valid message.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct OffchainMessage {
        format: u8,
        message: Vec<u8>,
    }

    impl OffchainMessage {
        pub const MAX_LEN: usize = 65515;
        pub const MAX_LEN_LEDGER: usize = 1212;
        pub const HEADER_LEN: usize = 20;

        /// Construct a new OffchainMessage object from the given message
        pub fn new(message: &[u8]) -> Result<Self, SanitizeError> {
            let format = if message.is_empty() {
                return Err(SanitizeError::InvalidValue);
            } else if message.len() <= OffchainMessage::MAX_LEN_LEDGER {
                if OffchainMessage::is_printable_ascii(message) {
                    0
                } else if OffchainMessage::is_utf8(message) {
                    1
                } else {
                    return Err(SanitizeError::InvalidValue);
                }
            } else if message.len() <= OffchainMessage::MAX_LEN {
                if OffchainMessage::is_utf8(message) {
                    2
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
        pub fn serialize(&self) -> Result<Vec<u8>, SanitizeError> {
            // invalid messages shouldn't be possible, but a quick sanity check never hurts
            assert!(
                self.format <= 2 && !self.message.is_empty() && self.message.len() <= Self::MAX_LEN
            );
            let mut res: Vec<u8> = vec![0; Self::HEADER_LEN + self.message.len()];
            let domain_len = Base::SIGNING_DOMAIN.len();
            // signing domain
            res[..domain_len].copy_from_slice(Base::SIGNING_DOMAIN);
            // version
            res[domain_len] = 0;
            // format
            res[domain_len + 1] = self.format;
            // message length
            res[(domain_len + 2)..(domain_len + 4)]
                .copy_from_slice(&(self.message.len() as u16).to_le_bytes());
            // message
            res[(domain_len + 4)..].copy_from_slice(&self.message);
            Ok(res)
        }

        /// Deserialize the message from bytes that include a full header
        pub fn deserialize(data: &[u8]) -> Result<Self, SanitizeError> {
            // validate data length
            if data.len() <= Self::HEADER_LEN || data.len() > Self::HEADER_LEN + Self::MAX_LEN {
                return Err(SanitizeError::ValueOutOfBounds);
            }
            // decode header
            let domain_len = Base::SIGNING_DOMAIN.len();
            let version = data[domain_len];
            let format = data[domain_len + 1];
            let message_len =
                u16::from_le_bytes([data[domain_len + 2], data[domain_len + 3]]) as usize;
            // check header
            if version != 0 || format > 2 || message_len + Self::HEADER_LEN != data.len() {
                return Err(SanitizeError::InvalidValue);
            }
            let message = &data[(domain_len + 4)..];
            // check format
            let is_valid = match format {
                0 => message.len() <= Self::MAX_LEN_LEDGER && Self::is_printable_ascii(message),
                1 => message.len() <= Self::MAX_LEN_LEDGER && Self::is_utf8(message),
                2 => message.len() <= Self::MAX_LEN && Self::is_utf8(message),
                _ => false,
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

        /// Compute the SHA256 hash of the off-chain message
        pub fn hash(&self) -> Result<Hash, SanitizeError> {
            let mut hasher = Hasher::default();
            hasher.hash(&self.serialize()?);
            Ok(hasher.result())
        }

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

        pub fn get_format(&self) -> u8 {
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
        match self {
            Self::V0(msg) => msg.serialize(),
        }
    }

    /// Deserialize the off-chain message from bytes that include full header
    pub fn deserialize(data: &[u8]) -> Result<Self, SanitizeError> {
        if data.len() < Self::HEADER_LEN {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        let version = data[Self::SIGNING_DOMAIN.len()];
        match version {
            0 => Ok(Self::V0(v0::OffchainMessage::deserialize(data)?)),
            _ => Err(SanitizeError::ValueOutOfBounds),
        }
    }

    /// Compute the hash of the off-chain message
    pub fn hash(&self) -> Result<Hash, SanitizeError> {
        match self {
            Self::V0(msg) => msg.hash(),
        }
    }

    pub fn get_version(&self) -> u8 {
        match self {
            Self::V0(_) => 0,
        }
    }

    pub fn get_format(&self) -> u32 {
        match self {
            Self::V0(msg) => msg.get_format() as u32,
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
        assert_eq!(message.get_format(), 0);
        assert_eq!(message.get_message().as_slice(), b"Test Message");
        assert!(matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == 0));
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
        assert_eq!(message.get_format(), 1);
        assert_eq!(
            message.get_message().as_slice(),
            "Тестовое сообщение".as_bytes()
        );
        assert!(matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == 1));
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
