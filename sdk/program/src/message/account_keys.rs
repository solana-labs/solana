use {
    crate::{
        instruction::{CompiledInstruction, Instruction},
        message::{v0::LoadedAddresses, CompileError},
        pubkey::Pubkey,
    },
    std::{collections::BTreeMap, ops::Index},
};

/// Collection of static and dynamically loaded keys used to load accounts
/// during transaction processing.
pub struct AccountKeys<'a> {
    static_keys: &'a [Pubkey],
    dynamic_keys: Option<&'a LoadedAddresses>,
}

impl Index<usize> for AccountKeys<'_> {
    type Output = Pubkey;
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("index is invalid")
    }
}

impl<'a> AccountKeys<'a> {
    pub fn new(static_keys: &'a [Pubkey], dynamic_keys: Option<&'a LoadedAddresses>) -> Self {
        Self {
            static_keys,
            dynamic_keys,
        }
    }

    /// Returns an iterator of account key segments. The ordering of segments
    /// affects how account indexes from compiled instructions are resolved and
    /// so should not be changed.
    fn key_segment_iter(&self) -> impl Iterator<Item = &'a [Pubkey]> {
        if let Some(dynamic_keys) = self.dynamic_keys {
            [
                self.static_keys,
                &dynamic_keys.writable,
                &dynamic_keys.readonly,
            ]
            .into_iter()
        } else {
            // empty segments added for branch type compatibility
            [self.static_keys, &[], &[]].into_iter()
        }
    }

    /// Returns the address of the account at the specified index of the list of
    /// message account keys constructed from static keys, followed by dynamically
    /// loaded writable addresses, and lastly the list of dynamically loaded
    /// readonly addresses.
    pub fn get(&self, mut index: usize) -> Option<&'a Pubkey> {
        for key_segment in self.key_segment_iter() {
            if index < key_segment.len() {
                return Some(&key_segment[index]);
            }
            index = index.saturating_sub(key_segment.len());
        }

        None
    }

    /// Returns the total length of loaded accounts for a message
    pub fn len(&self) -> usize {
        let mut len = 0usize;
        for key_segment in self.key_segment_iter() {
            len = len.saturating_add(key_segment.len());
        }
        len
    }

    /// Returns true if this collection of account keys is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterator for the addresses of the loaded accounts for a message
    pub fn iter(&self) -> impl Iterator<Item = &'a Pubkey> {
        self.key_segment_iter().flatten()
    }

    /// Compile instructions using the order of account keys to determine
    /// compiled instruction account indexes.
    ///
    /// # Panics
    ///
    /// Panics when compiling fails. See [`AccountKeys::try_compile_instructions`]
    /// for a full description of failure scenarios.
    pub fn compile_instructions(&self, instructions: &[Instruction]) -> Vec<CompiledInstruction> {
        self.try_compile_instructions(instructions)
            .expect("compilation failure")
    }

    /// Compile instructions using the order of account keys to determine
    /// compiled instruction account indexes.
    ///
    /// # Errors
    ///
    /// Compilation will fail if any `instructions` use account keys which are not
    /// present in this account key collection.
    ///
    /// Compilation will fail if any `instructions` use account keys which are located
    /// at an index which cannot be cast to a `u8` without overflow.
    pub fn try_compile_instructions(
        &self,
        instructions: &[Instruction],
    ) -> Result<Vec<CompiledInstruction>, CompileError> {
        let mut account_index_map = BTreeMap::<&Pubkey, u8>::new();
        for (index, key) in self.iter().enumerate() {
            let index = u8::try_from(index).map_err(|_| CompileError::AccountIndexOverflow)?;
            account_index_map.insert(key, index);
        }

        let get_account_index = |key: &Pubkey| -> Result<u8, CompileError> {
            account_index_map
                .get(key)
                .cloned()
                .ok_or(CompileError::UnknownInstructionKey(*key))
        };

        instructions
            .iter()
            .map(|ix| {
                let accounts: Vec<u8> = ix
                    .accounts
                    .iter()
                    .map(|account_meta| get_account_index(&account_meta.pubkey))
                    .collect::<Result<Vec<u8>, CompileError>>()?;

                Ok(CompiledInstruction {
                    program_id_index: get_account_index(&ix.program_id)?,
                    data: ix.data.clone(),
                    accounts,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::instruction::AccountMeta};

    fn test_account_keys() -> [Pubkey; 6] {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        [key0, key1, key2, key3, key4, key5]
    }

    #[test]
    fn test_key_segment_iter() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2]];
        let dynamic_keys = LoadedAddresses {
            writable: vec![keys[3], keys[4]],
            readonly: vec![keys[5]],
        };
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));

        let expected_segments = vec![
            vec![keys[0], keys[1], keys[2]],
            vec![keys[3], keys[4]],
            vec![keys[5]],
        ];

        assert!(account_keys.key_segment_iter().eq(expected_segments.iter()));
    }

    #[test]
    fn test_len() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2], keys[3], keys[4], keys[5]];
        let account_keys = AccountKeys::new(&static_keys, None);

        assert_eq!(account_keys.len(), keys.len());
    }

    #[test]
    fn test_len_with_dynamic_keys() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2]];
        let dynamic_keys = LoadedAddresses {
            writable: vec![keys[3], keys[4]],
            readonly: vec![keys[5]],
        };
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));

        assert_eq!(account_keys.len(), keys.len());
    }

    #[test]
    fn test_iter() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2], keys[3], keys[4], keys[5]];
        let account_keys = AccountKeys::new(&static_keys, None);

        assert!(account_keys.iter().eq(keys.iter()));
    }

    #[test]
    fn test_iter_with_dynamic_keys() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2]];
        let dynamic_keys = LoadedAddresses {
            writable: vec![keys[3], keys[4]],
            readonly: vec![keys[5]],
        };
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));

        assert!(account_keys.iter().eq(keys.iter()));
    }

    #[test]
    fn test_get() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2], keys[3]];
        let account_keys = AccountKeys::new(&static_keys, None);

        assert_eq!(account_keys.get(0), Some(&keys[0]));
        assert_eq!(account_keys.get(1), Some(&keys[1]));
        assert_eq!(account_keys.get(2), Some(&keys[2]));
        assert_eq!(account_keys.get(3), Some(&keys[3]));
        assert_eq!(account_keys.get(4), None);
        assert_eq!(account_keys.get(5), None);
    }

    #[test]
    fn test_get_with_dynamic_keys() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0], keys[1], keys[2]];
        let dynamic_keys = LoadedAddresses {
            writable: vec![keys[3], keys[4]],
            readonly: vec![keys[5]],
        };
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));

        assert_eq!(account_keys.get(0), Some(&keys[0]));
        assert_eq!(account_keys.get(1), Some(&keys[1]));
        assert_eq!(account_keys.get(2), Some(&keys[2]));
        assert_eq!(account_keys.get(3), Some(&keys[3]));
        assert_eq!(account_keys.get(4), Some(&keys[4]));
        assert_eq!(account_keys.get(5), Some(&keys[5]));
    }

    #[test]
    fn test_try_compile_instructions() {
        let keys = test_account_keys();

        let static_keys = vec![keys[0]];
        let dynamic_keys = LoadedAddresses {
            writable: vec![keys[1]],
            readonly: vec![keys[2]],
        };
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));

        let instruction = Instruction {
            program_id: keys[0],
            accounts: vec![
                AccountMeta::new(keys[1], true),
                AccountMeta::new(keys[2], true),
            ],
            data: vec![0],
        };

        assert_eq!(
            account_keys.try_compile_instructions(&[instruction]),
            Ok(vec![CompiledInstruction {
                program_id_index: 0,
                accounts: vec![1, 2],
                data: vec![0],
            }]),
        );
    }

    #[test]
    fn test_try_compile_instructions_with_unknown_key() {
        let static_keys = test_account_keys();
        let account_keys = AccountKeys::new(&static_keys, None);

        let unknown_key = Pubkey::new_unique();
        let test_instructions = [
            Instruction {
                program_id: unknown_key,
                accounts: vec![],
                data: vec![],
            },
            Instruction {
                program_id: static_keys[0],
                accounts: vec![
                    AccountMeta::new(static_keys[1], false),
                    AccountMeta::new(unknown_key, false),
                ],
                data: vec![],
            },
        ];

        for ix in test_instructions {
            assert_eq!(
                account_keys.try_compile_instructions(&[ix]),
                Err(CompileError::UnknownInstructionKey(unknown_key))
            );
        }
    }

    #[test]
    fn test_try_compile_instructions_with_too_many_account_keys() {
        const MAX_LENGTH_WITHOUT_OVERFLOW: usize = u8::MAX as usize + 1;
        let static_keys = vec![Pubkey::default(); MAX_LENGTH_WITHOUT_OVERFLOW];
        let dynamic_keys = LoadedAddresses {
            writable: vec![Pubkey::default()],
            readonly: vec![],
        };
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));
        assert_eq!(
            account_keys.try_compile_instructions(&[]),
            Err(CompileError::AccountIndexOverflow)
        );
    }
}
