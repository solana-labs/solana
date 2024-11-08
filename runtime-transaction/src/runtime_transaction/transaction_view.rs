use {
    super::{ComputeBudgetInstructionDetails, RuntimeTransaction},
    crate::{
        signature_details::get_precompile_signature_details, transaction_meta::TransactionMeta,
    },
    agave_transaction_view::{
        resolved_transaction_view::ResolvedTransactionView, transaction_data::TransactionData,
        transaction_version::TransactionVersion, transaction_view::SanitizedTransactionView,
    },
    solana_pubkey::Pubkey,
    solana_sdk::{
        message::{v0::LoadedAddresses, TransactionSignatureDetails, VersionedMessage},
        simple_vote_transaction_checker::is_simple_vote_transaction_impl,
        transaction::{MessageHash, Result, TransactionError},
    },
    std::collections::HashSet,
};

fn is_simple_vote_transaction<D: TransactionData>(
    transaction: &SanitizedTransactionView<D>,
) -> bool {
    let signatures = transaction.signatures();
    let is_legacy_message = matches!(transaction.version(), TransactionVersion::Legacy);
    let instruction_programs = transaction
        .program_instructions_iter()
        .map(|(program_id, _ix)| program_id);

    is_simple_vote_transaction_impl(signatures, is_legacy_message, instruction_programs)
}

impl<D: TransactionData> RuntimeTransaction<SanitizedTransactionView<D>> {
    pub fn try_from(
        transaction: SanitizedTransactionView<D>,
        message_hash: MessageHash,
        is_simple_vote_tx: Option<bool>,
    ) -> Result<Self> {
        let message_hash = match message_hash {
            MessageHash::Precomputed(hash) => hash,
            MessageHash::Compute => VersionedMessage::hash_raw_message(transaction.message_data()),
        };
        let is_simple_vote_tx =
            is_simple_vote_tx.unwrap_or_else(|| is_simple_vote_transaction(&transaction));

        let precompile_signature_details =
            get_precompile_signature_details(transaction.program_instructions_iter());
        let signature_details = TransactionSignatureDetails::new(
            u64::from(transaction.num_required_signatures()),
            precompile_signature_details.num_secp256k1_instruction_signatures,
            precompile_signature_details.num_ed25519_instruction_signatures,
        );
        let compute_budget_instruction_details =
            ComputeBudgetInstructionDetails::try_from(transaction.program_instructions_iter())?;

        Ok(Self {
            transaction,
            meta: TransactionMeta {
                message_hash,
                is_simple_vote_transaction: is_simple_vote_tx,
                signature_details,
                compute_budget_instruction_details,
            },
        })
    }
}

impl<D: TransactionData> RuntimeTransaction<ResolvedTransactionView<D>> {
    /// Create a new `RuntimeTransaction<ResolvedTransactionView>` from a
    /// `RuntimeTransaction<SanitizedTransactionView>` that already has
    /// static metadata loaded.
    pub fn try_from(
        statically_loaded_runtime_tx: RuntimeTransaction<SanitizedTransactionView<D>>,
        loaded_addresses: Option<LoadedAddresses>,
        reserved_account_keys: &HashSet<Pubkey>,
    ) -> Result<Self> {
        let RuntimeTransaction { transaction, meta } = statically_loaded_runtime_tx;
        // transaction-view does not distinguish between different types of errors here.
        // return generic sanitize failure error here.
        // these transactions should be immediately dropped, and we generally
        // will not care about the specific error at this point.
        let transaction =
            ResolvedTransactionView::try_new(transaction, loaded_addresses, reserved_account_keys)
                .map_err(|_| TransactionError::SanitizeFailure)?;
        let mut tx = Self { transaction, meta };
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
        crate::{runtime_transaction::RuntimeTransaction, transaction_meta::StaticMeta},
        agave_transaction_view::{
            resolved_transaction_view::ResolvedTransactionView,
            transaction_view::SanitizedTransactionView,
        },
        solana_pubkey::Pubkey,
        solana_sdk::{
            hash::Hash,
            reserved_account_keys::ReservedAccountKeys,
            signature::Keypair,
            system_transaction,
            transaction::{MessageHash, VersionedTransaction},
        },
    };

    #[test]
    fn test_advancing_transaction_type() {
        // Create serialized simple transfer.
        let serialized_transaction = {
            let transaction = VersionedTransaction::from(system_transaction::transfer(
                &Keypair::new(),
                &Pubkey::new_unique(),
                1,
                Hash::new_unique(),
            ));
            bincode::serialize(&transaction).unwrap()
        };

        let hash = Hash::new_unique();
        let transaction =
            SanitizedTransactionView::try_new_sanitized(&serialized_transaction[..]).unwrap();
        let static_runtime_transaction =
            RuntimeTransaction::<SanitizedTransactionView<_>>::try_from(
                transaction,
                MessageHash::Precomputed(hash),
                None,
            )
            .unwrap();

        assert_eq!(hash, *static_runtime_transaction.message_hash());
        assert!(!static_runtime_transaction.is_simple_vote_transaction());

        let dynamic_runtime_transaction =
            RuntimeTransaction::<ResolvedTransactionView<_>>::try_from(
                static_runtime_transaction,
                None,
                &ReservedAccountKeys::empty_key_set(),
            )
            .unwrap();

        assert_eq!(hash, *dynamic_runtime_transaction.message_hash());
        assert!(!dynamic_runtime_transaction.is_simple_vote_transaction());
    }
}
