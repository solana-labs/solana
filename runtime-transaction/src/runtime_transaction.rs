use {
    crate::{
        simple_vote_transaction_checker::is_simple_vote_transaction,
        transaction_meta::{ComputeBudgetLimitsTTL, DynamicMeta, StaticMeta, TransactionMeta},
    },
    solana_program_runtime::compute_budget_processor::{
        process_compute_budget_instructions, ComputeBudgetLimits,
    },
    solana_sdk::{
        feature_set::FeatureSet,
        hash::Hash,
        message::{AddressLoader, SanitizedMessage, SanitizedVersionedMessage},
        signature::Signature,
        slot_history::Slot,
        transaction::{Result, SanitizedVersionedTransaction},
    },
};

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

#[derive(Debug)]
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
    fn compute_budget_limits(&self, current_slot: Slot) -> Option<&ComputeBudgetLimits> {
        (current_slot <= self.meta.compute_budget_limits_ttl.max_age_slot)
            .then(|| &self.meta.compute_budget_limits_ttl.compute_budget_limits)
    }
}

impl RuntimeTransactionStatic {
    pub fn try_from(
        sanitized_versioned_tx: SanitizedVersionedTransaction,
        message_hash: Option<Hash>,
        is_simple_vote_tx: Option<bool>,
        feature_set: &FeatureSet,
        max_age_slot: Slot,
    ) -> Result<Self> {
        let meta = Self::load_static_metadata(
            &sanitized_versioned_tx,
            message_hash,
            is_simple_vote_tx,
            feature_set,
            max_age_slot,
        )?;

        Ok(Self {
            signatures: sanitized_versioned_tx.signatures,
            message: sanitized_versioned_tx.message,
            meta,
        })
    }

    // Cached TransactionMeta has exprity time, usually the last slot of epoch.
    // RuntimeTransaction types are read-only hence thread-safe. After exxpiry, the
    // RuntimeTransaction instance should be discarded, or renew into a new instance
    pub fn renew_from(self, feature_set: &FeatureSet, max_age_slot: Slot) -> Result<Self> {
        let compute_budget_limits: ComputeBudgetLimits = process_compute_budget_instructions(
            self.message.program_instructions_iter(),
            feature_set,
        )?;

        let meta = TransactionMeta {
            message_hash: *self.message_hash(),
            is_simple_vote_tx: self.is_simple_vote_tx(),
            compute_budget_limits_ttl: ComputeBudgetLimitsTTL {
                compute_budget_limits,
                max_age_slot,
            },
            ..TransactionMeta::default()
        };

        Ok(Self {
            signatures: self.signatures,
            message: self.message,
            meta,
        })
    }

    // private helpers
    fn load_static_metadata(
        sanitized_versioned_tx: &SanitizedVersionedTransaction,
        message_hash: Option<Hash>,
        is_simple_vote_tx: Option<bool>,
        feature_set: &FeatureSet,
        max_age_slot: Slot,
    ) -> Result<TransactionMeta> {
        let mut meta = TransactionMeta::default();
        meta.set_is_simple_vote_tx(
            is_simple_vote_tx.unwrap_or_else(|| is_simple_vote_transaction(sanitized_versioned_tx)),
        );
        meta.set_message_hash(
            message_hash.unwrap_or_else(|| sanitized_versioned_tx.message.message.hash()),
        );

        let compute_budget_limits: ComputeBudgetLimits = process_compute_budget_instructions(
            sanitized_versioned_tx.message.program_instructions_iter(),
            feature_set,
        )?;
        meta.set_compute_budget_limits(compute_budget_limits, max_age_slot);

        Ok(meta)
    }
}

/// Statically Loaded transaction can transit to Dynamically Loaded with supplied
/// address_loader, to load accounts from on-chain ALT, then resolve dynamic metadata
#[derive(Debug)]
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
    fn compute_budget_limits(&self, current_slot: Slot) -> Option<&ComputeBudgetLimits> {
        (current_slot <= self.meta.compute_budget_limits_ttl.max_age_slot)
            .then(|| &self.meta.compute_budget_limits_ttl.compute_budget_limits)
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

    // Cached TransactionMeta has exprity time, usually the last slot of epoch.
    // RuntimeTransaction types are read-only hence thread-safe. After exxpiry, the
    // RuntimeTransaction instance should be discarded, or renew into a new instance
    pub fn renew_from(
        self,
        feature_set: &FeatureSet,
        max_age_slot: Slot,
        address_loader: impl AddressLoader,
    ) -> Result<Self> {
        let compute_budget_limits: ComputeBudgetLimits = process_compute_budget_instructions(
            self.message.program_instructions_iter(),
            feature_set,
        )?;

        let meta = TransactionMeta {
            message_hash: *self.message_hash(),
            is_simple_vote_tx: self.is_simple_vote_tx(),
            compute_budget_limits_ttl: ComputeBudgetLimitsTTL {
                compute_budget_limits,
                max_age_slot,
            },
            ..TransactionMeta::default()
        };

        let mut tx = Self {
            signatures: self.signatures,
            message: self.message,
            meta,
        };
        tx.load_dynamic_metadata()?;

        Ok(tx)
    }

    // private helpers
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
            transaction::{Transaction, VersionedTransaction},
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

    #[test]
    fn test_new_runtime_transaction_static() {
        let hash = Hash::new_unique();
        let compute_unit_price = 1_000;
        let feature_set = FeatureSet::default();
        let max_age_slot = 100;

        let runtime_transaction_static = RuntimeTransactionStatic::try_from(
            non_vote_sanitized_versioned_transaction(compute_unit_price),
            Some(hash.clone()),
            None,
            &feature_set,
            max_age_slot,
        )
        .unwrap();

        // validates before metadata expiration
        {
            assert_eq!(&hash, runtime_transaction_static.message_hash());
            assert_eq!(false, runtime_transaction_static.is_simple_vote_tx());
            let compute_budget_limits =
                runtime_transaction_static.compute_budget_limits(max_age_slot);
            assert!(compute_budget_limits.is_some());
            assert_eq!(
                compute_unit_price,
                compute_budget_limits.unwrap().compute_unit_price
            );
        }

        // validates after metadata expiration
        {
            let compute_budget_limits =
                runtime_transaction_static.compute_budget_limits(max_age_slot + 1);
            assert!(compute_budget_limits.is_none());
        }
    }

    #[test]
    fn test_renew_runtime_transaction_static() {
        let hash = Hash::new_unique();
        let compute_unit_price = 1_000;
        let feature_set = FeatureSet::default();
        let max_age_slot = 100;

        let runtime_transaction_static = RuntimeTransactionStatic::try_from(
            non_vote_sanitized_versioned_transaction(compute_unit_price),
            Some(hash.clone()),
            None,
            &feature_set,
            max_age_slot,
        )
        .unwrap();

        // validates after metadata expiration
        let current_slot = max_age_slot + 1;
        {
            let compute_budget_limits =
                runtime_transaction_static.compute_budget_limits(current_slot);
            assert!(compute_budget_limits.is_none());
        }

        let renewed_runtime_transaction_static = runtime_transaction_static
            .renew_from(&feature_set, current_slot)
            .unwrap();

        // validates after renewal
        {
            assert_eq!(&hash, renewed_runtime_transaction_static.message_hash());
            assert_eq!(
                false,
                renewed_runtime_transaction_static.is_simple_vote_tx()
            );
            let compute_budget_limits =
                renewed_runtime_transaction_static.compute_budget_limits(max_age_slot);
            assert!(compute_budget_limits.is_some());
            assert_eq!(
                compute_unit_price,
                compute_budget_limits.unwrap().compute_unit_price
            );
        }
    }
}
