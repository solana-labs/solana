/// RuntimeTransaction is `runtime` facing representation of transaction, while
/// solana_sdk::SanitizedTransaction is client facing representation.
///
/// It has two states:
/// 1. Statically Loaded: after receiving `packet` from sigverify and deserializing
///    it into `solana_sdk::VersionedTransaction`, then sanitizing into
///    `solana_sdk::SanitizedVersionedTransaction`, `RuntimeTransactionStatic`
///    can be created from it with static transaction metadata extracted.
/// 2. Dynamically Loaded: after successfully loaded account addresses from onchain
///    ALT, RuntimeTransaction transits into Dynamically Loaded state, with
///    its dynamic metadata loaded.
use {
    crate::transaction_meta::{DynamicMeta, StaticMeta, TransactionMeta},
    solana_sdk::{
        hash::Hash,
        message::{AddressLoader, SanitizedMessage, SanitizedVersionedMessage},
        signature::Signature,
        simple_vote_transaction_checker::is_simple_vote_transaction,
        transaction::{Result, SanitizedVersionedTransaction},
    },
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeTransactionStatic {
    // sanitized signatures
    signatures: Vec<Signature>,

    // sanitized message
    message: SanitizedVersionedMessage,

    // transaction meta is a collection of fields, it is updated
    // during message state transition
    meta: TransactionMeta,
}

impl StaticMeta for RuntimeTransactionStatic {
    fn message_hash(&self) -> &Hash {
        &self.meta.message_hash
    }
    fn is_simple_vote_tx(&self) -> bool {
        self.meta.is_simple_vote_tx
    }
}

impl RuntimeTransactionStatic {
    pub fn try_from(
        sanitized_versioned_tx: SanitizedVersionedTransaction,
        message_hash: Option<Hash>,
        is_simple_vote_tx: Option<bool>,
    ) -> Result<Self> {
        let mut meta = TransactionMeta::default();
        meta.set_is_simple_vote_tx(
            is_simple_vote_tx
                .unwrap_or_else(|| is_simple_vote_transaction(&sanitized_versioned_tx)),
        );

        let (signatures, message) = sanitized_versioned_tx.destruct();

        meta.set_message_hash(message_hash.unwrap_or_else(|| message.message.hash()));

        Ok(Self {
            signatures,
            message,
            meta,
        })
    }
}

/// Statically Loaded transaction can transit to Dynamically Loaded with supplied
/// address_loader, to load accounts from on-chain ALT, then resolve dynamic metadata
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeTransactionDynamic {
    // sanitized signatures
    signatures: Vec<Signature>,

    // sanitized message
    message: SanitizedMessage,

    // transaction meta is a collection of fields, it is updated
    // during message state transition
    meta: TransactionMeta,
}

impl DynamicMeta for RuntimeTransactionDynamic {}

impl StaticMeta for RuntimeTransactionDynamic {
    fn message_hash(&self) -> &Hash {
        &self.meta.message_hash
    }
    fn is_simple_vote_tx(&self) -> bool {
        self.meta.is_simple_vote_tx
    }
}

impl RuntimeTransactionDynamic {
    pub fn try_from(
        statically_loaded_runtime_tx: RuntimeTransactionStatic,
        address_loader: impl AddressLoader,
    ) -> Result<Self> {
        let mut tx = Self {
            signatures: statically_loaded_runtime_tx.signatures,
            message: SanitizedMessage::try_new(
                statically_loaded_runtime_tx.message,
                address_loader,
            )?,
            meta: statically_loaded_runtime_tx.meta,
        };
        tx.load_dynamic_metadata()?;

        Ok(tx)
    }

    // private helpers
    fn load_dynamic_metadata(&mut self) -> Result<()> {
        Ok(())
    }
}
