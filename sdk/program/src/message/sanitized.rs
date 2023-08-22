use {
    crate::{
        ed25519_program,
        hash::Hash,
        instruction::CompiledInstruction,
        message::{
            legacy,
            v0::{self, LoadedAddresses},
            AccountKeys, AddressLoader, AddressLoaderError, MessageHeader,
            SanitizedVersionedMessage, VersionedMessage,
        },
        nonce::NONCED_TX_MARKER_IX_INDEX,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        secp256k1_program,
        solana_program::{system_instruction::SystemInstruction, system_program},
        sysvar::instructions::{BorrowedAccountMeta, BorrowedInstruction},
    },
    std::{borrow::Cow, convert::TryFrom},
    thiserror::Error,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LegacyMessage<'a> {
    /// Legacy message
    pub message: Cow<'a, legacy::Message>,
    /// List of boolean with same length as account_keys(), each boolean value indicates if
    /// corresponding account key is writable or not.
    pub is_writable_account_cache: Vec<bool>,
}

impl<'a> LegacyMessage<'a> {
    pub fn new(message: legacy::Message) -> Self {
        let is_writable_account_cache = message
            .account_keys
            .iter()
            .enumerate()
            .map(|(i, _key)| message.is_writable(i))
            .collect::<Vec<_>>();
        Self {
            message: Cow::Owned(message),
            is_writable_account_cache,
        }
    }

    pub fn has_duplicates(&self) -> bool {
        self.message.has_duplicates()
    }

    pub fn is_key_called_as_program(&self, key_index: usize) -> bool {
        self.message.is_key_called_as_program(key_index)
    }

    /// Inspect all message keys for the bpf upgradeable loader
    pub fn is_upgradeable_loader_present(&self) -> bool {
        self.message.is_upgradeable_loader_present()
    }

    /// Returns the full list of account keys.
    pub fn account_keys(&self) -> AccountKeys {
        AccountKeys::new(&self.message.account_keys, None)
    }

    pub fn is_writable(&self, index: usize) -> bool {
        *self.is_writable_account_cache.get(index).unwrap_or(&false)
    }
}

/// Sanitized message of a transaction.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SanitizedMessage {
    /// Sanitized legacy message
    Legacy(LegacyMessage<'static>),
    /// Sanitized version #0 message with dynamically loaded addresses
    V0(v0::LoadedMessage<'static>),
}

#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum SanitizeMessageError {
    #[error("index out of bounds")]
    IndexOutOfBounds,
    #[error("value out of bounds")]
    ValueOutOfBounds,
    #[error("invalid value")]
    InvalidValue,
    #[error("{0}")]
    AddressLoaderError(#[from] AddressLoaderError),
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

impl TryFrom<legacy::Message> for SanitizedMessage {
    type Error = SanitizeMessageError;
    fn try_from(message: legacy::Message) -> Result<Self, Self::Error> {
        message.sanitize()?;
        Ok(Self::Legacy(LegacyMessage::new(message)))
    }
}

impl SanitizedMessage {
    /// Create a sanitized message from a sanitized versioned message.
    /// If the input message uses address tables, attempt to look up the
    /// address for each table index.
    pub fn try_new(
        sanitized_msg: SanitizedVersionedMessage,
        address_loader: impl AddressLoader,
    ) -> Result<Self, SanitizeMessageError> {
        Ok(match sanitized_msg.message {
            VersionedMessage::Legacy(message) => {
                SanitizedMessage::Legacy(LegacyMessage::new(message))
            }
            VersionedMessage::V0(message) => {
                let loaded_addresses =
                    address_loader.load_addresses(&message.address_table_lookups)?;
                SanitizedMessage::V0(v0::LoadedMessage::new(message, loaded_addresses))
            }
        })
    }

    /// Return true if this message contains duplicate account keys
    pub fn has_duplicates(&self) -> bool {
        match self {
            SanitizedMessage::Legacy(message) => message.has_duplicates(),
            SanitizedMessage::V0(message) => message.has_duplicates(),
        }
    }

    /// Message header which identifies the number of signer and writable or
    /// readonly accounts
    pub fn header(&self) -> &MessageHeader {
        match self {
            Self::Legacy(legacy_message) => &legacy_message.message.header,
            Self::V0(loaded_msg) => &loaded_msg.message.header,
        }
    }

    /// Returns a legacy message if this sanitized message wraps one
    pub fn legacy_message(&self) -> Option<&legacy::Message> {
        if let Self::Legacy(legacy_message) = &self {
            Some(&legacy_message.message)
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
            Self::Legacy(legacy_message) => &legacy_message.message.recent_blockhash,
            Self::V0(loaded_msg) => &loaded_msg.message.recent_blockhash,
        }
    }

    /// Program instructions that will be executed in sequence and committed in
    /// one atomic transaction if all succeed.
    pub fn instructions(&self) -> &[CompiledInstruction] {
        match self {
            Self::Legacy(legacy_message) => &legacy_message.message.instructions,
            Self::V0(loaded_msg) => &loaded_msg.message.instructions,
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
            Self::Legacy(message) => message.account_keys(),
            Self::V0(message) => message.account_keys(),
        }
    }

    /// Returns the list of account keys used for account lookup tables.
    pub fn message_address_table_lookups(&self) -> &[v0::MessageAddressTableLookup] {
        match self {
            Self::Legacy(_message) => &[],
            Self::V0(message) => &message.message.address_table_lookups,
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
        }
    }

    /// Returns true if the account at the specified index is not invoked as a
    /// program or, if invoked, is passed to a program.
    pub fn is_non_loader_key(&self, key_index: usize) -> bool {
        !self.is_invoked(key_index) || self.is_key_passed_to_program(key_index)
    }

    /// Returns true if the account at the specified index is writable by the
    /// instructions in this message.
    pub fn is_writable(&self, index: usize) -> bool {
        match self {
            Self::Legacy(message) => message.is_writable(index),
            Self::V0(message) => message.is_writable(index),
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

    pub fn num_signatures(&self) -> u64 {
        let mut num_signatures = u64::from(self.header().num_required_signatures);
        // This next part is really calculating the number of pre-processor
        // operations being done and treating them like a signature
        for (program_id, instruction) in self.program_instructions_iter() {
            if secp256k1_program::check_id(program_id) || ed25519_program::check_id(program_id) {
                if let Some(num_verifies) = instruction.data.first() {
                    num_signatures = num_signatures.saturating_add(u64::from(*num_verifies));
                }
            }
        }
        num_signatures
    }

    pub fn num_write_locks(&self) -> u64 {
        self.account_keys()
            .len()
            .saturating_sub(self.num_readonly_accounts()) as u64
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::message::v0, std::collections::HashSet};

    #[test]
    fn test_try_from_message() {
        let legacy_message_with_no_signers = legacy::Message {
            account_keys: vec![Pubkey::new_unique()],
            ..legacy::Message::default()
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

        let message = SanitizedMessage::try_from(legacy::Message::new_with_compiled_instructions(
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

        let legacy_message = SanitizedMessage::try_from(legacy::Message {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key0, key1, key2, key3],
            ..legacy::Message::default()
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

        let message = SanitizedMessage::try_from(legacy::Message::new_with_compiled_instructions(
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
    #[allow(clippy::get_first)]
    fn test_is_writable_account_cache() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        let legacy_message = SanitizedMessage::try_from(legacy::Message {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key0, key1, key2, key3],
            ..legacy::Message::default()
        })
        .unwrap();
        match legacy_message {
            SanitizedMessage::Legacy(message) => {
                assert_eq!(
                    message.is_writable_account_cache.len(),
                    message.account_keys().len()
                );
                assert!(message.is_writable_account_cache.get(0).unwrap());
                assert!(!message.is_writable_account_cache.get(1).unwrap());
                assert!(message.is_writable_account_cache.get(2).unwrap());
                assert!(!message.is_writable_account_cache.get(3).unwrap());
            }
            _ => {
                panic!("Expect to be SanitizedMessage::LegacyMessage")
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
            SanitizedMessage::V0(message) => {
                assert_eq!(
                    message.is_writable_account_cache.len(),
                    message.account_keys().len()
                );
                assert!(message.is_writable_account_cache.get(0).unwrap());
                assert!(!message.is_writable_account_cache.get(1).unwrap());
                assert!(message.is_writable_account_cache.get(2).unwrap());
                assert!(!message.is_writable_account_cache.get(3).unwrap());
                assert!(message.is_writable_account_cache.get(4).unwrap());
                assert!(!message.is_writable_account_cache.get(5).unwrap());
            }
            _ => {
                panic!("Expect to be SanitizedMessage::V0")
            }
        }
    }
}
