use {
    crate::{
        bytes::read_byte,
        result::{Result, TransactionParsingError},
    },
    solana_sdk::message::MESSAGE_VERSION_PREFIX,
};

/// A byte that represents the version of the transaction.
#[derive(Copy, Clone, Default)]
#[repr(u8)]
pub enum TransactionVersion {
    #[default]
    Legacy = u8::MAX,
    V0 = 0,
}

/// Meta data for accessing message header fields in a transaction view.
pub(crate) struct MessageHeaderMeta {
    /// The offset to the first byte of the message in the transaction packet.
    pub(crate) offset: u16,
    /// The version of the transaction.
    pub(crate) version: TransactionVersion,
    /// The number of signatures required for this message to be considered
    /// valid.
    pub(crate) num_required_signatures: u8,
    /// The last `num_readonly_signed_accounts` of the signed keys are
    /// read-only.
    pub(crate) num_readonly_signed_accounts: u8,
    /// The last `num_readonly_unsigned_accounts` of the unsigned keys are
    /// read-only accounts.
    pub(crate) num_readonly_unsigned_accounts: u8,
}

impl MessageHeaderMeta {
    #[inline(always)]
    pub fn try_new(bytes: &[u8], offset: &mut usize) -> Result<Self> {
        // Get the message offset.
        // We know the offset does not exceed packet length, and our packet
        // length is less than u16::MAX, so we can safely cast to u16.
        let message_offset = *offset as u16;

        // Read the message prefix byte if present. This byte is present in V0
        // transactions but not in legacy transactions.
        // The message header begins immediately after the message prefix byte
        // if present.
        let message_prefix = read_byte(bytes, offset)?;
        let (version, num_required_signatures) = if message_prefix & MESSAGE_VERSION_PREFIX != 0 {
            let version = message_prefix & !MESSAGE_VERSION_PREFIX;
            match version {
                0 => (TransactionVersion::V0, read_byte(bytes, offset)?),
                _ => return Err(TransactionParsingError),
            }
        } else {
            // Legacy transaction. The `message_prefix` that was just read is
            // actually the number of required signatures.
            (TransactionVersion::Legacy, message_prefix)
        };

        let num_readonly_signed_accounts = read_byte(bytes, offset)?;
        let num_readonly_unsigned_accounts = read_byte(bytes, offset)?;

        Ok(Self {
            offset: message_offset,
            version,
            num_required_signatures,
            num_readonly_signed_accounts,
            num_readonly_unsigned_accounts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_version() {
        let bytes = [0b1000_0001];
        let mut offset = 0;
        assert!(MessageHeaderMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_legacy_transaction_missing_header_byte() {
        let bytes = [5, 0];
        let mut offset = 0;
        assert!(MessageHeaderMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_legacy_transaction_valid() {
        let bytes = [5, 1, 2];
        let mut offset = 0;
        let header = MessageHeaderMeta::try_new(&bytes, &mut offset).unwrap();
        assert!(matches!(header.version, TransactionVersion::Legacy));
        assert_eq!(header.num_required_signatures, 5);
        assert_eq!(header.num_readonly_signed_accounts, 1);
        assert_eq!(header.num_readonly_unsigned_accounts, 2);
    }

    #[test]
    fn test_v0_transaction_missing_header_byte() {
        let bytes = [MESSAGE_VERSION_PREFIX, 5, 1];
        let mut offset = 0;
        assert!(MessageHeaderMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_v0_transaction_valid() {
        let bytes = [MESSAGE_VERSION_PREFIX, 5, 1, 2];
        let mut offset = 0;
        let header = MessageHeaderMeta::try_new(&bytes, &mut offset).unwrap();
        assert!(matches!(header.version, TransactionVersion::V0));
        assert_eq!(header.num_required_signatures, 5);
        assert_eq!(header.num_readonly_signed_accounts, 1);
        assert_eq!(header.num_readonly_unsigned_accounts, 2);
    }
}
