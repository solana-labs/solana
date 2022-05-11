use {
    super::VersionedTransaction,
    crate::{sanitize::SanitizeError, signature::Signature},
    solana_program::message::SanitizedVersionedMessage,
};

/// Wraps a sanitized `VersionedTransaction` to provide a safe API
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanitizedVersionedTransaction {
    /// List of signatures
    pub(crate) signatures: Vec<Signature>,
    /// Message to sign.
    pub(crate) message: SanitizedVersionedMessage,
}

impl TryFrom<VersionedTransaction> for SanitizedVersionedTransaction {
    type Error = SanitizeError;
    fn try_from(tx: VersionedTransaction) -> Result<Self, Self::Error> {
        Self::try_new(tx)
    }
}

impl SanitizedVersionedTransaction {
    pub fn try_new(tx: VersionedTransaction) -> Result<Self, SanitizeError> {
        tx.sanitize_signatures()?;
        Ok(Self {
            signatures: tx.signatures,
            message: SanitizedVersionedMessage::try_from(tx.message)?,
        })
    }

    pub fn get_message(&self) -> &SanitizedVersionedMessage {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program::{
            message::{Message, VersionedMessage},
            pubkey::Pubkey,
        },
    };

    #[test]
    fn test_try_new_with_invalid_signatures() {
        let tx = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message::new(&[], Some(&Pubkey::new_unique()))),
        };

        assert_eq!(
            SanitizedVersionedTransaction::try_new(tx),
            Err(SanitizeError::IndexOutOfBounds)
        );
    }

    #[test]
    fn test_try_new_with_invalid_header() {
        let mut message = Message::new(&[], Some(&Pubkey::new_unique()));
        message.header.num_readonly_signed_accounts += 1;

        let tx = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::Legacy(message),
        };

        assert_eq!(
            SanitizedVersionedTransaction::try_new(tx),
            Err(SanitizeError::IndexOutOfBounds)
        );
    }
}
