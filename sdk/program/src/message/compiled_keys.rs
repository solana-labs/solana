#[cfg(not(target_arch = "bpf"))]
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
    writable_signer_keys: Vec<Pubkey>,
    readonly_signer_keys: Vec<Pubkey>,
    writable_non_signer_keys: Vec<Pubkey>,
    readonly_non_signer_keys: Vec<Pubkey>,
}

#[cfg_attr(target_arch = "bpf", allow(dead_code))]
#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum CompileError {
    #[error("account index overflowed during compilation")]
    AccountIndexOverflow,
    #[error("address lookup table index overflowed during compilation")]
    AddressTableLookupIndexOverflow,
    #[error("encountered unknown account key `{0}` during instruction compilation")]
    UnknownInstructionKey(Pubkey),
}

#[derive(Default, Debug)]
struct CompiledKeyMeta {
    is_signer: bool,
    is_writable: bool,
}

impl CompiledKeys {
    /// Compiles the pubkeys referenced by a list of instructions and organizes by
    /// signer/non-signer and writable/readonly.
    pub(crate) fn compile(instructions: &[Instruction], payer: Option<Pubkey>) -> Self {
        let mut key_meta_map = BTreeMap::<&Pubkey, CompiledKeyMeta>::new();
        for ix in instructions {
            key_meta_map.entry(&ix.program_id).or_default();
            for account_meta in &ix.accounts {
                let meta = key_meta_map.entry(&account_meta.pubkey).or_default();
                meta.is_signer |= account_meta.is_signer;
                meta.is_writable |= account_meta.is_writable;
            }
        }

        if let Some(payer) = &payer {
            key_meta_map.remove(payer);
        }

        let writable_signer_keys: Vec<Pubkey> = payer
            .into_iter()
            .chain(
                key_meta_map
                    .iter()
                    .filter_map(|(key, meta)| (meta.is_signer && meta.is_writable).then(|| **key)),
            )
            .collect();
        let readonly_signer_keys = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (meta.is_signer && !meta.is_writable).then(|| **key))
            .collect();
        let writable_non_signer_keys = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (!meta.is_signer && meta.is_writable).then(|| **key))
            .collect();
        let readonly_non_signer_keys = key_meta_map
            .iter()
            .filter_map(|(key, meta)| (!meta.is_signer && !meta.is_writable).then(|| **key))
            .collect();

        CompiledKeys {
            writable_signer_keys,
            readonly_signer_keys,
            writable_non_signer_keys,
            readonly_non_signer_keys,
        }
    }

    pub(crate) fn try_into_message_components(
        self,
    ) -> Result<(MessageHeader, Vec<Pubkey>), CompileError> {
        let try_into_u8 = |num: usize| -> Result<u8, CompileError> {
            u8::try_from(num).map_err(|_| CompileError::AccountIndexOverflow)
        };

        let signers_len = self
            .writable_signer_keys
            .len()
            .saturating_add(self.readonly_signer_keys.len());

        let header = MessageHeader {
            num_required_signatures: try_into_u8(signers_len)?,
            num_readonly_signed_accounts: try_into_u8(self.readonly_signer_keys.len())?,
            num_readonly_unsigned_accounts: try_into_u8(self.readonly_non_signer_keys.len())?,
        };

        let static_account_keys = std::iter::empty()
            .chain(self.writable_signer_keys)
            .chain(self.readonly_signer_keys)
            .chain(self.writable_non_signer_keys)
            .chain(self.readonly_non_signer_keys)
            .collect();

        Ok((header, static_account_keys))
    }

    #[cfg(not(target_arch = "bpf"))]
    pub(crate) fn try_extract_table_lookup(
        &mut self,
        lookup_table_account: &AddressLookupTableAccount,
    ) -> Result<Option<(MessageAddressTableLookup, LoadedAddresses)>, CompileError> {
        let (writable_indexes, drained_writable_keys) = try_drain_keys_found_in_lookup_table(
            &mut self.writable_non_signer_keys,
            &lookup_table_account.addresses,
        )?;
        let (readonly_indexes, drained_readonly_keys) = try_drain_keys_found_in_lookup_table(
            &mut self.readonly_non_signer_keys,
            &lookup_table_account.addresses,
        )?;

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
}

#[cfg_attr(target_arch = "bpf", allow(dead_code))]
fn try_drain_keys_found_in_lookup_table(
    keys: &mut Vec<Pubkey>,
    lookup_table_addresses: &[Pubkey],
) -> Result<(Vec<u8>, Vec<Pubkey>), CompileError> {
    let mut lookup_table_indexes = Vec::new();
    let mut drained_keys = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        let search_key = &keys[i];
        let mut lookup_table_index = None;
        for (key_index, key) in lookup_table_addresses.iter().enumerate() {
            if key == search_key {
                lookup_table_index = Some(
                    u8::try_from(key_index)
                        .map_err(|_| CompileError::AddressTableLookupIndexOverflow)?,
                );
                break;
            }
        }

        if let Some(index) = lookup_table_index {
            lookup_table_indexes.push(index);
            drained_keys.push(keys.remove(i));
        } else {
            i = i.saturating_add(1);
        }
    }
    Ok((lookup_table_indexes, drained_keys))
}

#[cfg(test)]
mod tests {
    use {super::*, crate::instruction::AccountMeta};

    #[test]
    fn test_compile_with_dups() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let id1 = Pubkey::new_unique();
        let id2 = Pubkey::new_unique();
        let keys = CompiledKeys::compile(
            &[Instruction::new_with_bincode(
                program_id,
                &0,
                vec![
                    AccountMeta::new(id0, true),
                    AccountMeta::new_readonly(id1, true),
                    AccountMeta::new(id2, false),
                    // duplicate the account inputs
                    AccountMeta::new(id0, true),
                    AccountMeta::new_readonly(id1, true),
                    AccountMeta::new(id2, false),
                ],
            )],
            None,
        );
        assert_eq!(
            keys,
            CompiledKeys {
                writable_signer_keys: vec![id0],
                readonly_signer_keys: vec![id1],
                writable_non_signer_keys: vec![id2],
                readonly_non_signer_keys: vec![program_id],
            }
        );
    }

    #[test]
    fn test_compile_with_dup_payer() {
        let program_id = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let keys = CompiledKeys::compile(
            &[Instruction::new_with_bincode(
                program_id,
                &0,
                vec![AccountMeta::new_readonly(payer, false)],
            )],
            Some(payer),
        );
        assert_eq!(
            keys,
            CompiledKeys {
                writable_signer_keys: vec![payer],
                readonly_non_signer_keys: vec![program_id],
                ..CompiledKeys::default()
            }
        );
    }

    #[test]
    fn test_compile_with_dup_signer_mismatch() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let keys = CompiledKeys::compile(
            &[Instruction::new_with_bincode(
                program_id,
                &0,
                vec![AccountMeta::new(id0, false), AccountMeta::new(id0, true)],
            )],
            None,
        );

        // Ensure the dup writable key is a signer
        assert_eq!(
            keys,
            CompiledKeys {
                writable_signer_keys: vec![id0],
                readonly_non_signer_keys: vec![program_id],
                ..CompiledKeys::default()
            }
        );
    }

    #[test]
    fn test_compile_with_dup_signer_writable_mismatch() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let keys = CompiledKeys::compile(
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
            keys,
            CompiledKeys {
                writable_signer_keys: vec![id0],
                readonly_non_signer_keys: vec![program_id],
                ..CompiledKeys::default()
            }
        );
    }

    #[test]
    fn test_compile_with_dup_nonsigner_writable_mismatch() {
        let program_id = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let keys = CompiledKeys::compile(
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
            keys,
            CompiledKeys {
                writable_non_signer_keys: vec![id0],
                readonly_non_signer_keys: vec![program_id],
                ..CompiledKeys::default()
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
            writable_signer_keys: vec![keys[0]],
            readonly_signer_keys: vec![keys[1]],
            writable_non_signer_keys: vec![keys[2]],
            readonly_non_signer_keys: vec![keys[3]],
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
        let too_many_keys_vec = vec![Pubkey::default(); 257];

        let mut test_keys_list = vec![CompiledKeys::default(); 3];
        test_keys_list[0]
            .writable_signer_keys
            .extend(too_many_keys_vec.clone());
        test_keys_list[1]
            .readonly_signer_keys
            .extend(too_many_keys_vec.clone());
        // skip writable_non_signer_keys because it isn't used for creating header values
        test_keys_list[2]
            .readonly_non_signer_keys
            .extend(too_many_keys_vec);

        for test_keys in test_keys_list {
            assert_eq!(
                test_keys.try_into_message_components(),
                Err(CompileError::AccountIndexOverflow)
            );
        }
    }

    #[test]
    fn test_try_extract_table_lookup() {
        let writable_keys = vec![Pubkey::new_unique(), Pubkey::new_unique()];
        let readonly_keys = vec![Pubkey::new_unique(), Pubkey::new_unique()];

        let mut compiled_keys = CompiledKeys {
            writable_signer_keys: vec![writable_keys[0]],
            readonly_signer_keys: vec![readonly_keys[0]],
            writable_non_signer_keys: vec![writable_keys[1]],
            readonly_non_signer_keys: vec![readonly_keys[1]],
        };

        let lookup_table_account = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![
                writable_keys[0],
                readonly_keys[0],
                writable_keys[1],
                readonly_keys[1],
                // add some duplicates to ensure lowest index is selected
                writable_keys[1],
                readonly_keys[1],
            ],
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
                    writable: vec![writable_keys[1]],
                    readonly: vec![readonly_keys[1]],
                },
            )))
        );
    }

    #[test]
    fn test_try_extract_table_lookup_returns_none() {
        let mut compiled_keys = CompiledKeys {
            writable_non_signer_keys: vec![Pubkey::new_unique()],
            readonly_non_signer_keys: vec![Pubkey::new_unique()],
            ..CompiledKeys::default()
        };

        let lookup_table_account = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![],
        };

        assert_eq!(
            compiled_keys.try_extract_table_lookup(&lookup_table_account),
            Ok(None)
        );
    }

    #[test]
    fn test_try_extract_table_lookup_for_invalid_table() {
        let mut compiled_keys = CompiledKeys {
            writable_non_signer_keys: vec![Pubkey::new_unique()],
            readonly_non_signer_keys: vec![Pubkey::new_unique()],
            ..CompiledKeys::default()
        };

        const MAX_LENGTH_WITHOUT_OVERFLOW: usize = u8::MAX as usize + 1;
        let mut addresses = vec![Pubkey::default(); MAX_LENGTH_WITHOUT_OVERFLOW];
        addresses.push(compiled_keys.writable_non_signer_keys[0]);

        let lookup_table_account = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses,
        };

        assert_eq!(
            compiled_keys.try_extract_table_lookup(&lookup_table_account),
            Err(CompileError::AddressTableLookupIndexOverflow),
        );
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table() {
        let orig_keys = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let lookup_table_addresses = vec![
            Pubkey::new_unique(),
            orig_keys[0],
            Pubkey::new_unique(),
            orig_keys[4],
            Pubkey::new_unique(),
            orig_keys[2],
            Pubkey::new_unique(),
        ];

        let mut keys = orig_keys.clone();
        let drain_result = try_drain_keys_found_in_lookup_table(&mut keys, &lookup_table_addresses);
        assert_eq!(drain_result.as_ref().err(), None);
        let (lookup_table_indexes, drained_keys) = drain_result.unwrap();

        assert_eq!(keys, vec![orig_keys[1], orig_keys[3]]);
        assert_eq!(drained_keys, vec![orig_keys[0], orig_keys[2], orig_keys[4]]);
        assert_eq!(lookup_table_indexes, vec![1, 5, 3]);
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table_with_empty_keys() {
        let mut keys = vec![];

        let lookup_table_addresses = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let drain_result = try_drain_keys_found_in_lookup_table(&mut keys, &lookup_table_addresses);
        assert_eq!(drain_result.as_ref().err(), None);
        let (lookup_table_indexes, drained_keys) = drain_result.unwrap();

        assert!(keys.is_empty());
        assert!(drained_keys.is_empty());
        assert!(lookup_table_indexes.is_empty());
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table_with_empty_table() {
        let original_keys = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];

        let lookup_table_addresses = vec![];

        let mut keys = original_keys.clone();
        let drain_result = try_drain_keys_found_in_lookup_table(&mut keys, &lookup_table_addresses);
        assert_eq!(drain_result.as_ref().err(), None);
        let (lookup_table_indexes, drained_keys) = drain_result.unwrap();

        assert_eq!(keys, original_keys);
        assert!(drained_keys.is_empty());
        assert!(lookup_table_indexes.is_empty());
    }

    #[test]
    fn test_try_drain_keys_found_in_lookup_table_with_too_many_addresses() {
        let mut keys = vec![Pubkey::new_unique()];
        const MAX_LENGTH_WITHOUT_OVERFLOW: usize = u8::MAX as usize + 1;
        let mut lookup_table_addresses = vec![Pubkey::default(); MAX_LENGTH_WITHOUT_OVERFLOW];
        lookup_table_addresses.push(keys[0]);

        let drain_result = try_drain_keys_found_in_lookup_table(&mut keys, &lookup_table_addresses);
        assert_eq!(
            drain_result.err(),
            Some(CompileError::AddressTableLookupIndexOverflow)
        );
    }
}
