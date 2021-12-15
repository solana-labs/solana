//! Defines a transaction which supports multiple versions of messages.

#![cfg(feature = "full")]

use {
    crate::{
        hash::Hash,
        message::VersionedMessage,
        sanitize::{Sanitize, SanitizeError},
        short_vec,
        signature::Signature,
        transaction::{Result, Transaction, TransactionError},
    },
    serde::Serialize,
    std::cmp::Ordering,
};

// NOTE: Serialization-related changes must be paired with the direct read at sigverify.
/// An atomic transaction
#[derive(Debug, PartialEq, Default, Eq, Clone, Serialize, Deserialize, AbiExample)]
pub struct VersionedTransaction {
    /// List of signatures
    #[serde(with = "short_vec")]
    pub signatures: Vec<Signature>,
    /// Message to sign.
    pub message: VersionedMessage,
}

impl Sanitize for VersionedTransaction {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        self.message.sanitize()?;

        let num_required_signatures = usize::from(self.message.header().num_required_signatures);
        match num_required_signatures.cmp(&self.signatures.len()) {
            Ordering::Greater => Err(SanitizeError::IndexOutOfBounds),
            Ordering::Less => Err(SanitizeError::InvalidValue),
            Ordering::Equal => Ok(()),
        }?;

        // Signatures are verified before message keys are mapped so all signers
        // must correspond to unmapped keys.
        if self.signatures.len() > self.message.unmapped_keys_len() {
            return Err(SanitizeError::IndexOutOfBounds);
        }

        Ok(())
    }
}

impl From<Transaction> for VersionedTransaction {
    fn from(transaction: Transaction) -> Self {
        Self {
            signatures: transaction.signatures,
            message: VersionedMessage::Legacy(transaction.message),
        }
    }
}

impl VersionedTransaction {
    /// Returns a legacy transaction if the transaction message is legacy.
    pub fn into_legacy_transaction(self) -> Option<Transaction> {
        match self.message {
            VersionedMessage::Legacy(message) => Some(Transaction {
                signatures: self.signatures,
                message,
            }),
            _ => None,
        }
    }

    /// Verify the transaction and hash its message
    pub fn verify_and_hash_message(&self) -> Result<Hash> {
        let message_bytes = self.message.serialize();
        if self
            .signatures
            .iter()
            .zip(self.message.unmapped_keys_iter())
            .map(|(signature, pubkey)| signature.verify(pubkey.as_ref(), &message_bytes))
            .any(|verified| !verified)
        {
            Err(TransactionError::SignatureFailure)
        } else {
            Ok(VersionedMessage::hash_raw_message(&message_bytes))
        }
    }
}
