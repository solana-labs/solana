use {
    crate::{
        bpf_loader_upgradeable,
        message::{legacy::is_builtin_key_or_sysvar, v0, AccountKeys},
        pubkey::Pubkey,
    },
    std::{borrow::Cow, collections::HashSet},
};

/// Combination of a version #0 message and its loaded addresses
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoadedMessage<'a> {
    /// Message which loaded a collection of lookup table addresses
    pub message: Cow<'a, v0::Message>,
    /// Addresses loaded with on-chain address lookup tables
    pub loaded_addresses: Cow<'a, LoadedAddresses>,
    /// List of boolean with same length as account_keys(), each boolean value indicates if
    /// corresponding account key is writable or not.
    pub is_writable_account_cache: Vec<bool>,
}

/// Collection of addresses loaded from on-chain lookup tables, split
/// by readonly and writable.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedAddresses {
    /// List of addresses for writable loaded accounts
    pub writable: Vec<Pubkey>,
    /// List of addresses for read-only loaded accounts
    pub readonly: Vec<Pubkey>,
}

impl FromIterator<LoadedAddresses> for LoadedAddresses {
    fn from_iter<T: IntoIterator<Item = LoadedAddresses>>(iter: T) -> Self {
        let (writable, readonly): (Vec<Vec<Pubkey>>, Vec<Vec<Pubkey>>) = iter
            .into_iter()
            .map(|addresses| (addresses.writable, addresses.readonly))
            .unzip();
        LoadedAddresses {
            writable: writable.into_iter().flatten().collect(),
            readonly: readonly.into_iter().flatten().collect(),
        }
    }
}

impl LoadedAddresses {
    /// Checks if there are no writable or readonly addresses
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Combined length of loaded writable and readonly addresses
    pub fn len(&self) -> usize {
        self.writable.len().saturating_add(self.readonly.len())
    }
}

impl<'a> LoadedMessage<'a> {
    pub fn new(message: v0::Message, loaded_addresses: LoadedAddresses) -> Self {
        let mut loaded_message = Self {
            message: Cow::Owned(message),
            loaded_addresses: Cow::Owned(loaded_addresses),
            is_writable_account_cache: Vec::default(),
        };
        loaded_message.set_is_writable_account_cache();
        loaded_message
    }

    pub fn new_borrowed(message: &'a v0::Message, loaded_addresses: &'a LoadedAddresses) -> Self {
        let mut loaded_message = Self {
            message: Cow::Borrowed(message),
            loaded_addresses: Cow::Borrowed(loaded_addresses),
            is_writable_account_cache: Vec::default(),
        };
        loaded_message.set_is_writable_account_cache();
        loaded_message
    }

    fn set_is_writable_account_cache(&mut self) {
        let is_writable_account_cache = self
            .account_keys()
            .iter()
            .enumerate()
            .map(|(i, _key)| self.is_writable_internal(i))
            .collect::<Vec<_>>();
        let _ = std::mem::replace(
            &mut self.is_writable_account_cache,
            is_writable_account_cache,
        );
    }

    /// Returns the full list of static and dynamic account keys that are loaded for this message.
    pub fn account_keys(&self) -> AccountKeys {
        AccountKeys::new(&self.message.account_keys, Some(&self.loaded_addresses))
    }

    /// Returns the list of static account keys that are loaded for this message.
    pub fn static_account_keys(&self) -> &[Pubkey] {
        &self.message.account_keys
    }

    /// Returns true if any account keys are duplicates
    pub fn has_duplicates(&self) -> bool {
        let mut uniq = HashSet::new();
        self.account_keys().iter().any(|x| !uniq.insert(x))
    }

    /// Returns true if the account at the specified index was requested to be
    /// writable.  This method should not be used directly.
    fn is_writable_index(&self, key_index: usize) -> bool {
        let header = &self.message.header;
        let num_account_keys = self.message.account_keys.len();
        let num_signed_accounts = usize::from(header.num_required_signatures);
        if key_index >= num_account_keys {
            let loaded_addresses_index = key_index.saturating_sub(num_account_keys);
            loaded_addresses_index < self.loaded_addresses.writable.len()
        } else if key_index >= num_signed_accounts {
            let num_unsigned_accounts = num_account_keys.saturating_sub(num_signed_accounts);
            let num_writable_unsigned_accounts = num_unsigned_accounts
                .saturating_sub(usize::from(header.num_readonly_unsigned_accounts));
            let unsigned_account_index = key_index.saturating_sub(num_signed_accounts);
            unsigned_account_index < num_writable_unsigned_accounts
        } else {
            let num_writable_signed_accounts = num_signed_accounts
                .saturating_sub(usize::from(header.num_readonly_signed_accounts));
            key_index < num_writable_signed_accounts
        }
    }

    /// Returns true if the account at the specified index was loaded as writable
    fn is_writable_internal(&self, key_index: usize) -> bool {
        if self.is_writable_index(key_index) {
            if let Some(key) = self.account_keys().get(key_index) {
                return !(is_builtin_key_or_sysvar(key) || self.demote_program_id(key_index));
            }
        }
        false
    }

    pub fn is_writable(&self, key_index: usize) -> bool {
        *self
            .is_writable_account_cache
            .get(key_index)
            .unwrap_or(&false)
    }

    pub fn is_signer(&self, i: usize) -> bool {
        i < self.message.header.num_required_signatures as usize
    }

    pub fn demote_program_id(&self, i: usize) -> bool {
        self.is_key_called_as_program(i) && !self.is_upgradeable_loader_present()
    }

    /// Returns true if the account at the specified index is called as a program by an instruction
    pub fn is_key_called_as_program(&self, key_index: usize) -> bool {
        if let Ok(key_index) = u8::try_from(key_index) {
            self.message
                .instructions
                .iter()
                .any(|ix| ix.program_id_index == key_index)
        } else {
            false
        }
    }

    /// Returns true if any account is the bpf upgradeable loader
    pub fn is_upgradeable_loader_present(&self) -> bool {
        self.account_keys()
            .iter()
            .any(|&key| key == bpf_loader_upgradeable::id())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{instruction::CompiledInstruction, message::MessageHeader, system_program, sysvar},
        itertools::Itertools,
    };

    fn check_test_loaded_message() -> (LoadedMessage<'static>, [Pubkey; 6]) {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        let message = LoadedMessage::new(
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
        );

        (message, [key0, key1, key2, key3, key4, key5])
    }

    #[test]
    fn test_has_duplicates() {
        let message = check_test_loaded_message().0;

        assert!(!message.has_duplicates());
    }

    #[test]
    fn test_has_duplicates_with_dupe_keys() {
        let create_message_with_dupe_keys = |mut keys: Vec<Pubkey>| {
            LoadedMessage::new(
                v0::Message {
                    account_keys: keys.split_off(2),
                    ..v0::Message::default()
                },
                LoadedAddresses {
                    writable: keys.split_off(2),
                    readonly: keys,
                },
            )
        };

        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let dupe_key = Pubkey::new_unique();

        let keys = vec![key0, key1, key2, key3, dupe_key, dupe_key];
        let keys_len = keys.len();
        for keys in keys.into_iter().permutations(keys_len).unique() {
            let message = create_message_with_dupe_keys(keys);
            assert!(message.has_duplicates());
        }
    }

    #[test]
    fn test_is_writable_index() {
        let message = check_test_loaded_message().0;

        assert!(message.is_writable_index(0));
        assert!(!message.is_writable_index(1));
        assert!(message.is_writable_index(2));
        assert!(!message.is_writable_index(3));
        assert!(message.is_writable_index(4));
        assert!(!message.is_writable_index(5));
    }

    #[test]
    fn test_is_writable() {
        solana_logger::setup();
        let create_message_with_keys = |keys: Vec<Pubkey>| {
            LoadedMessage::new(
                v0::Message {
                    header: MessageHeader {
                        num_required_signatures: 1,
                        num_readonly_signed_accounts: 0,
                        num_readonly_unsigned_accounts: 1,
                    },
                    account_keys: keys[..2].to_vec(),
                    ..v0::Message::default()
                },
                LoadedAddresses {
                    writable: keys[2..=2].to_vec(),
                    readonly: keys[3..].to_vec(),
                },
            )
        };

        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        {
            let message = create_message_with_keys(vec![sysvar::clock::id(), key0, key1, key2]);
            assert!(message.is_writable_index(0));
            assert!(!message.is_writable(0));
        }

        {
            let message = create_message_with_keys(vec![system_program::id(), key0, key1, key2]);
            assert!(message.is_writable_index(0));
            assert!(!message.is_writable(0));
        }

        {
            let message = create_message_with_keys(vec![key0, key1, system_program::id(), key2]);
            assert!(message.is_writable_index(2));
            assert!(!message.is_writable(2));
        }
    }

    #[test]
    fn test_demote_writable_program() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let message = LoadedMessage::new(
            v0::Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys: vec![key0],
                instructions: vec![CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![1],
                    data: vec![],
                }],
                ..v0::Message::default()
            },
            LoadedAddresses {
                writable: vec![key1, key2],
                readonly: vec![],
            },
        );

        assert!(message.is_writable_index(2));
        assert!(!message.is_writable(2));
    }
}
