//! Off-chain message container for storing non-transaction messages.

#![cfg(feature = "full")]

use std::str::FromStr;

use thiserror::Error;

use {
    crate::{
        hash::Hash,
        pubkey::Pubkey,
        signature::Signature,
        signer::{Signer, SignerError},
    },
    num_enum::{IntoPrimitive, TryFromPrimitive}
};
use solana_program::sanitize::SanitizeError;

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
    IllegalMessageFormat,
    #[error(transparent)]
    SignerError(#[from] SignerError),
    #[error("at least one signer required")]
    SignerRequired,
    #[error("too many signers")]
    TooManySigners,
    #[error("invalid signing domain")]
    InvalidSigningDomain,
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone, TryFromPrimitive, IntoPrimitive)]
pub enum MessageFormat {
    RestrictedAscii,
    LimitedUtf8,
    ExtendedUtf8,
}

impl MessageFormat {
    const LEN: usize = std::mem::size_of::<Self>();
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
pub enum Version {
    V0 = 0,
}

impl Version {
    const LEN: usize = std::mem::size_of::<Self>();
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApplicationDomain([u8; Self::LEN]);

impl ApplicationDomain {
    const LEN: usize = 32;
    pub fn new(domain: [u8; Self::LEN]) -> Self {
        Self(domain)
    }

    ///Return current values
    pub fn buffer(&self) -> [u8; Self::LEN] {
        self.0
    }

    pub fn from_slice(domain: &[u8]) -> Self {
        let mut application_domain = Self::default();
        application_domain.0.copy_from_slice(domain);
        application_domain
    }
}


#[allow(clippy::arithmetic_side_effects)]
pub mod v0 {
    use num_enum::TryFromPrimitive;

    use {
        crate::{
            hash::{Hash, Hasher},
            packet::PACKET_DATA_SIZE,
            pubkey::PUBKEY_BYTES,
        },
        super::MessageFormat,
    };
    use solana_program::pubkey::Pubkey;
    use solana_program::sanitize::SanitizeError;
    use solana_sdk::offchain_message::{ApplicationDomain, Version};
    use solana_sdk::string_utils::{is_printable_ascii, is_utf8};

    use crate::offchain_message::Error;

    pub const SIGNING_DOMAIN: &[u8; 16] = b"\xffsolana offchain";

    /// OffchainMessage Version 0.
    /// Struct always contains a non-empty valid message.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct OffchainMessage {
        //Max 16 bytes
        signing_domain: [u8; 16],
        header_version: Version,
        //Max 32 bytes - can be arbitrary data
        application_domain: ApplicationDomain,
        format: MessageFormat,
        signer_count: u8,
        signers: Vec<Pubkey>,
        message: Vec<u8>,
    }

    pub(crate) type MessageLen = u16;
    pub(crate) type SignersLen = u8;

    impl OffchainMessage {
        /// Construct a new OffchainMessage object from the given message
        pub fn new(message: &[u8], signers: Vec<Pubkey>, application_domain: ApplicationDomain) -> Result<Self, Error> {
            let format = if message.is_empty() {
                return Err(Error::MessageEmpty);
            } else if message.len() <= Self::max_message_for_ledger(&signers) {
                if is_printable_ascii(message) {
                    MessageFormat::RestrictedAscii
                } else if is_utf8(message) {
                    MessageFormat::LimitedUtf8
                } else {
                    return Err(Error::UnexpectedMessageEncoding);
                }
            } else if message.len() <= Self::max_message(&signers) {
                if is_utf8(message) {
                    MessageFormat::ExtendedUtf8
                } else {
                    return Err(Error::UnexpectedMessageEncoding);
                }
            } else if message.len() <= Self::max_message(&signers) {
                MessageFormat::ExtendedUtf8
            } else {
                return Err(Error::MessageTooLong);
            };

            if signers.is_empty() {
                return Err(Error::SignerRequired);
            }
            let _ = SignersLen::try_from(signers.len()).map_err(|_| Error::TooManySigners)?;

            Ok(Self {
                signing_domain: *SIGNING_DOMAIN,
                header_version: Version::V0,//This implementation is defined for V0 only
                application_domain,
                format,
                signer_count: signers.len() as u8,
                signers,
                message: message.to_vec(),
            })
        }

        /// Signing domain (16 bytes) + Header version (1 byte) + Application domain (32 bytes) +
        /// + Message format (1 byte) + Signer count (1 bytes) + Signers (signer_count *  32 bytes) +
        /// + Message length (2 bytes)
        /// = 85 bytes
        /// Current cli implementation uses single signer - header size can be estimated up front
        pub const fn header_length(signers: &[Pubkey]) -> usize {
            ApplicationDomain::LEN +
                MessageFormat::LEN +
                std::mem::size_of::<SignersLen>() +
                signers.len() * std::mem::size_of::<Pubkey>() +
                std::mem::size_of::<MessageLen>()
        }

        pub const fn min_header_length() -> usize {
            let signer = Pubkey::new_from_array([0u8; PUBKEY_BYTES]);
            Self::header_length(std::slice::from_ref(&signer))
        }

        // Max Length of the input message supported by the Ledger
        pub fn max_message_for_ledger(signers: &[Pubkey]) -> usize {
            PACKET_DATA_SIZE.saturating_sub(Self::header_length(signers))
        }

        // Max length of the input message
        pub fn max_message(signers: &[Pubkey]) -> usize {
            usize::from(MessageLen::MAX).saturating_sub(Self::header_length(signers))
        }

        pub fn max_message_with_header() -> usize {
            usize::from(MessageLen::MAX)
        }

        /// Serialize the message to bytes, including the full header
        /// Message preamble data order:
        /// 1. Signing domain (16 bytes)
        /// 2. Header version (1 byte)
        /// 3. Application domain (32 bytes)
        /// 4. Message format (1 byte)
        /// 5. Signer count (1 bytes)
        /// 6. Signers (signer_count *  32 bytes)
        /// 7. Message length (2 bytes)
        pub fn serialize(&self, data: &mut Vec<u8>) -> Result<(), SanitizeError> {
            // invalid messages shouldn't be possible, but a quick sanity check never hurts
            assert!(!self.message.is_empty() && self.message.len() <= Self::max_message(self.get_signers()));

            //Reserve space for header + message
            data.reserve(
                Self::header_length(self.get_signers()).saturating_add(self.message.len()),
            );

            //Signing domain
            data.append(SIGNING_DOMAIN.to_vec().as_mut());

            //Header version
            data.push(0);

            //Application domain
            data.extend_from_slice(&self.application_domain.buffer());

            //Message format
            data.push(self.format.into());

            // Signer count
            data.push(self.signer_count);

            // Signers
            for signer in self.signers.iter() {
                data.extend_from_slice(signer.as_ref());
            }

            //Message length
            data.extend_from_slice(&(self.message.len() as u16).to_le_bytes());

            //Message
            data.extend_from_slice(&self.message);

            Ok(())
        }

        /// Deserialize the message from bytes that include a full header
        pub fn deserialize(data: &[u8]) -> Result<Self, Error> {
            // validate data length
            if data.len() <= Self::min_header_length() || data.len() > Self::max_message_with_header() {
                return Err(Error::BufferTooShort);
            }
            //We know that header is at least 85 bytes long and message is not empty - using raw indexes should be safe

            // decode header
            let mut offset = 0;

            let signing_domain_range = offset..SIGNING_DOMAIN.len();
            let signing_domain: [u8; 16] = data[signing_domain_range].try_into().unwrap();

            if signing_domain != *SIGNING_DOMAIN {
                return Err(Error::InvalidSigningDomain);
            }

            offset += SIGNING_DOMAIN.len();

            if data[offset..].len() < Version::LEN {
                return Err(Error::BufferTooShort);
            }

            let header_version: u8 = data[offset];
            let header_version = Version::try_from_primitive(header_version)
                .map_err(|e| Error::UnexpectedVersionDiscriminant(e.number))?;
            offset += 1;

            let mut application_domain = ApplicationDomain::default();
            let application_domain_range = offset..offset + ApplicationDomain::LEN;
            application_domain.0.copy_from_slice(&data[application_domain_range]);
            offset += ApplicationDomain::LEN;

            let format = MessageFormat::try_from(data[offset])
                .map_err(|e| Error::UnexpectedFormatDiscriminant(e.number))?;
            offset += 1;

            let num_signers = usize::from(data[offset]);
            offset += 1;
            if num_signers == 0 {
                return Err(Error::SignerRequired);
            }
            if data[offset..].len() < num_signers * PUBKEY_BYTES {
                return Err(Error::BufferTooShort);
            }

            let mut signers = Vec::with_capacity(num_signers);
            let signer_count: u8 = num_signers.try_into().unwrap();
            for _ in 0..num_signers {
                let signer_range = offset..offset + PUBKEY_BYTES;
                offset += PUBKEY_BYTES;
                let signer = Pubkey::try_from(&data[signer_range])
                    .expect("buffer contains enough data to represent all signers");
                signers.push(signer);
            }

            const MESSAGE_LEN_LEN: usize = std::mem::size_of::<MessageLen>();
            if data[offset..].len() < MESSAGE_LEN_LEN {
                return Err(Error::BufferTooShort);
            }
            let message_len_range = offset..offset + MESSAGE_LEN_LEN;
            let message_len_bytes =
                <[u8; MESSAGE_LEN_LEN]>::try_from(&data[message_len_range]).unwrap();
            let message_len = usize::from(MessageLen::from_le_bytes(message_len_bytes));
            offset += MESSAGE_LEN_LEN;

            let message = &data[offset..];
            let expected_remaining_buffer = message_len;
            let actual_remaining_buffer = message.len();
            println!("off: {offset}, exp: {expected_remaining_buffer}, act: {actual_remaining_buffer}");
            if actual_remaining_buffer > expected_remaining_buffer {
                return Err(Error::BufferTooLong);
            }
            println!("4");
            if actual_remaining_buffer < expected_remaining_buffer {
                return Err(Error::BufferTooShort);
            }

            // check format
            let is_valid = match format {
                MessageFormat::RestrictedAscii => {
                    (message.len() <= Self::max_message_for_ledger(&signers)) && is_printable_ascii(message)
                }
                MessageFormat::LimitedUtf8 => {
                    (message.len() <= Self::max_message_for_ledger(&signers)) && is_utf8(message)
                }
                MessageFormat::ExtendedUtf8 => (message.len() <= Self::max_message(&signers)) && is_utf8(message),
            };

            if is_valid {
                Ok(Self {
                    signing_domain,
                    header_version,
                    application_domain,
                    format,
                    signer_count,
                    signers,
                    message: message.to_vec(),
                })
            } else {
                Err(Error::IllegalMessageFormat)
            }

        }

        /// Compute the SHA256 hash of the serialized off-chain message
        pub fn hash(serialized_message: &[u8]) -> Result<Hash, Error> {
            let mut hasher = Hasher::default();
            hasher.hash(serialized_message);
            Ok(hasher.result())
        }

        pub fn get_application_domain(&self) -> &ApplicationDomain {
            &self.application_domain
        }

        pub fn get_format(&self) -> MessageFormat {
            self.format
        }

        pub fn get_signers(&self) -> &[Pubkey] {
            &self.signers
        }

        pub fn get_message(&self) -> &Vec<u8> {
            &self.message
        }

    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn test_header_length() {
            assert_eq!(
                OffchainMessage::min_header_length(),
                68,
            )
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum OffchainMessage {
    V0(v0::OffchainMessage),
}

impl OffchainMessage {
    /// Construct a new OffchainMessage object from the given version and message
    pub fn new(version: Version, message: &[u8], signers: Vec<Pubkey>, application_domain: ApplicationDomain) -> Result<Self, Error> {
        match version {
            Version::V0 => Ok(Self::V0(v0::OffchainMessage::new(message, signers, application_domain)?)),
        }
    }

    /// Serialize the off-chain message to bytes including full header
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        let mut data = Vec::new();

        // serialize version and call version specific serializer
        match self {
            Self::V0(msg) => {
                msg.serialize(&mut data).unwrap();
            }
        }
        Ok(data)
    }

    /// Deserialize the off-chain message from bytes that include full header
    /// Used only in tests
    pub fn deserialize(version: Version, data: &[u8]) -> Result<Self, Error> {
        match version {
            Version::V0 => Ok(Self::V0(v0::OffchainMessage::deserialize(data)?)),
        }
    }

    /// Compute the hash of the off-chain message
    pub fn hash(&self) -> Result<Hash, Error> {
        match self {
            // Hash input message contents
            Self::V0(_) => v0::OffchainMessage::hash(self.get_message()),
        }
    }

    pub fn get_version(&self) -> u8 {
        match self {
            Self::V0(_) => 0,
        }
    }

    pub fn get_application_domain(&self) -> &ApplicationDomain {
        match self {
            Self::V0(msg) => msg.get_application_domain(),
        }
    }

    pub fn get_format(&self) -> MessageFormat {
        match self {
            Self::V0(msg) => msg.get_format(),
        }
    }

    pub fn get_signers(&self) -> &[Pubkey] {
        match self {
            Self::V0(msg) => msg.get_signers(),
        }
    }

    pub fn get_message(&self) -> &Vec<u8> {
        match self {
            Self::V0(msg) => msg.get_message(),
        }
    }

    /// Sign the message with provided keypair
    pub fn sign(&self, signer: &dyn Signer) -> Result<Signature, SanitizeError> {
        Ok(signer.sign_message(&self.serialize().unwrap()))
    }

    /// Verify that the message signature is valid for the given public key
    pub fn verify(&self, signer: &Pubkey, signature: &Signature) -> Result<bool, SanitizeError> {
        Ok(signature.verify(signer.as_ref(), &self.serialize().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use {crate::signature::Keypair, std::str::FromStr, super::*};

    #[test]
    fn test_offchain_message_extended_utf8() {
        let signers = vec![Pubkey::default()];
        let text_message = "Ł".repeat(v0::OffchainMessage::max_message_for_ledger(&signers).saturating_add(1));
        let message = OffchainMessage::new(Version::V0, text_message.as_ref(), signers, ApplicationDomain::default()).unwrap();
        assert_eq!(message.get_format(), MessageFormat::ExtendedUtf8);
    }

    #[test]
    fn test_offchain_message_empty_message() {
        let message = OffchainMessage::new(Version::V0, b"", vec![Pubkey::default()], ApplicationDomain::default());
        assert!(matches!(message, Err(Error::MessageEmpty)));
    }

    #[test]
    fn test_offchain_message_too_long_utf8() {
        let signers = vec![Pubkey::default()];
        let msg_size = v0::OffchainMessage::max_message(&signers);
        let message = &vec![0_u8; msg_size.saturating_add(1)];
        let message = OffchainMessage::new(Version::V0, message, signers, ApplicationDomain::default());
        assert!(matches!(message, Err(Error::MessageTooLong)));
    }

    #[test]
    fn test_offchain_message_invalid_utf8() {
        let message = OffchainMessage::new(Version::V0, &[0xF1], vec![Pubkey::default()], ApplicationDomain::default());
        assert!(matches!(message, Err(Error::UnexpectedMessageEncoding)));
    }

    #[test]
    fn test_offchain_message_utf8_long_message() {
        let signers = vec![Pubkey::default()];
        let msg_size = v0::OffchainMessage::max_message_for_ledger(&signers);
        let message = &vec![0_u8; msg_size.saturating_add(1)];
        let message = OffchainMessage::new(Version::V0, message, signers, ApplicationDomain::default());

        assert_eq!(message.unwrap().get_format(), MessageFormat::ExtendedUtf8);
    }

    #[test]
    fn test_offchain_message_ascii() {
        let message = OffchainMessage::new(Version::V0, b"Test Message", vec![Pubkey::default()], ApplicationDomain::default()).unwrap();

        assert_eq!(message.get_version(), 0);
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message().as_slice(), b"Test Message");
        assert!(
            matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == MessageFormat::RestrictedAscii)
        );
        let serialized = [
            255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12,
            0, 84, 101, 115, 116, 32, 77, 101, 115, 115, 97, 103, 101
        ];
        let hash = Hash::from_str("DHMr3vK1BJ5RzfwAmjzkayGyEs3ouKUhHzZdYRG5migZ").unwrap();
        assert_eq!(message.serialize().unwrap(), serialized);
        assert_eq!(message.hash().unwrap(), hash);
        assert_eq!(message, OffchainMessage::deserialize(Version::V0, &serialized).unwrap());
    }

    #[test]
    fn test_offchain_message_utf8() {
        let message = OffchainMessage::new(Version::V0, "Тестовое сообщение".as_bytes(), vec![Pubkey::default()], ApplicationDomain::default()).unwrap();
        assert_eq!(message.get_version(), 0);
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), "Тестовое сообщение".as_bytes(),);
        assert!(
            matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == MessageFormat::LimitedUtf8)
        );
        let serialized = [
            255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35,
            0, 208, 162, 208, 181, 209, 129, 209, 130, 208, 190, 208, 178, 208, 190, 208, 181, 32, 209, 129,
            208, 190, 208, 190, 208, 177, 209, 137, 208, 181, 208, 189, 208, 184, 208, 181
        ];
        let hash = Hash::from_str("GTARP1PJqT7JoqnjN5TCJDzzoxB7MqtsBvA2b5hbNc7c").unwrap();
        assert_eq!(message.serialize().unwrap(), serialized);
        assert_eq!(message.hash().unwrap(), hash);
        assert_eq!(message, OffchainMessage::deserialize(Version::V0, &serialized).unwrap());
    }

    #[test]
    fn test_offchain_message_sign_and_verify() {
        let message = OffchainMessage::new(Version::V0, b"Test Message", vec![Pubkey::default()], ApplicationDomain::default()).unwrap();
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

    #[test]
    fn test_message_len_boundaries() {
        let signers = vec![Pubkey::from([1u8; 32])];
        let application_domain = ApplicationDomain::new([2u8; 32]);

        // empty message
        let empty_message_result = OffchainMessage::new(
            Version::V0,
            "".as_ref(),
            signers.clone(),
            application_domain.clone(),
        );
        assert_eq!(empty_message_result, Err(Error::MessageEmpty));

        let c_ascii = 'T'; // 0x54
        let c_utf8 = 'Т';  // 0xd0a2

        // min for Format::RestrictedAscii
        let min_message = [c_ascii].iter().collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            min_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message(), min_message.as_bytes());

        // min for Format::LimitedUtf8
        let min_message = [c_utf8].iter().collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            min_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), min_message.as_bytes());

        // min for Format::ExtendedUtf8
        let max_message_for_ledger = v0::OffchainMessage::max_message_for_ledger(&signers);
        let min_message = std::iter::repeat(c_ascii).take(max_message_for_ledger + 1).collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            min_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::ExtendedUtf8);
        assert_eq!(message.get_message(), min_message.as_bytes());

        // max for Format::LimitedAscii
        let max_message = std::iter::repeat(c_ascii).take(max_message_for_ledger).collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            max_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message(), max_message.as_bytes());

        // max for Format::RestrictedUtf8
        // one utf8 char to flip the format, then pad out to max with ascii chars
        let max_message = [c_utf8]
            .into_iter()
            .chain(std::iter::repeat(c_ascii).take(max_message_for_ledger - c_utf8.len_utf8()))
            .collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            max_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), max_message.as_bytes());

        // max for Format::ExtendedUtf8
        let max_message_len = v0::OffchainMessage::max_message(&signers);
        let max_message = std::iter::repeat(c_ascii).take(max_message_len).collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            max_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::ExtendedUtf8);
        assert_eq!(message.get_message(), max_message.as_bytes());

        // overflow
        let overflow_message = std::iter::repeat(c_ascii).take(max_message_len + 1).collect::<String>();
        let message_result = OffchainMessage::new(
            Version::V0,
            overflow_message.as_ref(),
            signers.clone(),
            application_domain.clone(),
        );
        assert_eq!(message_result, Err(Error::MessageTooLong));
    }

    #[test]
    fn test_signers_len_boundaries() {
        let application_domain = ApplicationDomain::new([2u8; 32]);

        // empty signers
        let empty_message_result = OffchainMessage::new(
            Version::V0,
            "test message".as_ref(),
            Vec::new(),
            application_domain.clone(),
        );
        assert_eq!(empty_message_result, Err(Error::SignerRequired));

        // min signers
        let min_signers = vec![Pubkey::new_unique()];
        let message = OffchainMessage::new(
            Version::V0,
            "test message".as_ref(),
            min_signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_signers(), min_signers);

        // max signers
        let max_signers_len = usize::from(u8::MAX);
        let max_signers = std::iter::repeat_with(Pubkey::new_unique).take(max_signers_len).collect::<Vec<_>>();
        let message = OffchainMessage::new(
            Version::V0,
            "test message".as_ref(),
            max_signers.clone(),
            application_domain.clone(),
        ).unwrap();
        assert_eq!(message.get_signers(), max_signers);

        // overflow
        let overflow_signers = std::iter::repeat_with(Pubkey::new_unique).take(max_signers_len + 1).collect::<Vec<_>>();
        let message_result = OffchainMessage::new(
            Version::V0,
            "test message".as_ref(),
            overflow_signers.clone(),
            application_domain.clone(),
        );
        assert_eq!(message_result, Err(Error::TooManySigners));
    }

    #[test]
    fn test_deserialize_bounds() {
        let test_serialized: Vec<u8> = vec![255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 0, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 12, 0, 116, 101, 115, 116, 32, 109, 101, 115, 115, 97, 103, 101];

        // assert test bytes inputs
        let signers = vec![Pubkey::from([1u8; 32]), Pubkey::from([2u8; 32])];
        let application_domain = ApplicationDomain::new([3u8; 32]);
        let message = OffchainMessage::new(
            Version::V0,
            "test message".as_ref(),
            signers,
            application_domain.clone(),
        ).unwrap();
        assert_eq!(test_serialized, message.serialize().unwrap());
        assert_eq!(OffchainMessage::deserialize(Version::V0, &test_serialized), Ok(message));

        let mut next_field_offset = v0::SIGNING_DOMAIN.len();

        // too short for signing domain
        let data = &test_serialized[..next_field_offset - 1];
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, data),
            Err(Error::BufferTooShort),
        );

        // wrong signing domain
        let mut data_cpy = test_serialized.clone();
        data_cpy[1] += 1;
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, &data_cpy),
            Err(Error::InvalidSigningDomain),
        );

        // too short for version
        let data = &test_serialized[..next_field_offset];
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, data),
            Err(Error::BufferTooShort),
        );

        // wrong version
        let mut data_cpy = test_serialized.clone();
        data_cpy[v0::SIGNING_DOMAIN.len()] = 21;//Set invalid version
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, &data_cpy),
            Err(Error::UnexpectedVersionDiscriminant(21)),
        );

        // too short for v0 header
        let data = &test_serialized[..v0::OffchainMessage::min_header_length() - 1];
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, data),
            Err(Error::BufferTooShort),
        );

        next_field_offset += ApplicationDomain::LEN;

        // invalid v0 message format
        assert_eq!(MessageFormat::LEN, std::mem::size_of::<u8>());
        let mut data_cpy = test_serialized.clone();
        data_cpy[v0::SIGNING_DOMAIN.len() + Version::LEN + ApplicationDomain::LEN] = u8::MAX;
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, &data_cpy),
            Err(Error::UnexpectedFormatDiscriminant(u8::MAX))
        );

        next_field_offset += MessageFormat::LEN;

        // empty v0 signers
        let mut data_cpy = test_serialized.clone();
        data_cpy[v0::SIGNING_DOMAIN.len() + Version::LEN + ApplicationDomain::LEN + MessageFormat::LEN] = 0;
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, &data_cpy),
            Err(Error::SignerRequired),
        );

        // v0 too short for all signers
        let mut data_cpy = test_serialized.clone();
        data_cpy[v0::SIGNING_DOMAIN.len() + Version::LEN + ApplicationDomain::LEN + MessageFormat::LEN] = 10;
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, &data_cpy),
            Err(Error::BufferTooShort),
        );

        // v0 too short for message len
        let data = &test_serialized[..next_field_offset - 1];
        assert_eq!(
            OffchainMessage::deserialize(Version::V0, data),
            Err(Error::BufferTooShort),
        );
    }

    #[test]
    fn test_application_domain_from_slice_empty_buffer(){
        let input_params = [0_u8; 32];

        let app_domain: ApplicationDomain = ApplicationDomain::from_slice(&input_params);

        assert_eq!(app_domain.buffer(), input_params);
    }

    #[test]
    fn test_application_domain_from_slice_key(){
        let input_params_base58: &str = "9tjwwvfJssteUgJXs6uj6WYCXABCfcwhtzhbeXrhgBzc";
        let input_params_decoded = bs58::decode(input_params_base58).into_vec().unwrap();

        let app_domain = ApplicationDomain::from_slice(input_params_decoded.as_ref());

        assert_eq!(app_domain.buffer(), input_params_decoded.as_ref());
    }

    #[test]
    fn test_application_domain_invalid_utf8_message(){
        let signers = vec![Pubkey::from([1u8; 32]), Pubkey::from([2u8; 32])];
        let application_domain = ApplicationDomain::new([3u8; 32]);

        //Generate invalid utf8 message
        let mut message_data = Vec::from([0xc3, 0x28]);
        message_data.resize(v0::OffchainMessage::max_message(&signers) - 10, 0);


        let message_result = OffchainMessage::new(
            Version::V0,
            message_data.as_ref(),
            signers,
            application_domain.clone(),
        );

        assert_eq!(message_result, Err(Error::UnexpectedMessageEncoding));

    }

}
