use {
    crate::{
        hash::Hash,
        instruction::CompiledInstruction,
        message::{
            legacy::Message as LegacyMessage,
            v0::{self, LoadedAddresses},
            AccountKeys, MessageHeader,
        },
        nonce::NONCED_TX_MARKER_IX_INDEX,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        solana_program::{system_instruction::SystemInstruction, system_program},
        sysvar::instructions::{BorrowedAccountMeta, BorrowedInstruction},
    },
    std::collections::HashSet,
    std::convert::TryFrom,
    thiserror::Error,
};

/// Sanitized message of a transaction.
#[derive(Debug, Clone)]
pub enum SanitizedMessage {
    /// Sanitized legacy message
    Legacy(LegacyMessage),
    /// Sanitized version #0 message with dynamically loaded addresses
    V0(v0::LoadedMessage<'static>),
    ///
    SanitizedWithWritableAccountsIndexCache {
        sanitized_message: Box<SanitizedMessage>,
        writable_accounts_cache: HashSet<usize>,
    },
}

#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum SanitizeMessageError {
    #[error("index out of bounds")]
    IndexOutOfBounds,
    #[error("value out of bounds")]
    ValueOutOfBounds,
    #[error("invalid value")]
    InvalidValue,
}

impl From<SanitizeError> for SanitizeMessageError {
    fn from(err: SanitizeError) -> Self {
        match err {
            SanitizeError::IndexOutOfBounds => Self::IndexOutOfBounds,
            SanitizeError::ValueOutOfBounds => Self::ValueOutOfBounds,
            SanitizeError::InvalidValue => Self::InvalidValue,
        }
    }
}

impl TryFrom<LegacyMessage> for SanitizedMessage {
    type Error = SanitizeMessageError;
    fn try_from(message: LegacyMessage) -> Result<Self, Self::Error> {
        message.sanitize()?;
        Ok(Self::Legacy(message))
    }
}

impl SanitizedMessage {
    /// Return true if this message contains duplicate account keys
    pub fn has_duplicates(&self) -> bool {
        match self {
            SanitizedMessage::Legacy(message) => message.has_duplicates(),
            SanitizedMessage::V0(message) => message.has_duplicates(),
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.has_duplicates(),
        }
    }

    /// Message header which identifies the number of signer and writable or
    /// readonly accounts
    pub fn header(&self) -> &MessageHeader {
        match self {
            Self::Legacy(message) => &message.header,
            Self::V0(loaded_msg) => &loaded_msg.message.header,
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.header(),
        }
    }

    /// Returns a legacy message if this sanitized message wraps one
    pub fn legacy_message(&self) -> Option<&LegacyMessage> {
        if let Self::Legacy(message) = &self {
            Some(message)
        } else {
            None
        }
    }

    /// Returns the fee payer for the transaction
    pub fn fee_payer(&self) -> &Pubkey {
        self.account_keys()
            .get(0)
            .expect("sanitized message always has non-program fee payer at index 0")
    }

    /// The hash of a recent block, used for timing out a transaction
    pub fn recent_blockhash(&self) -> &Hash {
        match self {
            Self::Legacy(message) => &message.recent_blockhash,
            Self::V0(loaded_msg) => &loaded_msg.message.recent_blockhash,
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.recent_blockhash(),
        }
    }

    /// Program instructions that will be executed in sequence and committed in
    /// one atomic transaction if all succeed.
    pub fn instructions(&self) -> &[CompiledInstruction] {
        match self {
            Self::Legacy(message) => &message.instructions,
            Self::V0(loaded_msg) => &loaded_msg.message.instructions,
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.instructions(),
        }
    }

    /// Program instructions iterator which includes each instruction's program
    /// id.
    pub fn program_instructions_iter(
        &self,
    ) -> impl Iterator<Item = (&Pubkey, &CompiledInstruction)> {
        self.instructions().iter().map(move |ix| {
            (
                self.account_keys()
                    .get(usize::from(ix.program_id_index))
                    .expect("program id index is sanitized"),
                ix,
            )
        })
    }

    /// Returns the list of account keys that are loaded for this message.
    pub fn account_keys(&self) -> AccountKeys {
        match self {
            Self::Legacy(message) => AccountKeys::new(&message.account_keys, None),
            Self::V0(message) => message.account_keys(),
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.account_keys(),
        }
    }

    /// Returns true if the account at the specified index is an input to some
    /// program instruction in this message.
    fn is_key_passed_to_program(&self, key_index: usize) -> bool {
        if let Ok(key_index) = u8::try_from(key_index) {
            self.instructions()
                .iter()
                .any(|ix| ix.accounts.contains(&key_index))
        } else {
            false
        }
    }

    /// Returns true if the account at the specified index is invoked as a
    /// program in this message.
    pub fn is_invoked(&self, key_index: usize) -> bool {
        match self {
            Self::Legacy(message) => message.is_key_called_as_program(key_index),
            Self::V0(message) => message.is_key_called_as_program(key_index),
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.is_invoked(key_index),
        }
    }

    /// Returns true if the account at the specified index is not invoked as a
    /// program or, if invoked, is passed to a program.
    pub fn is_non_loader_key(&self, key_index: usize) -> bool {
        !self.is_invoked(key_index) || self.is_key_passed_to_program(key_index)
    }

    /// Caches writable accounts index
    fn get_writable_accounts_indexes(&self) -> HashSet<usize> {
        let account_keys = self.account_keys();
        let num_readonly_accounts = self.num_readonly_accounts();
        let num_writable_accounts = account_keys.len().saturating_sub(num_readonly_accounts);
        let mut writable_accounts = HashSet::with_capacity(num_writable_accounts);

        for (i, _key) in account_keys.iter().enumerate() {
            if self.is_writable(i) {
                writable_accounts.insert(i);
            }
        }

        writable_accounts
    }

    /// Returns true if the account at the specified index is writable by the
    /// instructions in this message.
    pub fn is_writable(&self, index: usize) -> bool {
        match self {
            Self::Legacy(message) => message.is_writable(index),
            Self::V0(message) => message.is_writable(index),
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message: _,
                writable_accounts_cache,
            } => writable_accounts_cache.contains(&index),
        }
    }

    /// Returns true if the account at the specified index signed this
    /// message.
    pub fn is_signer(&self, index: usize) -> bool {
        index < usize::from(self.header().num_required_signatures)
    }

    /// Return the resolved addresses for this message if it has any.
    fn loaded_lookup_table_addresses(&self) -> Option<&LoadedAddresses> {
        match &self {
            SanitizedMessage::V0(message) => Some(&message.loaded_addresses),
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.loaded_lookup_table_addresses(),
            _ => None,
        }
    }

    /// Return the number of readonly accounts loaded by this message.
    pub fn num_readonly_accounts(&self) -> usize {
        let loaded_readonly_addresses = self
            .loaded_lookup_table_addresses()
            .map(|keys| keys.readonly.len())
            .unwrap_or_default();
        loaded_readonly_addresses
            .saturating_add(usize::from(self.header().num_readonly_signed_accounts))
            .saturating_add(usize::from(self.header().num_readonly_unsigned_accounts))
    }

    /// Decompile message instructions without cloning account keys
    pub fn decompile_instructions(&self) -> Vec<BorrowedInstruction> {
        let account_keys = self.account_keys();
        self.program_instructions_iter()
            .map(|(program_id, instruction)| {
                let accounts = instruction
                    .accounts
                    .iter()
                    .map(|account_index| {
                        let account_index = *account_index as usize;
                        BorrowedAccountMeta {
                            is_signer: self.is_signer(account_index),
                            is_writable: self.is_writable(account_index),
                            pubkey: account_keys.get(account_index).unwrap(),
                        }
                    })
                    .collect();

                BorrowedInstruction {
                    accounts,
                    data: &instruction.data,
                    program_id,
                }
            })
            .collect()
    }

    /// Inspect all message keys for the bpf upgradeable loader
    pub fn is_upgradeable_loader_present(&self) -> bool {
        match self {
            Self::Legacy(message) => message.is_upgradeable_loader_present(),
            Self::V0(message) => message.is_upgradeable_loader_present(),
            SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                sanitized_message,
                writable_accounts_cache: _,
            } => sanitized_message.is_upgradeable_loader_present(),
        }
    }

    /// Get a list of signers for the instruction at the given index
    pub fn get_ix_signers(&self, ix_index: usize) -> impl Iterator<Item = &Pubkey> {
        self.instructions()
            .get(ix_index)
            .into_iter()
            .flat_map(|ix| {
                ix.accounts
                    .iter()
                    .copied()
                    .map(usize::from)
                    .filter(|index| self.is_signer(*index))
                    .filter_map(|signer_index| self.account_keys().get(signer_index))
            })
    }

    /// If the message uses a durable nonce, return the pubkey of the nonce account
    pub fn get_durable_nonce(&self) -> Option<&Pubkey> {
        self.instructions()
            .get(NONCED_TX_MARKER_IX_INDEX as usize)
            .filter(
                |ix| match self.account_keys().get(ix.program_id_index as usize) {
                    Some(program_id) => system_program::check_id(program_id),
                    _ => false,
                },
            )
            .filter(|ix| {
                matches!(
                    limited_deserialize(&ix.data, 4 /* serialized size of AdvanceNonceAccount */),
                    Ok(SystemInstruction::AdvanceNonceAccount)
                )
            })
            .and_then(|ix| {
                ix.accounts.first().and_then(|idx| {
                    let idx = *idx as usize;
                    if !self.is_writable(idx) {
                        None
                    } else {
                        self.account_keys().get(idx)
                    }
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::message::v0, std::collections::HashSet};

    #[test]
    fn test_try_from_message() {
        let legacy_message_with_no_signers = LegacyMessage {
            account_keys: vec![Pubkey::new_unique()],
            ..LegacyMessage::default()
        };

        assert_eq!(
            SanitizedMessage::try_from(legacy_message_with_no_signers).err(),
            Some(SanitizeMessageError::IndexOutOfBounds),
        );
    }

    #[test]
    fn test_is_non_loader_key() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let loader_key = Pubkey::new_unique();
        let instructions = vec![
            CompiledInstruction::new(1, &(), vec![0]),
            CompiledInstruction::new(2, &(), vec![0, 1]),
        ];

        let message = SanitizedMessage::try_from(LegacyMessage::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![key0, key1, loader_key],
            Hash::default(),
            instructions,
        ))
        .unwrap();

        assert!(message.is_non_loader_key(0));
        assert!(message.is_non_loader_key(1));
        assert!(!message.is_non_loader_key(2));
    }

    #[test]
    fn test_num_readonly_accounts() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        let legacy_message = SanitizedMessage::try_from(LegacyMessage {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key0, key1, key2, key3],
            ..LegacyMessage::default()
        })
        .unwrap();

        assert_eq!(legacy_message.num_readonly_accounts(), 2);

        let v0_message = SanitizedMessage::V0(v0::LoadedMessage::new(
            v0::Message {
                header: MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 1,
                },
                account_keys: vec![key0, key1, key2, key3],
                ..v0::Message::default()
            },
            LoadedAddresses {
                writable: vec![key4],
                readonly: vec![key5],
            },
        ));

        assert_eq!(v0_message.num_readonly_accounts(), 3);
    }

    #[test]
    fn test_get_ix_signers() {
        let signer0 = Pubkey::new_unique();
        let signer1 = Pubkey::new_unique();
        let non_signer = Pubkey::new_unique();
        let loader_key = Pubkey::new_unique();
        let instructions = vec![
            CompiledInstruction::new(3, &(), vec![2, 0]),
            CompiledInstruction::new(3, &(), vec![0, 1]),
            CompiledInstruction::new(3, &(), vec![0, 0]),
        ];

        let message = SanitizedMessage::try_from(LegacyMessage::new_with_compiled_instructions(
            2,
            1,
            2,
            vec![signer0, signer1, non_signer, loader_key],
            Hash::default(),
            instructions,
        ))
        .unwrap();

        assert_eq!(
            message.get_ix_signers(0).collect::<HashSet<_>>(),
            HashSet::from_iter([&signer0])
        );
        assert_eq!(
            message.get_ix_signers(1).collect::<HashSet<_>>(),
            HashSet::from_iter([&signer0, &signer1])
        );
        assert_eq!(
            message.get_ix_signers(2).collect::<HashSet<_>>(),
            HashSet::from_iter([&signer0])
        );
        assert_eq!(
            message.get_ix_signers(3).collect::<HashSet<_>>(),
            HashSet::default()
        );
    }

    #[test]
    fn test_writable_accounts_cache() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        let legacy_message = SanitizedMessage::try_from(LegacyMessage {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key0, key1, key2, key3],
            ..LegacyMessage::default()
        })
        .unwrap();
        match legacy_message {
            SanitizedMessage::Legacy(_) => {}
            _ => panic!("Expect a SanitizedMessage::Legacy"),
        }

        // assert can convert sanitized legacy message to have writable accounts cache
        {
            let writable_accounts_cache = legacy_message.get_writable_accounts_indexes();
            let legacy_message_sanitized_with_writable_accounts_index_cache =
                SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                    sanitized_message: Box::new(legacy_message),
                    writable_accounts_cache,
                };

            match legacy_message_sanitized_with_writable_accounts_index_cache {
                SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                    sanitized_message: _,
                    writable_accounts_cache,
                } => {
                    assert_eq!(writable_accounts_cache.len(), 2);
                    assert!(writable_accounts_cache.contains(&0));
                    assert!(!writable_accounts_cache.contains(&1));
                    assert!(writable_accounts_cache.contains(&2));
                    assert!(!writable_accounts_cache.contains(&3));
                }
                _ => {
                    panic!("Expect to be SanitizedMessage::SanitizedWithWritableAccountsIndexCache")
                }
            }
        }

        let v0_message = SanitizedMessage::V0(v0::LoadedMessage::new(
            v0::Message {
                header: MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 1,
                },
                account_keys: vec![key0, key1, key2, key3],
                ..v0::Message::default()
            },
            LoadedAddresses {
                writable: vec![key4],
                readonly: vec![key5],
            },
        ));
        match v0_message {
            SanitizedMessage::V0(_) => {}
            _ => panic!("Expect a SanitizedMessage::Legacy"),
        }

        // assert can convert sanitized versioned message to have writable accounts cache
        {
            let writable_accounts_cache = v0_message.get_writable_accounts_indexes();
            let v0_message_sanitized_with_writable_accounts_index_cache =
                SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                    sanitized_message: Box::new(v0_message),
                    writable_accounts_cache,
                };

            match v0_message_sanitized_with_writable_accounts_index_cache {
                SanitizedMessage::SanitizedWithWritableAccountsIndexCache {
                    sanitized_message: _,
                    writable_accounts_cache,
                } => {
                    assert_eq!(writable_accounts_cache.len(), 3);
                    assert!(writable_accounts_cache.contains(&0));
                    assert!(!writable_accounts_cache.contains(&1));
                    assert!(writable_accounts_cache.contains(&2));
                    assert!(!writable_accounts_cache.contains(&3));
                    assert!(writable_accounts_cache.contains(&4));
                    assert!(!writable_accounts_cache.contains(&5));
                }
                _ => {
                    panic!("Expect to be SanitizedMessage::SanitizedWithWritableAccountsIndexCache")
                }
            }
        }
    }
}
