use {
    super::{ComputeBudgetInstructionDetails, RuntimeTransaction},
    crate::{
        signature_details::get_precompile_signature_details,
        transaction_meta::{StaticMeta, TransactionMeta},
        transaction_with_meta::TransactionWithMeta,
    },
    agave_transaction_view::{
        resolved_transaction_view::ResolvedTransactionView, transaction_data::TransactionData,
        transaction_version::TransactionVersion, transaction_view::SanitizedTransactionView,
    },
    solana_pubkey::Pubkey,
    solana_sdk::{
        instruction::CompiledInstruction,
        message::{
            v0::{LoadedAddresses, LoadedMessage, MessageAddressTableLookup},
            LegacyMessage, MessageHeader, SanitizedMessage, TransactionSignatureDetails,
            VersionedMessage,
        },
        simple_vote_transaction_checker::is_simple_vote_transaction_impl,
        transaction::{
            MessageHash, Result, SanitizedTransaction, TransactionError, VersionedTransaction,
        },
    },
    solana_svm_transaction::svm_message::SVMMessage,
    std::{borrow::Cow, collections::HashSet},
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

impl<D: TransactionData> TransactionWithMeta for RuntimeTransaction<ResolvedTransactionView<D>> {
    fn as_sanitized_transaction(&self) -> Cow<SanitizedTransaction> {
        let VersionedTransaction {
            signatures,
            message,
        } = self.to_versioned_transaction();

        let is_writable_account_cache = (0..self.transaction.total_num_accounts())
            .map(|index| self.is_writable(usize::from(index)))
            .collect();

        let message = match message {
            VersionedMessage::Legacy(message) => SanitizedMessage::Legacy(LegacyMessage {
                message: Cow::Owned(message),
                is_writable_account_cache,
            }),
            VersionedMessage::V0(message) => SanitizedMessage::V0(LoadedMessage {
                message: Cow::Owned(message),
                loaded_addresses: Cow::Owned(self.loaded_addresses().unwrap().clone()),
                is_writable_account_cache,
            }),
        };

        // SAFETY:
        // - Simple conversion between different formats
        // - `ResolvedTransactionView` has undergone sanitization checks
        Cow::Owned(
            SanitizedTransaction::try_new_from_fields(
                message,
                *self.message_hash(),
                self.is_simple_vote_transaction(),
                signatures,
            )
            .expect("transaction view is sanitized"),
        )
    }

    fn to_versioned_transaction(&self) -> VersionedTransaction {
        let header = MessageHeader {
            num_required_signatures: self.num_required_signatures(),
            num_readonly_signed_accounts: self.num_readonly_signed_static_accounts(),
            num_readonly_unsigned_accounts: self.num_readonly_unsigned_static_accounts(),
        };
        let static_account_keys = self.static_account_keys().to_vec();
        let recent_blockhash = *self.recent_blockhash();
        let instructions = self
            .instructions_iter()
            .map(|ix| CompiledInstruction {
                program_id_index: ix.program_id_index,
                accounts: ix.accounts.to_vec(),
                data: ix.data.to_vec(),
            })
            .collect();

        let message = match self.version() {
            TransactionVersion::Legacy => {
                VersionedMessage::Legacy(solana_sdk::message::legacy::Message {
                    header,
                    account_keys: static_account_keys,
                    recent_blockhash,
                    instructions,
                })
            }
            TransactionVersion::V0 => VersionedMessage::V0(solana_sdk::message::v0::Message {
                header,
                account_keys: static_account_keys,
                recent_blockhash,
                instructions,
                address_table_lookups: self
                    .address_table_lookup_iter()
                    .map(|atl| MessageAddressTableLookup {
                        account_key: *atl.account_key,
                        writable_indexes: atl.writable_indexes.to_vec(),
                        readonly_indexes: atl.readonly_indexes.to_vec(),
                    })
                    .collect(),
            }),
        };

        VersionedTransaction {
            signatures: self.signatures().to_vec(),
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            address_lookup_table::AddressLookupTableAccount,
            hash::Hash,
            message::{v0, SimpleAddressLoader},
            reserved_account_keys::ReservedAccountKeys,
            signature::{Keypair, Signature},
            system_instruction, system_transaction,
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

    #[test]
    fn test_to_versioned_transaction() {
        fn assert_translation(
            original_transaction: VersionedTransaction,
            loaded_addresses: Option<LoadedAddresses>,
            reserved_account_keys: &HashSet<Pubkey>,
        ) {
            let bytes = bincode::serialize(&original_transaction).unwrap();
            let transaction_view = SanitizedTransactionView::try_new_sanitized(&bytes[..]).unwrap();
            let runtime_transaction = RuntimeTransaction::<SanitizedTransactionView<_>>::try_from(
                transaction_view,
                MessageHash::Compute,
                None,
            )
            .unwrap();
            let runtime_transaction = RuntimeTransaction::<ResolvedTransactionView<_>>::try_from(
                runtime_transaction,
                loaded_addresses,
                reserved_account_keys,
            )
            .unwrap();

            let versioned_transaction = runtime_transaction.to_versioned_transaction();
            assert_eq!(original_transaction, versioned_transaction);
        }

        let reserved_key_set = ReservedAccountKeys::empty_key_set();

        // Simple transfer.
        let original_transaction = VersionedTransaction::from(system_transaction::transfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            Hash::new_unique(),
        ));
        assert_translation(original_transaction, None, &reserved_key_set);

        // Simple transfer with loaded addresses.
        let payer = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let original_transaction = VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer,
                    &[system_instruction::transfer(&payer, &to, 1)],
                    &[AddressLookupTableAccount {
                        key: Pubkey::new_unique(),
                        addresses: vec![to],
                    }],
                    Hash::default(),
                )
                .unwrap(),
            ),
        };
        assert_translation(
            original_transaction,
            Some(LoadedAddresses {
                writable: vec![to],
                readonly: vec![],
            }),
            &reserved_key_set,
        );
    }

    #[test]
    fn test_as_sanitized_transaction() {
        fn assert_translation(
            original_transaction: SanitizedTransaction,
            loaded_addresses: Option<LoadedAddresses>,
            reserved_account_keys: &HashSet<Pubkey>,
        ) {
            let bytes =
                bincode::serialize(&original_transaction.to_versioned_transaction()).unwrap();
            let transaction_view = SanitizedTransactionView::try_new_sanitized(&bytes[..]).unwrap();
            let runtime_transaction = RuntimeTransaction::<SanitizedTransactionView<_>>::try_from(
                transaction_view,
                MessageHash::Compute,
                None,
            )
            .unwrap();
            let runtime_transaction = RuntimeTransaction::<ResolvedTransactionView<_>>::try_from(
                runtime_transaction,
                loaded_addresses,
                reserved_account_keys,
            )
            .unwrap();

            let sanitized_transaction = runtime_transaction.as_sanitized_transaction();
            assert_eq!(
                sanitized_transaction.message_hash(),
                original_transaction.message_hash()
            );
        }

        let reserved_key_set = ReservedAccountKeys::empty_key_set();

        // Simple transfer.
        let original_transaction = VersionedTransaction::from(system_transaction::transfer(
            &Keypair::new(),
            &Pubkey::new_unique(),
            1,
            Hash::new_unique(),
        ));
        let sanitized_transaction = SanitizedTransaction::try_create(
            original_transaction,
            MessageHash::Compute,
            None,
            SimpleAddressLoader::Disabled,
            &reserved_key_set,
        )
        .unwrap();
        assert_translation(sanitized_transaction, None, &reserved_key_set);

        // Simple transfer with loaded addresses.
        let payer = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        let original_transaction = VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer,
                    &[system_instruction::transfer(&payer, &to, 1)],
                    &[AddressLookupTableAccount {
                        key: Pubkey::new_unique(),
                        addresses: vec![to],
                    }],
                    Hash::default(),
                )
                .unwrap(),
            ),
        };
        let loaded_addresses = LoadedAddresses {
            writable: vec![to],
            readonly: vec![],
        };
        let sanitized_transaction = SanitizedTransaction::try_create(
            original_transaction,
            MessageHash::Compute,
            None,
            SimpleAddressLoader::Enabled(loaded_addresses.clone()),
            &reserved_key_set,
        )
        .unwrap();
        assert_translation(
            sanitized_transaction,
            Some(loaded_addresses),
            &reserved_key_set,
        );
    }
}
