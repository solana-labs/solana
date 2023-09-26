use {
    crate::{
        simple_vote_transaction_checker::is_simple_vote_transaction, transaction_meta::TransactionMeta,
    },
    solana_sdk::{
        hash::Hash,
        message::{
            v0::{self},
            AddressLoader, LegacyMessage, SanitizedMessage, SanitizedVersionedMessage,
            VersionedMessage,
        },
        signature::Signature,
        transaction::{MessageHash, Result, SanitizedVersionedTransaction},
    },
};

/// RuntimeTransaction is runtime face representation of transaction, as
/// SanitizedTransaction is client facing rep.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeTransaction {
    message: SanitizedMessage,
    message_hash: Hash,
    signatures: Vec<Signature>,
    transaction_meta: TransactionMeta,
}

impl RuntimeTransaction {
    /// Create a runtime transaction from a sanitized versioend transaction,
    /// then load account addresses from address-lookup-table if apply, and
    /// populate transaction metadata.
    pub fn try_new(
        // Note: transaction sanitization is a function of `sdk` that is shared
        // by both runtime and client.
        sanitized_versioned_tx: SanitizedVersionedTransaction,
        message_hash: impl Into<MessageHash>,
        is_simple_vote_tx: Option<bool>,
        address_loader: impl AddressLoader,
    ) -> Result<Self> {
        // metadata can be lazily updated alone transaction's execution path
        let mut transaction_meta = TransactionMeta::default();
        transaction_meta.set_is_simple_vote_tx(is_simple_vote_tx.unwrap_or_else(|| {
            is_simple_vote_transaction(&sanitized_versioned_tx)
        }));

        let signatures = sanitized_versioned_tx.signatures;

        let SanitizedVersionedMessage { message } = sanitized_versioned_tx.message;

        let message_hash = match message_hash.into() {
            MessageHash::Compute => message.hash(),
            MessageHash::Precomputed(hash) => hash,
        };

        let message = match message {
            VersionedMessage::Legacy(message) => {
                SanitizedMessage::Legacy(LegacyMessage::new(message))
            }
            VersionedMessage::V0(message) => {
                let loaded_addresses =
                    address_loader.load_addresses(&message.address_table_lookups)?;
                SanitizedMessage::V0(v0::LoadedMessage::new(message, loaded_addresses))
            }
        };

        Ok(Self {
            message,
            message_hash,
            signatures,
            transaction_meta,
        })
    }
}
