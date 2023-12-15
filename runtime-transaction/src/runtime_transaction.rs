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
    solana_program_runtime::compute_budget_processor::{
        process_compute_budget_instructions, ComputeBudgetLimits,
    },
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
    fn compute_unit_limit(&self) -> u32 {
        self.meta.compute_unit_limit
    }
    fn compute_unit_price(&self) -> u64 {
        self.meta.compute_unit_price
    }
    fn loaded_accounts_bytes(&self) -> u32 {
        self.meta.loaded_accounts_bytes
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

        let ComputeBudgetLimits {
            compute_unit_limit,
            compute_unit_price,
            loaded_accounts_bytes,
            ..
        } = process_compute_budget_instructions(message.program_instructions_iter())?;
        meta.set_compute_unit_limit(compute_unit_limit);
        meta.set_compute_unit_price(compute_unit_price);
        meta.set_loaded_accounts_bytes(loaded_accounts_bytes);

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
            instruction::Instruction,
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

    fn non_vote_sanitized_versioned_transaction() -> SanitizedVersionedTransaction {
        TestTransaction::new().to_sanitized_versioned_transaction()
    }

    // Simple transfer transaction for testing, it does not support vote instruction
    // because simple vote transaction will not request limits
    struct TestTransaction {
        from_keypair: Keypair,
        hash: Hash,
        instructions: Vec<Instruction>,
    }

    impl TestTransaction {
        fn new() -> Self {
            let from_keypair = Keypair::new();
            let instructions = vec![system_instruction::transfer(
                &from_keypair.pubkey(),
                &solana_sdk::pubkey::new_rand(),
                1,
            )];
            TestTransaction {
                from_keypair,
                hash: Hash::new_unique(),
                instructions,
            }
        }

        fn add_compute_unit_limit(&mut self, val: u32) -> &mut TestTransaction {
            self.instructions
                .push(ComputeBudgetInstruction::set_compute_unit_limit(val));
            self
        }

        fn add_compute_unit_price(&mut self, val: u64) -> &mut TestTransaction {
            self.instructions
                .push(ComputeBudgetInstruction::set_compute_unit_price(val));
            self
        }

        fn add_loaded_accounts_bytes(&mut self, val: u32) -> &mut TestTransaction {
            self.instructions
                .push(ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(val));
            self
        }

        fn to_sanitized_versioned_transaction(&self) -> SanitizedVersionedTransaction {
            let message = Message::new(&self.instructions, Some(&self.from_keypair.pubkey()));
            let tx = Transaction::new(&[&self.from_keypair], message, self.hash);
            SanitizedVersionedTransaction::try_from(VersionedTransaction::from(tx)).unwrap()
        }
    }

    #[test]
    fn test_runtime_transaction_is_vote_meta() {
        fn get_is_simple_vote(
            svt: SanitizedVersionedTransaction,
            is_simple_vote: Option<bool>,
        ) -> bool {
            RuntimeTransaction::<SanitizedVersionedMessage>::try_from(svt, None, is_simple_vote)
                .unwrap()
                .meta
                .is_simple_vote_tx
        }

        assert!(!get_is_simple_vote(
            non_vote_sanitized_versioned_transaction(),
            None
        ));

        assert!(get_is_simple_vote(
            non_vote_sanitized_versioned_transaction(),
            Some(true), // override
        ));

        assert!(get_is_simple_vote(
            vote_sanitized_versioned_transaction(),
            None
        ));

        assert!(!get_is_simple_vote(
            vote_sanitized_versioned_transaction(),
            Some(false), // override
        ));
    }

    #[test]
    fn test_advancing_transaction_type() {
        let hash = Hash::new_unique();

        let statically_loaded_transaction =
            RuntimeTransaction::<SanitizedVersionedMessage>::try_from(
                non_vote_sanitized_versioned_transaction(),
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

    #[test]
    fn test_runtime_transaction_static_meta() {
        let hash = Hash::new_unique();
        let compute_unit_limit = 250_000;
        let compute_unit_price = 1_000;
        let loaded_accounts_bytes = 1_024;
        let mut test_transaction = TestTransaction::new();

        let runtime_transaction_static = RuntimeTransaction::<SanitizedVersionedMessage>::try_from(
            test_transaction
                .add_compute_unit_limit(compute_unit_limit)
                .add_compute_unit_price(compute_unit_price)
                .add_loaded_accounts_bytes(loaded_accounts_bytes)
                .to_sanitized_versioned_transaction(),
            Some(hash),
            None,
        )
        .unwrap();

        assert_eq!(&hash, runtime_transaction_static.message_hash());
        assert!(!runtime_transaction_static.is_simple_vote_tx());
        assert_eq!(
            compute_unit_limit,
            runtime_transaction_static.compute_unit_limit()
        );
        assert_eq!(
            compute_unit_price,
            runtime_transaction_static.compute_unit_price()
        );
        assert_eq!(
            loaded_accounts_bytes,
            runtime_transaction_static.loaded_accounts_bytes()
        );
    }
}
