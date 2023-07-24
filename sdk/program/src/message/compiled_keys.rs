#[cfg(not(target_os = "solana"))]
use crate::{
    address_lookup_table_account::AddressLookupTableAccount,
    message::v0::{LoadedAddresses, MessageAddressTableLookup},
};
use {
    crate::{instruction::Instruction, message::MessageHeader, pubkey::Pubkey},
    std::collections::BTreeMap,
    thiserror::Error,
};

/// A helper struct to collect pubkeys compiled for a set of instructions
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompiledKeys {
    payer: Option<Pubkey>,
    key_meta_map: BTreeMap<Pubkey, CompiledKeyMeta>,
}

#[cfg_attr(target_os = "solana", allow(dead_code))]
#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum CompileError {
    #[error("account index overflowed during compilation")]
    AccountIndexOverflow,
    #[error("address lookup table index overflowed during compilation")]
    AddressTableLookupIndexOverflow,
    #[error("encountered unknown account key `{0}` during instruction compilation")]
    UnknownInstructionKey(Pubkey),
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct CompiledKeyMeta {
    is_signer: bool,
    is_writable: bool,
    is_invoked: bool,
}

impl CompiledKeys {
    /// Compiles the pubkeys referenced by a list of instructions and organizes by
    /// signer/non-signer and writable/readonly.
    pub(crate) fn compile(instructions: &[Instruction], payer: Option<Pubkey>) -> Self {
        let mut key_meta_map = BTreeMap::<Pubkey, CompiledKeyMeta>::new();
        for ix in instructions {
            let meta = key_meta_map.entry(ix.program_id).or_default();
            meta.is_invoked = true;
            for account_meta in &ix.accounts {
                let meta = key_meta_map.entry(account_meta.pubkey).or_default();
                meta.is_signer |= account_meta.is_signer;
                meta.is_writable |= account_meta.is_writable;
            }
        }
        if let Some(payer) = &payer {
            let meta = key_meta_map.entry(*payer).or_default();
            meta.is_signer = true;
            meta.is_writable = true;
        }
        Self {
            payer,
            key_meta_map,
        }
    }

    pub(crate) fn try_into_message_components(
        self,
    ) -> Result<(MessageHeader, Vec<Pubkey>), CompileError> {
        let try_into_u8 = |num: usize| -> Result<u8, CompileError> {
            u8::try_from(num).map_err(|_| CompileError::AccountIndexOverflow)
        };

        let Self {
            payer,
            mut key_meta_map,
        } = self;

        if let Some(payer) = &payer {
            key_meta_map.remove_entry(payer);
        }

        let writable_signer_keys: Vec<Pubkey> = payer
            .into_iter()
            .chain(
                key_meta_map
                    .iter()
                    .filter_map(|(key, meta)| (meta.is_signer && meta.is_writable).then_some(*key)),
            )
            .collect();
        let readonly_signer_keys: Vec<Pubkey> = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (meta.is_signer && !meta.is_writable).then_some(*key))
            .collect();
        let writable_non_signer_keys: Vec<Pubkey> = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (!meta.is_signer && meta.is_writable).then_some(*key))
            .collect();
        let readonly_non_signer_keys: Vec<Pubkey> = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (!meta.is_signer && !meta.is_writable).then_some(*key))
            .collect();

        let signers_len = writable_signer_keys
            .len()
            .saturating_add(readonly_signer_keys.len());

        let header = MessageHeader {
            num_required_signatures: try_into_u8(signers_len)?,
            num_readonly_signed_accounts: try_into_u8(readonly_signer_keys.len())?,
            num_readonly_unsigned_accounts: try_into_u8(readonly_non_signer_keys.len())?,
        };

        let static_account_keys = std::iter::empty()
            .chain(writable_signer_keys)
            .chain(readonly_signer_keys)
            .chain(writable_non_signer_keys)
            .chain(readonly_non_signer_keys)
            .collect();

        Ok((header, static_account_keys))
    }

    #[cfg(not(target_os = "solana"))]
    pub(crate) fn try_extract_table_lookup(
        &mut self,
        lookup_table_account: &AddressLookupTableAccount,
    ) -> Result<Option<(MessageAddressTableLookup, LoadedAddresses)>, CompileError> {
        let (writable_indexes, drained_writable_keys) = self
            .try_drain_keys_found_in_lookup_table(&lookup_table_account.addresses, |meta| {
                !meta.is_signer && !meta.is_invoked && meta.is_writable
            })?;
        let (readonly_indexes, drained_readonly_keys) = self
            .try_drain_keys_found_in_lookup_table(&lookup_table_account.addresses, |meta| {
                !meta.is_signer && !meta.is_invoked && !meta.is_writable
            })?;

        // Don't extract lookup if no keys were found
        if writable_indexes.is_empty() && readonly_indexes.is_empty() {
            return Ok(None);
        }

        Ok(Some((
            MessageAddressTableLookup {
                account_key: lookup_table_account.key,
                writable_indexes,
                readonly_indexes,
            },
            LoadedAddresses {
                writable: drained_writable_keys,
                readonly: drained_readonly_keys,
            },
        )))
    }

    #[cfg(not(target_os = "solana"))]
    fn try_drain_keys_found_in_lookup_table(
        &mut self,
        lookup_table_addresses: &[Pubkey],
        key_meta_filter: impl Fn(&CompiledKeyMeta) -> bool,
    ) -> Result<(Vec<u8>, Vec<Pubkey>), CompileError> {
        let mut lookup_table_indexes = Vec::new();
        let mut drained_keys = Vec::new();

        for search_key in self
            .key_meta_map
            .iter()
            .filter_map(|(key, meta)| key_meta_filter(meta).then_some(key))
        {
            for (key_index, key) in lookup_table_addresses.iter().enumerate() {
                if key == search_key {
                    let lookup_table_index = u8::try_from(key_index)
                        .map_err(|_| CompileError::AddressTableLookupIndexOverflow)?;

                    lookup_table_indexes.push(lookup_table_index);
                    drained_keys.push(*search_key);
                    break;
                }
            }
        }

        for key in &drained_keys {
            self.key_meta_map.remove_entry(key);
        }

        Ok((lookup_table_indexes, drained_keys))
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::instruction::AccountMeta, bitflags::bitflags};

    bitflags! {
        #[derive(Clone, Copy)]
        pub struct KeyFlags: u8 {
            const SIGNER   = 0b00000001;
            const WRITABLE = 0b00000010;
            const INVOKED  = 0b00000100;
        }
    }

    impl From<KeyFlags> for CompiledKeyMeta {
        fn from(flags: KeyFlags) -> Self {
            Self {
                is_signer: flags.contains(KeyFlags::SIGNER),
                is_writable: flags.contains(KeyFlags::WRITABLE),
                is_invoked: flags.contains(KeyFlags::INVOKED),
            }
        }
    }

    #[test]
    fn test_compile_with_dups() {
        let program_id0 = Pubkey::new_unique();
        let program_id1 = Pubkey::new_unique();
        let program_id2 = Pubkey::new_unique();
        let program_id3 = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let id1 = Pubkey::new_unique();
        let id2 = Pubkey::new_unique();
        let id3 = Pubkey::new_unique();
        let compiled_keys = CompiledKeys::compile(
            &[
                Instruction::new_with_bincode(
                    program_id0,
                    &0,
                    vec![
                        AccountMeta::new_readonly(id0, false),
                        AccountMeta::new_readonly(id1, true),
                        AccountMeta::new(id2, false),
                        AccountMeta::new(id3, true),
                        // duplicate the account inputs
                        AccountMeta::new_readonly(id0, false),
                        AccountMeta::new_readonly(id1, true),
                        AccountMeta::new(id2, false),
                        AccountMeta::new(id3, true),
                        // reference program ids
                        AccountMeta::new_readonly(program_id0, false),
                        AccountMeta::new_readonly(program_id1, true),
                        AccountMeta::new(program_id2, false),
                        AccountMeta::new(program_id3, true),
                    ],
                ),
                Instruction::new_with_bincode(program_id1, &0, vec![]),
                Instruction::new_with_bincode(program_id2, &0, vec![]),
                Instruction::new_with_bincode(program_id3, &0, vec![]),
            ],
            None,
        );

        assert_eq!(
            compiled_keys,
            CompiledKeys {
                payer: None,
                key_meta_map: BTreeMap::from([
                    (id0, KeyFlags::empty().into()),
                    (id1, KeyFlags::SIGNER.into()),
                    (id2, KeyFlags::WRITABLE.into()),
                    (id3, (KeyFlags::SIGNER | KeyFlags::WRITABLE).into()),
                    (program_id0, KeyFlags::INVOKED.into()),
                    (program_id1, (KeyFlags::INVOKED | KeyFlags::SIGNER).into()),
                    (program_id2, (KeyFlags::INVOKED | KeyFlags::WRITABLE).into()),
                    (program_id3, KeyFlags::all().into()),
                ]),
            }
        );
    }

    #[test]
    fn test_compile_with_dup_payer() {
        let program_id = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let compiled_keys = CompiledKeys::compile(
            &[Instruction::new_with_bincode(
                program_id,
                &0,
                vec![AccountMeta::new_readonly(payer, false)],
            )],
            Some(payer),
        );
        assert_eq!(
            compiled_keys,
            CompiledKeys {
                payer: Some(payer),
                key_meta_map: BTreeMap::from([
                    (payer, (KeyFlags::SIGNER | KeyFlags::WRITABLE).into()),
                    (program_id, KeyFlags::INVOKED.into()),
                ]),
            }
        );
    }

    #[test]
    fn test_compile_with_dup_signer_mismatch() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let compiled_keys = CompiledKeys::compile(
            &[Instruction::new_with_bincode(
                program_id,
                &0,
                vec![AccountMeta::new(id0, false), AccountMeta::new(id0, true)],
            )],
            None,
        );

        // Ensure the dup writable key is a signer
        assert_eq!(
            compiled_keys,
            CompiledKeys {
                payer: None,
                key_meta_map: BTreeMap::from([
                    (id0, (KeyFlags::SIGNER | KeyFlags::WRITABLE).into()),
                    (program_id, KeyFlags::INVOKED.into()),
                ]),
            }
        );
    }

    #[test]
    fn test_compile_with_dup_signer_writable_mismatch() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let compiled_keys = CompiledKeys::compile(
            &[Instruction::new_with_bincode(
                program_id,
                &0,
                vec![
                    AccountMeta::new_readonly(id0, true),
                    AccountMeta::new(id0, true),
                ],
            )],
            None,
        );

        // Ensure the dup signer key is writable
        assert_eq!(
            compiled_keys,
            CompiledKeys {
                payer: None,
                key_meta_map: BTreeMap::from([
                    (id0, (KeyFlags::SIGNER | KeyFlags::WRITABLE).into()),
                    (program_id, KeyFlags::INVOKED.into()),
                ]),
            }
        );
    }

    #[test]
    fn test_compile_with_dup_nonsigner_writable_mismatch() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let compiled_keys = CompiledKeys::compile(
            &[
                Instruction::new_with_bincode(
                    program_id,
                    &0,
                    vec![
                        AccountMeta::new_readonly(id0, false),
                        AccountMeta::new(id0, false),
                    ],
                ),
                Instruction::new_with_bincode(program_id, &0, vec![AccountMeta::new(id0, false)]),
            ],
            None,
        );

        // Ensure the dup nonsigner key is writable
        assert_eq!(
            compiled_keys,
            CompiledKeys {
                payer: None,
                key_meta_map: BTreeMap::from([
                    (id0, KeyFlags::WRITABLE.into()),
                    (program_id, KeyFlags::INVOKED.into()),
                ]),
            }
        );
    }

    #[test]
    fn test_try_into_message_components() {
        let keys = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from([
                (keys[0], (KeyFlags::SIGNER | KeyFlags::WRITABLE).into()),
                (keys[1], KeyFlags::SIGNER.into()),
                (keys[2], KeyFlags::WRITABLE.into()),
                (keys[3], KeyFlags::empty().into()),
            ]),
        };

        let result = compiled_keys.try_into_message_components();
        assert_eq!(result.as_ref().err(), None);
        let (header, static_keys) = result.unwrap();

        assert_eq!(static_keys, keys);
        assert_eq!(
            header,
            MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            }
        );
    }

    #[test]
    fn test_try_into_message_components_with_too_many_keys() {
        const TOO_MANY_KEYS: usize = 257;

        for key_flags in [
            KeyFlags::WRITABLE | KeyFlags::SIGNER,
            KeyFlags::SIGNER,
            // skip writable_non_signer_keys because it isn't used for creating header values
            KeyFlags::empty(),
        ] {
            let test_keys = CompiledKeys {
                payer: None,
                key_meta_map: BTreeMap::from_iter(
                    (0..TOO_MANY_KEYS).map(|_| (Pubkey::new_unique(), key_flags.into())),
                ),
            };

            assert_eq!(
                test_keys.try_into_message_components(),
                Err(CompileError::AccountIndexOverflow)
            );
        }
    }

    #[test]
    fn test_try_extract_table_lookup() {
        let keys = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let mut compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from([
                (keys[0], (KeyFlags::SIGNER | KeyFlags::WRITABLE).into()),
                (keys[1], KeyFlags::SIGNER.into()),
                (keys[2], KeyFlags::WRITABLE.into()),
                (keys[3], KeyFlags::empty().into()),
                (keys[4], (KeyFlags::INVOKED | KeyFlags::WRITABLE).into()),
                (keys[5], (KeyFlags::INVOKED).into()),
            ]),
        };

        // add some duplicates to ensure lowest index is selected
        let addresses = [keys.clone(), keys.clone()].concat();
        let lookup_table_account = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses,
        };

        assert_eq!(
            compiled_keys.try_extract_table_lookup(&lookup_table_account),
            Ok(Some((
                MessageAddressTableLookup {
                    account_key: lookup_table_account.key,
                    writable_indexes: vec![2],
                    readonly_indexes: vec![3],
                },
                LoadedAddresses {
                    writable: vec![keys[2]],
                    readonly: vec![keys[3]],
                },
            )))
        );

        assert_eq!(compiled_keys.key_meta_map.len(), 4);
        assert!(!compiled_keys.key_meta_map.contains_key(&keys[2]));
        assert!(!compiled_keys.key_meta_map.contains_key(&keys[3]));
    }

    #[test]
    fn test_try_extract_table_lookup_returns_none() {
        let mut compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from([
                (Pubkey::new_unique(), KeyFlags::WRITABLE.into()),
                (Pubkey::new_unique(), KeyFlags::empty().into()),
            ]),
        };

        let lookup_table_account = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![],
        };

        let expected_compiled_keys = compiled_keys.clone();
        assert_eq!(
            compiled_keys.try_extract_table_lookup(&lookup_table_account),
            Ok(None)
        );
        assert_eq!(compiled_keys, expected_compiled_keys);
    }

    #[test]
    fn test_try_extract_table_lookup_for_invalid_table() {
        let writable_key = Pubkey::new_unique();
        let mut compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from([
                (writable_key, KeyFlags::WRITABLE.into()),
                (Pubkey::new_unique(), KeyFlags::empty().into()),
            ]),
        };

        const MAX_LENGTH_WITHOUT_OVERFLOW: usize = u8::MAX as usize + 1;
        let mut addresses = vec![Pubkey::default(); MAX_LENGTH_WITHOUT_OVERFLOW];
        addresses.push(writable_key);

        let lookup_table_account = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses,
        };

        let expected_compiled_keys = compiled_keys.clone();
        assert_eq!(
            compiled_keys.try_extract_table_lookup(&lookup_table_account),
            Err(CompileError::AddressTableLookupIndexOverflow),
        );
        assert_eq!(compiled_keys, expected_compiled_keys);
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table() {
        let orig_keys = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let mut compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from([
                (orig_keys[0], KeyFlags::empty().into()),
                (orig_keys[1], KeyFlags::WRITABLE.into()),
                (orig_keys[2], KeyFlags::WRITABLE.into()),
                (orig_keys[3], KeyFlags::empty().into()),
                (orig_keys[4], KeyFlags::empty().into()),
            ]),
        };

        let lookup_table_addresses = vec![
            Pubkey::new_unique(),
            orig_keys[0],
            Pubkey::new_unique(),
            orig_keys[4],
            Pubkey::new_unique(),
            orig_keys[2],
            Pubkey::new_unique(),
        ];

        let drain_result = compiled_keys
            .try_drain_keys_found_in_lookup_table(&lookup_table_addresses, |meta| {
                !meta.is_writable
            });
        assert_eq!(drain_result.as_ref().err(), None);
        let (lookup_table_indexes, drained_keys) = drain_result.unwrap();

        assert_eq!(
            compiled_keys.key_meta_map.keys().collect::<Vec<&_>>(),
            vec![&orig_keys[1], &orig_keys[2], &orig_keys[3]]
        );
        assert_eq!(drained_keys, vec![orig_keys[0], orig_keys[4]]);
        assert_eq!(lookup_table_indexes, vec![1, 3]);
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table_with_empty_keys() {
        let mut compiled_keys = CompiledKeys::default();

        let lookup_table_addresses = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let drain_result =
            compiled_keys.try_drain_keys_found_in_lookup_table(&lookup_table_addresses, |_| true);
        assert_eq!(drain_result.as_ref().err(), None);
        let (lookup_table_indexes, drained_keys) = drain_result.unwrap();

        assert!(drained_keys.is_empty());
        assert!(lookup_table_indexes.is_empty());
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table_with_empty_table() {
        let original_keys = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let mut compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from_iter(
                original_keys
                    .iter()
                    .map(|key| (*key, CompiledKeyMeta::default())),
            ),
        };

        let lookup_table_addresses = vec![];

        let drain_result =
            compiled_keys.try_drain_keys_found_in_lookup_table(&lookup_table_addresses, |_| true);
        assert_eq!(drain_result.as_ref().err(), None);
        let (lookup_table_indexes, drained_keys) = drain_result.unwrap();

        assert_eq!(compiled_keys.key_meta_map.len(), original_keys.len());
        assert!(drained_keys.is_empty());
        assert!(lookup_table_indexes.is_empty());
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table_with_too_many_addresses() {
        let key = Pubkey::new_unique();
        let mut compiled_keys = CompiledKeys {
            payer: None,
            key_meta_map: BTreeMap::from([(key, CompiledKeyMeta::default())]),
        };

        const MAX_LENGTH_WITHOUT_OVERFLOW: usize = u8::MAX as usize + 1;
        let mut lookup_table_addresses = vec![Pubkey::default(); MAX_LENGTH_WITHOUT_OVERFLOW];
        lookup_table_addresses.push(key);

        let drain_result =
            compiled_keys.try_drain_keys_found_in_lookup_table(&lookup_table_addresses, |_| true);
        assert_eq!(
            drain_result.err(),
            Some(CompileError::AddressTableLookupIndexOverflow)
        );
    }
}
