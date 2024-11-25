use {
    super::{ComputeBudgetInstructionDetails, RuntimeTransaction},
    crate::{
        signature_details::get_precompile_signature_details,
        transaction_meta::{StaticMeta, TransactionMeta},
        transaction_with_meta::TransactionWithMeta,
    },
    solana_pubkey::Pubkey,
    solana_sdk::{
        message::{AddressLoader, TransactionSignatureDetails},
        simple_vote_transaction_checker::is_simple_vote_transaction,
        transaction::{
            MessageHash, Result, SanitizedTransaction, SanitizedVersionedTransaction,
            VersionedTransaction,
        },
    },
    solana_svm_transaction::instruction::SVMInstruction,
    std::{borrow::Cow, collections::HashSet},
};

impl RuntimeTransaction<SanitizedVersionedTransaction> {
    pub fn try_from(
        sanitized_versioned_tx: SanitizedVersionedTransaction,
        message_hash: MessageHash,
        is_simple_vote_tx: Option<bool>,
    ) -> Result<Self> {
        let message_hash = match message_hash {
            MessageHash::Precomputed(hash) => hash,
            MessageHash::Compute => sanitized_versioned_tx.get_message().message.hash(),
        };
        let is_simple_vote_tx = is_simple_vote_tx
            .unwrap_or_else(|| is_simple_vote_transaction(&sanitized_versioned_tx));

        let precompile_signature_details = get_precompile_signature_details(
            sanitized_versioned_tx
                .get_message()
                .program_instructions_iter()
                .map(|(program_id, ix)| (program_id, SVMInstruction::from(ix))),
        );
        let signature_details = TransactionSignatureDetails::new(
            u64::from(
                sanitized_versioned_tx
                    .get_message()
                    .message
                    .header()
                    .num_required_signatures,
            ),
            precompile_signature_details.num_secp256k1_instruction_signatures,
            precompile_signature_details.num_ed25519_instruction_signatures,
        );
        let compute_budget_instruction_details = ComputeBudgetInstructionDetails::try_from(
            sanitized_versioned_tx
                .get_message()
                .program_instructions_iter()
                .map(|(program_id, ix)| (program_id, SVMInstruction::from(ix))),
        )?;

        Ok(Self {
            transaction: sanitized_versioned_tx,
            meta: TransactionMeta {
                message_hash,
                is_simple_vote_transaction: is_simple_vote_tx,
                signature_details,
                compute_budget_instruction_details,
            },
        })
    }
}

impl RuntimeTransaction<SanitizedTransaction> {
    /// Create a new `RuntimeTransaction<SanitizedTransaction>` from an
    /// unsanitized `VersionedTransaction`.
    pub fn try_create(
        tx: VersionedTransaction,
        message_hash: MessageHash,
        is_simple_vote_tx: Option<bool>,
        address_loader: impl AddressLoader,
        reserved_account_keys: &HashSet<Pubkey>,
    ) -> Result<Self> {
        let statically_loaded_runtime_tx =
            RuntimeTransaction::<SanitizedVersionedTransaction>::try_from(
                SanitizedVersionedTransaction::try_from(tx)?,
                message_hash,
                is_simple_vote_tx,
            )?;
        Self::try_from(
            statically_loaded_runtime_tx,
            address_loader,
            reserved_account_keys,
        )
    }

    /// Create a new `RuntimeTransaction<SanitizedTransaction>` from a
    /// `RuntimeTransaction<SanitizedVersionedTransaction>` that already has
    /// static metadata loaded.
    pub fn try_from(
        statically_loaded_runtime_tx: RuntimeTransaction<SanitizedVersionedTransaction>,
        address_loader: impl AddressLoader,
        reserved_account_keys: &HashSet<Pubkey>,
    ) -> Result<Self> {
        let hash = *statically_loaded_runtime_tx.message_hash();
        let is_simple_vote_tx = statically_loaded_runtime_tx.is_simple_vote_transaction();
        let sanitized_transaction = SanitizedTransaction::try_new(
            statically_loaded_runtime_tx.transaction,
            hash,
            is_simple_vote_tx,
            address_loader,
            reserved_account_keys,
        )?;

        let mut tx = Self {
            transaction: sanitized_transaction,
            meta: statically_loaded_runtime_tx.meta,
        };
        tx.load_dynamic_metadata()?;

        Ok(tx)
    }

    fn load_dynamic_metadata(&mut self) -> Result<()> {
        Ok(())
    }
}

impl TransactionWithMeta for RuntimeTransaction<SanitizedTransaction> {
    #[inline]
    fn as_sanitized_transaction(&self) -> Cow<SanitizedTransaction> {
        Cow::Borrowed(self)
    }

    #[inline]
    fn to_versioned_transaction(&self) -> VersionedTransaction {
        self.transaction.to_versioned_transaction()
    }
}

#[cfg(feature = "dev-context-only-utils")]
impl RuntimeTransaction<SanitizedTransaction> {
    pub fn from_transaction_for_tests(transaction: solana_sdk::transaction::Transaction) -> Self {
        let versioned_transaction = VersionedTransaction::from(transaction);
        Self::try_create(
            versioned_transaction,
            MessageHash::Compute,
            None,
            solana_sdk::message::SimpleAddressLoader::Disabled,
            &HashSet::new(),
        )
        .expect("failed to create RuntimeTransaction from Transaction")
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
            feature_set::FeatureSet,
            hash::Hash,
            instruction::Instruction,
            message::Message,
            reserved_account_keys::ReservedAccountKeys,
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
            RuntimeTransaction::<SanitizedVersionedTransaction>::try_from(
                svt,
                MessageHash::Compute,
                is_simple_vote,
            )
            .unwrap()
            .meta
            .is_simple_vote_transaction
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
            RuntimeTransaction::<SanitizedVersionedTransaction>::try_from(
                non_vote_sanitized_versioned_transaction(),
                MessageHash::Precomputed(hash),
                None,
            )
            .unwrap();

        assert_eq!(hash, *statically_loaded_transaction.message_hash());
        assert!(!statically_loaded_transaction.is_simple_vote_transaction());

        let dynamically_loaded_transaction = RuntimeTransaction::<SanitizedTransaction>::try_from(
            statically_loaded_transaction,
            SimpleAddressLoader::Disabled,
            &ReservedAccountKeys::empty_key_set(),
        );
        let dynamically_loaded_transaction =
            dynamically_loaded_transaction.expect("created from statically loaded tx");

        assert_eq!(hash, *dynamically_loaded_transaction.message_hash());
        assert!(!dynamically_loaded_transaction.is_simple_vote_transaction());
    }

    #[test]
    fn test_runtime_transaction_static_meta() {
        let hash = Hash::new_unique();
        let compute_unit_limit = 250_000;
        let compute_unit_price = 1_000;
        let loaded_accounts_bytes = 1_024;
        let mut test_transaction = TestTransaction::new();

        let runtime_transaction_static =
            RuntimeTransaction::<SanitizedVersionedTransaction>::try_from(
                test_transaction
                    .add_compute_unit_limit(compute_unit_limit)
                    .add_compute_unit_price(compute_unit_price)
                    .add_loaded_accounts_bytes(loaded_accounts_bytes)
                    .to_sanitized_versioned_transaction(),
                MessageHash::Precomputed(hash),
                None,
            )
            .unwrap();

        assert_eq!(&hash, runtime_transaction_static.message_hash());
        assert!(!runtime_transaction_static.is_simple_vote_transaction());

        let signature_details = &runtime_transaction_static.meta.signature_details;
        assert_eq!(1, signature_details.num_transaction_signatures());
        assert_eq!(0, signature_details.num_secp256k1_instruction_signatures());
        assert_eq!(0, signature_details.num_ed25519_instruction_signatures());

        let compute_budget_limits = runtime_transaction_static
            .compute_budget_limits(&FeatureSet::default())
            .unwrap();
        assert_eq!(compute_unit_limit, compute_budget_limits.compute_unit_limit);
        assert_eq!(compute_unit_price, compute_budget_limits.compute_unit_price);
        assert_eq!(
            loaded_accounts_bytes,
            compute_budget_limits.loaded_accounts_bytes.get()
        );
    }
}
