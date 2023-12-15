//! RuntimeTransaction is `runtime` facing representation of transaction, while
//! solana_sdk::SanitizedTransaction is client facing representation.
//!
//! It has two states:
//! 1. Statically Loaded: after receiving `packet` from sigverify and deserializing
//!    it into `solana_sdk::VersionedTransaction`, then sanitizing into
//!    `solana_sdk::SanitizedVersionedTransaction`, which can be wrapped into
//!    `RuntimeTransaction` with static transaction metadata extracted.
//! 2. Dynamically Loaded: after successfully loaded account addresses from onchain
//!    ALT, RuntimeTransaction<SanitizedMessage> transits into Dynamically Loaded state,
//!    with its dynamic metadata loaded.
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
pub struct RuntimeTransaction<M> {
    signatures: Vec<Signature>,
    message: M,
    // transaction meta is a collection of fields, it is updated
    // during message state transition
    meta: TransactionMeta,
}

// These traits gate access to static and dynamic metadata
// so that only transactions with supporting message types
// can access them.
trait StaticMetaAccess {}
trait DynamicMetaAccess: StaticMetaAccess {}

// Implement the gate traits for the message types that should
// have access to the static and dynamic metadata.
impl StaticMetaAccess for SanitizedVersionedMessage {}
impl StaticMetaAccess for SanitizedMessage {}
impl DynamicMetaAccess for SanitizedMessage {}

impl<M: StaticMetaAccess> StaticMeta for RuntimeTransaction<M> {
    fn message_hash(&self) -> &Hash {
        &self.meta.message_hash
    }
    fn is_simple_vote_tx(&self) -> bool {
        self.meta.is_simple_vote_tx
    }
}

impl<M: DynamicMetaAccess> DynamicMeta for RuntimeTransaction<M> {}

impl RuntimeTransaction<SanitizedVersionedMessage> {
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

impl RuntimeTransaction<SanitizedMessage> {
    pub fn try_from(
        statically_loaded_runtime_tx: RuntimeTransaction<SanitizedVersionedMessage>,
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

    fn load_dynamic_metadata(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program::{
            system_instruction,
            vote::{self, state::Vote},
        },
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            message::Message,
            signer::{keypair::Keypair, Signer},
            transaction::{SimpleAddressLoader, Transaction, VersionedTransaction},
        },
    };

    fn vote_sanitized_versioned_transaction() -> SanitizedVersionedTransaction {
        let bank_hash = Hash::new_unique();
        let block_hash = Hash::new_unique();
        let vote_keypair = Keypair::new();
        let node_keypair = Keypair::new();
        let auth_keypair = Keypair::new();
        let votes = Vote::new(vec![1, 2, 3], bank_hash);
        let vote_ix =
            vote::instruction::vote(&vote_keypair.pubkey(), &auth_keypair.pubkey(), votes);
        let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));
        vote_tx.partial_sign(&[&node_keypair], block_hash);
        vote_tx.partial_sign(&[&auth_keypair], block_hash);

        SanitizedVersionedTransaction::try_from(VersionedTransaction::from(vote_tx)).unwrap()
    }

    fn non_vote_sanitized_versioned_transaction(
        compute_unit_price: u64,
    ) -> SanitizedVersionedTransaction {
        let from_keypair = Keypair::new();
        let ixs = vec![
            system_instruction::transfer(
                &from_keypair.pubkey(),
                &solana_sdk::pubkey::new_rand(),
                1,
            ),
            ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
        ];
        let message = Message::new(&ixs, Some(&from_keypair.pubkey()));
        let tx = Transaction::new(&[&from_keypair], message, Hash::new_unique());
        SanitizedVersionedTransaction::try_from(VersionedTransaction::from(tx)).unwrap()
    }

    fn get_transaction_meta(
        svt: SanitizedVersionedTransaction,
        hash: Option<Hash>,
        is_simple_vote: Option<bool>,
    ) -> TransactionMeta {
        RuntimeTransaction::<SanitizedVersionedMessage>::try_from(svt, hash, is_simple_vote)
            .unwrap()
            .meta
    }

    #[test]
    fn test_new_runtime_transaction_static() {
        let hash = Hash::new_unique();
        let compute_unit_price = 1_000;

        assert_eq!(
            TransactionMeta {
                message_hash: hash,
                is_simple_vote_tx: false,
            },
            get_transaction_meta(
                non_vote_sanitized_versioned_transaction(compute_unit_price),
                Some(hash),
                None
            )
        );

        assert_eq!(
            TransactionMeta {
                message_hash: hash,
                is_simple_vote_tx: true,
            },
            get_transaction_meta(
                non_vote_sanitized_versioned_transaction(compute_unit_price),
                Some(hash),
                Some(true), // override
            )
        );

        assert_eq!(
            TransactionMeta {
                message_hash: hash,
                is_simple_vote_tx: true,
            },
            get_transaction_meta(vote_sanitized_versioned_transaction(), Some(hash), None)
        );

        assert_eq!(
            TransactionMeta {
                message_hash: hash,
                is_simple_vote_tx: false,
            },
            get_transaction_meta(
                vote_sanitized_versioned_transaction(),
                Some(hash),
                Some(false), // override
            )
        );
    }

    #[test]
    fn test_advance_transaction_type() {
        let hash = Hash::new_unique();
        let compute_unit_price = 999;

        let statically_loaded_transaction =
            RuntimeTransaction::<SanitizedVersionedMessage>::try_from(
                non_vote_sanitized_versioned_transaction(compute_unit_price),
                Some(hash),
                None,
            )
            .unwrap();

        assert_eq!(hash, *statically_loaded_transaction.message_hash());
        assert!(!statically_loaded_transaction.is_simple_vote_tx());

        let dynamically_loaded_transaction = RuntimeTransaction::<SanitizedMessage>::try_from(
            statically_loaded_transaction,
            SimpleAddressLoader::Disabled,
        );
        let dynamically_loaded_transaction =
            dynamically_loaded_transaction.expect("created from statically loaded tx");

        assert_eq!(hash, *dynamically_loaded_transaction.message_hash());
        assert!(!dynamically_loaded_transaction.is_simple_vote_tx());
    }
}
