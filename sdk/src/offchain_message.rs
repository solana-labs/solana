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
    std::{
        ops::{Deref, DerefMut},
        str::FromStr,
    },
    thiserror::Error,
};

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
    #[error("at least one signer required")]
    SignerRequired,
    #[error("too many signers")]
    TooManySigners,
    #[error("invalid signing domain")]
    InvalidSigningDomain,
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

    pub fn from_slice(domain: &[u8]) -> Self {
        let mut application_domain = Self::default();
        (*application_domain).copy_from_slice(domain);
        application_domain
    }
}

impl Deref for ApplicationDomain {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ApplicationDomain {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[allow(clippy::arithmetic_side_effects)]
pub mod v0 {
    use {
        super::*,
        crate::{
            hash::{Hash, Hasher},
            packet::PACKET_DATA_SIZE,
            pubkey::{Pubkey, PUBKEY_BYTES},
        },
    };

    /// OffchainMessage Version 0.
    /// Struct always contains a non-empty valid message.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct OffchainMessage {
        application_domain: ApplicationDomain,
        format: MessageFormat,
        signers: Vec<Pubkey>,
        message: String,
    }

    pub type MessageLen = u16;
    pub type SignersLen = u8;

    impl OffchainMessage {
        const fn header_length(signers: &[Pubkey]) -> usize {
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

        pub fn max_message_for_ledger(signers: &[Pubkey]) -> usize {
            PACKET_DATA_SIZE.saturating_sub(Self::header_length(signers))
        }

        pub fn max_message(signers: &[Pubkey]) -> usize {
            usize::from(MessageLen::MAX).saturating_sub(Self::header_length(signers))
        }

        /// Construct a new OffchainMessage object from the given message
        pub fn new(
            message: &str,
            application_domain: ApplicationDomain,
            signers: Vec<Pubkey>,
        ) -> Result<Self, Error> {
            let format = if message.is_empty() {
                return Err(Error::MessageEmpty);
            } else if message.len() <= Self::max_message_for_ledger(&signers) {
                if is_printable_ascii(message) {
                    MessageFormat::RestrictedAscii
                } else {
                    MessageFormat::LimitedUtf8
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
                format,
                message: message.to_string(),
                application_domain,
                signers,
            })
        }

        /// Serialize the message to bytes, including the full header
        pub fn serialize(&self, mut data: Vec<u8>) -> Result<Vec<u8>, Error> {
            data.reserve(
                Self::header_length(self.get_signers()).saturating_add(self.message.len()),
            );
            // application domain
            data.extend_from_slice(self.get_application_domain().deref());
            // format
            let format = <MessageFormat as TryFromPrimitive>::Primitive::from(self.format);
            data.extend_from_slice(std::slice::from_ref(&format));
            // signers length
            let short_signers_len = SignersLen::try_from(self.signers.len())
                .expect("`signers.len()` was asserted to fit in `u8` in `Self::new()`");
            data.extend_from_slice(std::slice::from_ref(&short_signers_len));
            // signers
            for signer in &self.signers {
                data.extend_from_slice(signer.as_ref());
            }
            // message length
            let message_length =
                MessageLen::try_from(self.message.len())
                    .expect("`message.len()` was asserted to fit in `u16` in `Self::new()`");
            data.extend_from_slice(&message_length.to_le_bytes());
            // message
            data.extend_from_slice(self.message.as_bytes());
            Ok(data)
        }

        /// Deserialize the message from bytes that include a full header
        pub fn deserialize(data: &[u8]) -> Result<Self, Error> {
            println!("{}  {:?}", data.len(), data);
            // validate data length
            // signing domain and version have already been consumed from `data`
            if data.len() < Self::min_header_length() {
                return Err(Error::BufferTooShort);
            }

            // decode header
            let mut offset = 0;

            let mut application_domain = ApplicationDomain::default();
            let application_domain_range = offset..offset + ApplicationDomain::LEN;
            (*application_domain).copy_from_slice(&data[application_domain_range]);
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

            // check header
            let message_bytes = &data[offset..];
            let expected_remaining_buffer = message_len;
            let actual_remaining_buffer = message_bytes.len();
            println!("off: {offset}, exp: {expected_remaining_buffer}, act: {actual_remaining_buffer}");
            if actual_remaining_buffer > expected_remaining_buffer {
                return Err(Error::BufferTooLong);
            }
            println!("4");
            if actual_remaining_buffer < expected_remaining_buffer {
                return Err(Error::BufferTooShort);
            }
            let message =
                std::str::from_utf8(message_bytes).map_err(|_| Error::UnexpectedMessageEncoding)?;
            // check format
            let max_message_for_ledger = Self::max_message_for_ledger(&signers);
            match format {
                MessageFormat::RestrictedAscii => {
                    (message.len() <= max_message_for_ledger) && is_printable_ascii(message)
                }
                MessageFormat::LimitedUtf8 => message.len() <= max_message_for_ledger,
                MessageFormat::ExtendedUtf8 => message.len() <= Self::max_message(&signers),
            }
            .then_some(Self {
                application_domain,
                format,
                signers,
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

        pub fn get_application_domain(&self) -> &ApplicationDomain {
            &self.application_domain
        }

        pub fn get_format(&self) -> MessageFormat {
            self.format
        }

        pub fn get_signers(&self) -> &[Pubkey] {
            &self.signers
        }

        pub fn get_message(&self) -> &str {
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
    pub const SIGNING_DOMAIN: &'static [u8] = b"\xffsolana offchain";

    pub fn max_message_for_ledger(version: Version, signers: &[Pubkey]) -> usize {
        match version {
            Version::V0 => v0::OffchainMessage::max_message_for_ledger(signers),
        }
    }

    pub fn max_message(version: Version, signers: &[Pubkey]) -> usize {
        match version {
            Version::V0 => v0::OffchainMessage::max_message(signers),
        }
    }

    pub const fn min_header_length(version: Version) -> usize {
        let versioned_min_header_length = match version {
            Version::V0 => v0::OffchainMessage::min_header_length()
        };
        Self::SIGNING_DOMAIN.len() +
            Version::LEN +
            versioned_min_header_length
    }

    /// Construct a new OffchainMessage object from the given version and message
    pub fn new(
        version: Version,
        message: &str,
        application_domain: ApplicationDomain,
        signers: Vec<Pubkey>,
    ) -> Result<Self, Error> {
        match version {
            Version::V0 => {
                let v0_msg = v0::OffchainMessage::new(message, application_domain, signers)?;
                Ok(Self::V0(v0_msg))
            }
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
                msg.serialize(data)
            }
        }
    }

    /// Deserialize the off-chain message from bytes that include full header
    pub fn deserialize(data: &[u8]) -> Result<Self, Error> {
        let mut offset = 0;

        if data[offset..].len() < Self::SIGNING_DOMAIN.len() {
            return Err(Error::BufferTooShort);
        }
        let version_start_offset = offset.saturating_add(Self::SIGNING_DOMAIN.len());
        let signing_domain_range = offset..version_start_offset;
        let signing_domain = &data[signing_domain_range];
        if signing_domain != Self::SIGNING_DOMAIN {
            return Err(Error::InvalidSigningDomain);
        }
        offset = version_start_offset;

        if data[offset..].len() < Version::LEN {
            return Err(Error::BufferTooShort);
        }
        let versioned_data_start_offset = offset.saturating_add(Version::LEN);
        let version_byte = data[offset];
        let version = Version::try_from_primitive(version_byte)
            .map_err(|e| Error::UnexpectedVersionDiscriminant(e.number))?;
        offset = versioned_data_start_offset;
        let data = &data[offset..];
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
            Self::V0(msg) => &msg.get_signers(),
        }
    }

    pub fn get_message(&self) -> &str {
        match self {
            Self::V0(msg) => msg.get_message(),
        }
    }

    /// Sign the message with provided keypair
    pub fn sign(&self, signer: &dyn Signer) -> Result<Signature, Error> {
        let bytes = self.serialize()?;
        println!("{bytes:#04X?}");
        signer.try_sign_message(&bytes)
            .map_err(|e| e.into())
    }

    /// Verify that the message signature is valid for the given public key
    pub fn verify(&self, signer: &Pubkey, signature: &Signature) -> Result<bool, Error> {
        Ok(signature.verify(signer.as_ref(), &self.serialize()?))
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::{pubkey::PUBKEY_BYTES, signature::Keypair}, std::str::FromStr};

    #[test]
    fn test_offchain_message_ascii() {
        let signers = vec![Pubkey::from([1u8; 32])];
        let application_domain = ApplicationDomain::new([2u8; 32]);

        let message =
            OffchainMessage::new(Version::V0, "Test Message", application_domain, signers).unwrap();
        assert_eq!(message.get_version(), Version::V0);
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message(), "Test Message");
        assert!(
            matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == MessageFormat::RestrictedAscii)
        );
        let serialized = [
            255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 2, 2, 2,
            2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 12, 0, 84, 101, 115, 116, 32, 77, 101, 115, 115, 97, 103, 101,
        ];
        let hash = Hash::from_str("596s6UHPiJyAUPQa4vgBkk9EJ3RC95narRhhE2h3Ghm6").unwrap();
        assert_eq!(message.serialize().unwrap(), serialized);
        assert_eq!(message.hash().unwrap(), hash);
        assert_eq!(message, OffchainMessage::deserialize(&serialized).unwrap());
    }

    #[test]
    fn test_offchain_message_utf8() {
        let signers = vec![Pubkey::from([1u8; 32])];
        let application_domain = ApplicationDomain::new([2u8; 32]);

        let message = OffchainMessage::new(
            Version::V0,
            "Тестовое сообщение",
            application_domain,
            signers,
        )
        .unwrap();
        assert_eq!(message.get_version(), Version::V0);
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), "Тестовое сообщение",);
        assert!(
            matches!(message, OffchainMessage::V0(ref msg) if msg.get_format() == MessageFormat::LimitedUtf8)
        );
        let serialized = [
            255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 2, 2, 2,
            2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 1, 35, 0, 208, 162, 208, 181, 209, 129, 209, 130, 208, 190, 208, 178, 208,
            190, 208, 181, 32, 209, 129, 208, 190, 208, 190, 208, 177, 209, 137, 208, 181, 208,
            189, 208, 184, 208, 181,
        ];
        let hash = Hash::from_str("B9J78FmsRtWgkCEyVGCLbNw5uiNkPkfjdZ6mKBrExtWH").unwrap();
        assert_eq!(message.serialize().unwrap(), serialized);
        assert_eq!(message.hash().unwrap(), hash);
        assert_eq!(message, OffchainMessage::deserialize(&serialized).unwrap());
    }

    #[test]
    fn test_offchain_message_sign_and_verify() {
        // re-ordering `Pubkey::new_unique()` calls will break `serialized` below
        let signers = vec![Pubkey::new_unique()];
        let application_domain = ApplicationDomain::new(Pubkey::new_unique().to_bytes());

        let message =
            OffchainMessage::new(Version::V0, "Test Message", application_domain, signers).unwrap();
        let keypair = Keypair::new();
        let signature = message.sign(&keypair).unwrap();
        assert!(message.verify(&keypair.pubkey(), &signature).unwrap());
    }

    #[test]
    fn test_message_len_boundaries() {
        let signers = vec![Pubkey::from([1u8; 32])];
        let application_domain = ApplicationDomain::new([2u8; 32]);

        // empty message
        let empty_message_result = OffchainMessage::new(
            Version::V0,
            "",
            application_domain.clone(),
            signers.clone(),
        );
        assert_eq!(empty_message_result, Err(Error::MessageEmpty));

        let c_ascii = 'T'; // 0x54
        let c_utf8 = 'Т';  // 0xd0a2

        // min for Format::RestrictedAscii
        let min_message = [c_ascii].iter().collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            &min_message,
            application_domain.clone(),
            signers.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message(), min_message);

        // min for Format::LimitedUtf8
        let min_message = [c_utf8].iter().collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            &min_message,
            application_domain.clone(),
            signers.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), min_message);

        // min for Format::ExtendedUtf8
        let max_message_for_ledger = OffchainMessage::max_message_for_ledger(Version::V0, &signers);
        let min_message = std::iter::repeat(c_ascii).take(max_message_for_ledger + 1).collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            &min_message,
            application_domain.clone(),
            signers.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::ExtendedUtf8);
        assert_eq!(message.get_message(), min_message);

        // max for Format::LimitedAscii
        let max_message = std::iter::repeat(c_ascii).take(max_message_for_ledger).collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            &max_message,
            application_domain.clone(),
            signers.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::RestrictedAscii);
        assert_eq!(message.get_message(), max_message);

        // max for Format::RestrictedUtf8
        // one utf8 char to flip the format, then pad out to max with ascii chars
        let max_message = [c_utf8]
            .into_iter()
            .chain(std::iter::repeat(c_ascii).take(max_message_for_ledger - c_utf8.len_utf8()))
            .collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            &max_message,
            application_domain.clone(),
            signers.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::LimitedUtf8);
        assert_eq!(message.get_message(), max_message);

        // max for Format::ExtendedUtf8
        let max_message_len = OffchainMessage::max_message(Version::V0, &signers);
        let max_message = std::iter::repeat(c_ascii).take(max_message_len).collect::<String>();
        let message = OffchainMessage::new(
            Version::V0,
            &max_message,
            application_domain.clone(),
            signers.clone(),
        ).unwrap();
        assert_eq!(message.get_format(), MessageFormat::ExtendedUtf8);
        assert_eq!(message.get_message(), max_message);

        // overflow
        let overflow_message = std::iter::repeat(c_ascii).take(max_message_len + 1).collect::<String>();
        let message_result = OffchainMessage::new(
            Version::V0,
            &overflow_message,
            application_domain.clone(),
            signers.clone(),
        );
        assert_eq!(message_result, Err(Error::MessageTooLong));
    }

    #[test]
    fn test_signers_len_boundaries() {
        let application_domain = ApplicationDomain::new([2u8; 32]);

        // empty signers
        let empty_message_result = OffchainMessage::new(
            Version::V0,
            "test message",
            application_domain.clone(),
            Vec::new(),
        );
        assert_eq!(empty_message_result, Err(Error::SignerRequired));

        // min signers
        let min_signers = vec![Pubkey::new_unique()];
        let message = OffchainMessage::new(
            Version::V0,
            "test message",
            application_domain.clone(),
            min_signers.clone(),
        ).unwrap();
        assert_eq!(message.get_signers(), min_signers);

        // max signers
        let max_signers_len = usize::from(u8::MAX);
        let max_signers = std::iter::repeat_with(Pubkey::new_unique).take(max_signers_len).collect::<Vec<_>>();
        let message = OffchainMessage::new(
            Version::V0,
            "test message",
            application_domain.clone(),
            max_signers.clone(),
        ).unwrap();
        assert_eq!(message.get_signers(), max_signers);

        // overflow
        let overflow_signers = std::iter::repeat_with(Pubkey::new_unique).take(max_signers_len + 1).collect::<Vec<_>>();
        let message_result = OffchainMessage::new(
            Version::V0,
            "test message",
            application_domain.clone(),
            overflow_signers.clone(),
        );
        assert_eq!(message_result, Err(Error::TooManySigners));
    }

    #[test]
    fn test_deserialize_bounds() {
        let mut test_serialized: Vec<u8> = vec![255, 115, 111, 108, 97, 110, 97, 32, 111, 102, 102, 99, 104, 97, 105, 110, 0, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 0, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 12, 0, 116, 101, 115, 116, 32, 109, 101, 115, 115, 97, 103, 101];

        // assert test bytes inputs
        let signers = vec![Pubkey::from([1u8; 32]), Pubkey::from([2u8; 32])];
        let num_signers = signers.len();
        let application_domain = ApplicationDomain::new([3u8; 32]);
        let message = OffchainMessage::new(
            Version::V0,
            "test message",
            application_domain.clone(),
            signers,
        ).unwrap();
        assert_eq!(test_serialized, message.serialize().unwrap());
        assert_eq!(OffchainMessage::deserialize(&test_serialized), Ok(message));

        let mut next_field_offset = OffchainMessage::SIGNING_DOMAIN.len();

        // too short for signing domain
        let data = &test_serialized[..next_field_offset - 1];
        assert_eq!(
            OffchainMessage::deserialize(data),
            Err(Error::BufferTooShort),
        );

        // wrong signing domain
        let mut data = test_serialized[..next_field_offset].to_vec();
        data[1] += 1;
        assert_eq!(
            OffchainMessage::deserialize(&data),
            Err(Error::InvalidSigningDomain),
        );

        // too short for version
        let data = &test_serialized[..next_field_offset];
        assert_eq!(
            OffchainMessage::deserialize(data),
            Err(Error::BufferTooShort),
        );

        next_field_offset += Version::LEN;

        // wrong version
        let mut data = test_serialized[..next_field_offset].to_vec();
        let corrupt_version = data.last().unwrap() + 1;
        *data.last_mut().unwrap() = corrupt_version;
        assert_eq!(
            OffchainMessage::deserialize(&data),
            Err(Error::UnexpectedVersionDiscriminant(corrupt_version)),
        );

        // too short for v0 header
        let data = &test_serialized[..OffchainMessage::min_header_length(Version::V0) - 1];
        assert_eq!(
            OffchainMessage::deserialize(data),
            Err(Error::BufferTooShort),
        );

        next_field_offset += ApplicationDomain::LEN;

        // invalid v0 message format
        assert_eq!(MessageFormat::LEN, std::mem::size_of::<u8>());
        let mut data = test_serialized[..OffchainMessage::min_header_length(Version::V0)].to_vec();
        *data.get_mut(next_field_offset).unwrap() = u8::MAX;
        assert_eq!(
            OffchainMessage::deserialize(&data),
            Err(Error::UnexpectedFormatDiscriminant(u8::MAX))
        );

        next_field_offset += MessageFormat::LEN;

        // empty v0 signers
        let mut data = test_serialized[..OffchainMessage::min_header_length(Version::V0)].to_vec();
        *data.get_mut(next_field_offset).unwrap() = 0;
        assert_eq!(
            OffchainMessage::deserialize(&data),
            Err(Error::SignerRequired),
        );

        next_field_offset += std::mem::size_of::<v0::SignersLen>();
        next_field_offset += num_signers * PUBKEY_BYTES;

        // v0 too short for all signers
        let data = &test_serialized[..next_field_offset - 1];
        assert_eq!(
            OffchainMessage::deserialize(data),
            Err(Error::BufferTooShort),
        );

        next_field_offset += std::mem::size_of::<v0::MessageLen>();

        // v0 too short for message len
        let data = &test_serialized[..next_field_offset - 1];
        assert_eq!(
            OffchainMessage::deserialize(data),
            Err(Error::BufferTooShort),
        );


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
