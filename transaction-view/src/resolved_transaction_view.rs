use {
    crate::{
        result::{Result, TransactionViewError},
        transaction_data::TransactionData,
        transaction_view::TransactionView,
    },
    core::{
        fmt::{Debug, Formatter},
        ops::Deref,
    },
    solana_sdk::{
        bpf_loader_upgradeable, ed25519_program,
        hash::Hash,
        message::{v0::LoadedAddresses, AccountKeys, TransactionSignatureDetails},
        pubkey::Pubkey,
        secp256k1_program,
    },
    solana_svm_transaction::{
        instruction::SVMInstruction, message_address_table_lookup::SVMMessageAddressTableLookup,
        svm_message::SVMMessage,
    },
    std::collections::HashSet,
};

/// A parsed and sanitized transaction view that has had all address lookups
/// resolved.
pub struct ResolvedTransactionView<D: TransactionData> {
    /// The parsed and sanitized transction view.
    view: TransactionView<true, D>,
    /// The resolved address lookups.
    resolved_addresses: Option<LoadedAddresses>,
    /// A cache for whether an address is writable.
    writable_cache: Vec<bool>, // TODO: should this be a vec, bitset, or array[256].
}

impl<D: TransactionData> Deref for ResolvedTransactionView<D> {
    type Target = TransactionView<true, D>;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

impl<D: TransactionData> ResolvedTransactionView<D> {
    /// Given a parsed and sanitized transaction view, and a set of resolved
    /// addresses, create a resolved transaction view.
    pub fn try_new(
        view: TransactionView<true, D>,
        resolved_addresses: Option<LoadedAddresses>,
        reserved_account_keys: &HashSet<Pubkey>,
    ) -> Result<Self> {
        let resolved_addresses_ref = resolved_addresses.as_ref();

        // verify that the number of readable and writable match up.
        // This is a basic sanity check to make sure we're not passing a totally
        // invalid set of resolved addresses.
        if let Some(loaded_addresses) = resolved_addresses_ref {
            if loaded_addresses.writable.len() != usize::from(view.total_writable_lookup_accounts())
                || loaded_addresses.readonly.len()
                    != usize::from(view.total_readonly_lookup_accounts())
            {
                return Err(TransactionViewError::AddressLookupMismatch);
            }
        } else if view.total_writable_lookup_accounts() != 0
            || view.total_readonly_lookup_accounts() != 0
        {
            return Err(TransactionViewError::AddressLookupMismatch);
        }

        let writable_cache =
            Self::cache_is_writable(&view, resolved_addresses_ref, reserved_account_keys);
        Ok(Self {
            view,
            resolved_addresses,
            writable_cache,
        })
    }

    /// Helper function to check if an address is writable,
    /// and cache the result.
    /// This is done so we avoid recomputing the expensive checks each time we call
    /// `is_writable` - since there is more to it than just checking index.
    fn cache_is_writable(
        view: &TransactionView<true, D>,
        resolved_addresses: Option<&LoadedAddresses>,
        reserved_account_keys: &HashSet<Pubkey>,
    ) -> Vec<bool> {
        // Build account keys so that we can iterate over and check if
        // an address is writable.
        let account_keys = AccountKeys::new(view.static_account_keys(), resolved_addresses);

        let mut is_writable_cache = Vec::with_capacity(account_keys.len());
        let num_static_account_keys = usize::from(view.num_static_account_keys());
        let num_writable_lookup_accounts = usize::from(view.total_writable_lookup_accounts());
        let num_signed_accounts = usize::from(view.num_required_signatures());
        let num_writable_unsigned_static_accounts =
            usize::from(view.num_writable_unsigned_static_accounts());
        let num_writable_signed_static_accounts =
            usize::from(view.num_writable_signed_static_accounts());

        for (index, key) in account_keys.iter().enumerate() {
            let is_requested_write = {
                // If the account is a resolved address, check if it is writable.
                if index >= num_static_account_keys {
                    let loaded_address_index = index.wrapping_sub(num_static_account_keys);
                    loaded_address_index < num_writable_lookup_accounts
                } else if index >= num_signed_accounts {
                    let unsigned_account_index = index.wrapping_sub(num_signed_accounts);
                    unsigned_account_index < num_writable_unsigned_static_accounts
                } else {
                    index < num_writable_signed_static_accounts
                }
            };

            // If the key is reserved it cannot be writable.
            is_writable_cache.push(is_requested_write && !reserved_account_keys.contains(key));
        }

        // If a program account is locked, it cannot be writable unless the
        // upgradable loader is present.
        // However, checking for the upgradable loader is somewhat expensive, so
        // we only do it if we find a writable program id.
        let mut is_upgradable_loader_present = None;
        for ix in view.instructions_iter() {
            let program_id_index = usize::from(ix.program_id_index);
            if is_writable_cache[program_id_index]
                && !*is_upgradable_loader_present.get_or_insert_with(|| {
                    for key in account_keys.iter() {
                        if key == &bpf_loader_upgradeable::ID {
                            return true;
                        }
                    }
                    false
                })
            {
                is_writable_cache[program_id_index] = false;
            }
        }

        is_writable_cache
    }

    fn num_readonly_accounts(&self) -> usize {
        usize::from(self.view.total_readonly_lookup_accounts())
            .wrapping_add(usize::from(self.view.num_readonly_signed_static_accounts()))
            .wrapping_add(usize::from(
                self.view.num_readonly_unsigned_static_accounts(),
            ))
    }

    fn signature_details(&self) -> TransactionSignatureDetails {
        // counting the number of pre-processor operations separately
        let mut num_secp256k1_instruction_signatures: u64 = 0;
        let mut num_ed25519_instruction_signatures: u64 = 0;
        for (program_id, instruction) in self.program_instructions_iter() {
            if secp256k1_program::check_id(program_id) {
                if let Some(num_verifies) = instruction.data.first() {
                    num_secp256k1_instruction_signatures =
                        num_secp256k1_instruction_signatures.wrapping_add(u64::from(*num_verifies));
                }
            } else if ed25519_program::check_id(program_id) {
                if let Some(num_verifies) = instruction.data.first() {
                    num_ed25519_instruction_signatures =
                        num_ed25519_instruction_signatures.wrapping_add(u64::from(*num_verifies));
                }
            }
        }

        TransactionSignatureDetails::new(
            u64::from(self.view.num_required_signatures()),
            num_secp256k1_instruction_signatures,
            num_ed25519_instruction_signatures,
        )
    }
}

impl<D: TransactionData> SVMMessage for ResolvedTransactionView<D> {
    fn num_total_signatures(&self) -> u64 {
        self.signature_details().total_signatures()
    }

    fn num_write_locks(&self) -> u64 {
        self.account_keys()
            .len()
            .wrapping_sub(self.num_readonly_accounts()) as u64
    }

    fn recent_blockhash(&self) -> &Hash {
        self.view.recent_blockhash()
    }

    fn num_instructions(&self) -> usize {
        usize::from(self.view.num_instructions())
    }

    fn instructions_iter(&self) -> impl Iterator<Item = SVMInstruction> {
        self.view.instructions_iter()
    }

    fn program_instructions_iter(
        &self,
    ) -> impl Iterator<
        Item = (
            &solana_sdk::pubkey::Pubkey,
            solana_svm_transaction::instruction::SVMInstruction,
        ),
    > {
        self.view.program_instructions_iter()
    }

    fn account_keys(&self) -> AccountKeys {
        AccountKeys::new(
            self.view.static_account_keys(),
            self.resolved_addresses.as_ref(),
        )
    }

    fn fee_payer(&self) -> &Pubkey {
        &self.view.static_account_keys()[0]
    }

    fn is_writable(&self, index: usize) -> bool {
        self.writable_cache.get(index).copied().unwrap_or(false)
    }

    fn is_signer(&self, index: usize) -> bool {
        index < usize::from(self.view.num_required_signatures())
    }

    fn is_invoked(&self, key_index: usize) -> bool {
        let Ok(index) = u8::try_from(key_index) else {
            return false;
        };
        self.view
            .instructions_iter()
            .any(|ix| ix.program_id_index == index)
    }

    fn num_lookup_tables(&self) -> usize {
        usize::from(self.view.num_address_table_lookups())
    }

    fn message_address_table_lookups(&self) -> impl Iterator<Item = SVMMessageAddressTableLookup> {
        self.view.address_table_lookup_iter()
    }
}

impl<D: TransactionData> Debug for ResolvedTransactionView<D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedTransactionView")
            .field("view", &self.view)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::transaction_view::SanitizedTransactionView,
        solana_sdk::{
            instruction::CompiledInstruction,
            message::{
                v0::{self, MessageAddressTableLookup},
                MessageHeader, VersionedMessage,
            },
            signature::Signature,
            system_program, sysvar,
            transaction::VersionedTransaction,
        },
    };

    #[test]
    fn test_expected_loaded_addresses() {
        // Expected addresses passed in, but `None` was passed.
        let static_keys = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let transaction = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(v0::Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                instructions: vec![],
                account_keys: static_keys,
                address_table_lookups: vec![MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![0],
                    readonly_indexes: vec![1],
                }],
                recent_blockhash: Hash::default(),
            }),
        };
        let bytes = bincode::serialize(&transaction).unwrap();
        let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
        let result = ResolvedTransactionView::try_new(view, None, &HashSet::default());
        assert!(matches!(
            result,
            Err(TransactionViewError::AddressLookupMismatch)
        ));
    }

    #[test]
    fn test_unexpected_loaded_addresses() {
        // Expected no addresses passed in, but `Some` was passed.
        let static_keys = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let loaded_addresses = LoadedAddresses {
            writable: vec![Pubkey::new_unique()],
            readonly: vec![],
        };
        let transaction = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(v0::Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                instructions: vec![],
                account_keys: static_keys,
                address_table_lookups: vec![],
                recent_blockhash: Hash::default(),
            }),
        };
        let bytes = bincode::serialize(&transaction).unwrap();
        let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
        let result =
            ResolvedTransactionView::try_new(view, Some(loaded_addresses), &HashSet::default());
        assert!(matches!(
            result,
            Err(TransactionViewError::AddressLookupMismatch)
        ));
    }

    #[test]
    fn test_mismatched_loaded_address_lengths() {
        // Loaded addresses only has 1 writable address, no readonly.
        // The message ATL has 1 writable and 1 readonly.
        let static_keys = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let loaded_addresses = LoadedAddresses {
            writable: vec![Pubkey::new_unique()],
            readonly: vec![],
        };
        let transaction = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(v0::Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                instructions: vec![],
                account_keys: static_keys,
                address_table_lookups: vec![MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![0],
                    readonly_indexes: vec![1],
                }],
                recent_blockhash: Hash::default(),
            }),
        };
        let bytes = bincode::serialize(&transaction).unwrap();
        let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
        let result =
            ResolvedTransactionView::try_new(view, Some(loaded_addresses), &HashSet::default());
        assert!(matches!(
            result,
            Err(TransactionViewError::AddressLookupMismatch)
        ));
    }

    #[test]
    fn test_is_writable() {
        let reserved_account_keys = HashSet::from_iter([sysvar::clock::id(), system_program::id()]);
        // Create a versioned transaction.
        let create_transaction_with_keys =
            |static_keys: Vec<Pubkey>, loaded_addresses: &LoadedAddresses| VersionedTransaction {
                signatures: vec![Signature::default()],
                message: VersionedMessage::V0(v0::Message {
                    header: MessageHeader {
                        num_required_signatures: 1,
                        num_readonly_signed_accounts: 0,
                        num_readonly_unsigned_accounts: 1,
                    },
                    account_keys: static_keys[..2].to_vec(),
                    recent_blockhash: Hash::default(),
                    instructions: vec![],
                    address_table_lookups: vec![MessageAddressTableLookup {
                        account_key: Pubkey::new_unique(),
                        writable_indexes: (0..loaded_addresses.writable.len())
                            .map(|x| (static_keys.len() + x) as u8)
                            .collect(),
                        readonly_indexes: (0..loaded_addresses.readonly.len())
                            .map(|x| {
                                (static_keys.len() + loaded_addresses.writable.len() + x) as u8
                            })
                            .collect(),
                    }],
                }),
            };

        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        {
            let static_keys = vec![sysvar::clock::id(), key0];
            let loaded_addresses = LoadedAddresses {
                writable: vec![key1],
                readonly: vec![key2],
            };
            let transaction = create_transaction_with_keys(static_keys, &loaded_addresses);
            let bytes = bincode::serialize(&transaction).unwrap();
            let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
            let resolved_view = ResolvedTransactionView::try_new(
                view,
                Some(loaded_addresses),
                &reserved_account_keys,
            )
            .unwrap();

            // demote reserved static key to readonly
            let expected = vec![false, false, true, false];
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(resolved_view.is_writable(index), expected);
            }
        }

        {
            let static_keys = vec![system_program::id(), key0];
            let loaded_addresses = LoadedAddresses {
                writable: vec![key1],
                readonly: vec![key2],
            };
            let transaction = create_transaction_with_keys(static_keys, &loaded_addresses);
            let bytes = bincode::serialize(&transaction).unwrap();
            let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
            let resolved_view = ResolvedTransactionView::try_new(
                view,
                Some(loaded_addresses),
                &reserved_account_keys,
            )
            .unwrap();

            // demote reserved static key to readonly
            let expected = vec![false, false, true, false];
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(resolved_view.is_writable(index), expected);
            }
        }

        {
            let static_keys = vec![key0, key1];
            let loaded_addresses = LoadedAddresses {
                writable: vec![system_program::id()],
                readonly: vec![key2],
            };
            let transaction = create_transaction_with_keys(static_keys, &loaded_addresses);
            let bytes = bincode::serialize(&transaction).unwrap();
            let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
            let resolved_view = ResolvedTransactionView::try_new(
                view,
                Some(loaded_addresses),
                &reserved_account_keys,
            )
            .unwrap();

            // demote loaded key to readonly
            let expected = vec![true, false, false, false];
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(resolved_view.is_writable(index), expected);
            }
        }
    }

    #[test]
    fn test_demote_writable_program() {
        let reserved_account_keys = HashSet::default();
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let loaded_addresses = LoadedAddresses {
            writable: vec![key3, key4],
            readonly: vec![],
        };
        let create_transaction_with_static_keys =
            |static_keys: Vec<Pubkey>, loaded_addresses: &LoadedAddresses| VersionedTransaction {
                signatures: vec![Signature::default()],
                message: VersionedMessage::V0(v0::Message {
                    header: MessageHeader {
                        num_required_signatures: 1,
                        num_readonly_signed_accounts: 0,
                        num_readonly_unsigned_accounts: 0,
                    },
                    instructions: vec![CompiledInstruction {
                        program_id_index: 1,
                        accounts: vec![0],
                        data: vec![],
                    }],
                    account_keys: static_keys,
                    address_table_lookups: vec![MessageAddressTableLookup {
                        account_key: Pubkey::new_unique(),
                        writable_indexes: (0..loaded_addresses.writable.len())
                            .map(|x| x as u8)
                            .collect(),
                        readonly_indexes: (0..loaded_addresses.readonly.len())
                            .map(|x| (loaded_addresses.writable.len() + x) as u8)
                            .collect(),
                    }],
                    recent_blockhash: Hash::default(),
                }),
            };

        // Demote writable program - static
        {
            let static_keys = vec![key0, key1, key2];
            let transaction = create_transaction_with_static_keys(static_keys, &loaded_addresses);
            let bytes = bincode::serialize(&transaction).unwrap();
            let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
            let resolved_view = ResolvedTransactionView::try_new(
                view,
                Some(loaded_addresses.clone()),
                &reserved_account_keys,
            )
            .unwrap();

            let expected = vec![true, false, true, true, true];
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(resolved_view.is_writable(index), expected);
            }
        }

        // Do not demote writable program - static address: upgradable loader
        {
            let static_keys = vec![key0, key1, bpf_loader_upgradeable::ID];
            let transaction = create_transaction_with_static_keys(static_keys, &loaded_addresses);
            let bytes = bincode::serialize(&transaction).unwrap();
            let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();
            let resolved_view = ResolvedTransactionView::try_new(
                view,
                Some(loaded_addresses.clone()),
                &reserved_account_keys,
            )
            .unwrap();

            let expected = vec![true, true, true, true, true];
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(resolved_view.is_writable(index), expected);
            }
        }

        // Do not demote writable program - loaded address: upgradable loader
        {
            let static_keys = vec![key0, key1, key2];
            let loaded_addresses = LoadedAddresses {
                writable: vec![key3],
                readonly: vec![bpf_loader_upgradeable::ID],
            };
            let transaction = create_transaction_with_static_keys(static_keys, &loaded_addresses);
            let bytes = bincode::serialize(&transaction).unwrap();
            let view = SanitizedTransactionView::try_new_sanitized(bytes.as_ref()).unwrap();

            let resolved_view = ResolvedTransactionView::try_new(
                view,
                Some(loaded_addresses.clone()),
                &reserved_account_keys,
            )
            .unwrap();

            let expected = vec![true, true, true, true, false];
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(resolved_view.is_writable(index), expected);
            }
        }
    }
}
