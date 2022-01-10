use {
    crate::{
        bpf_loader_upgradeable,
        message::{legacy::BUILTIN_PROGRAMS_KEYS, v0},
        pubkey::Pubkey,
        sysvar,
    },
    std::{collections::HashSet, convert::TryFrom, ops::Deref},
};

/// Combination of a version #0 message and its loaded addresses
#[derive(Debug, Clone)]
pub struct LoadedMessage {
    /// Message which loaded a collection of lookup table addresses
    pub message: v0::Message,
    /// Addresses loaded with on-chain address lookup tables
    pub loaded_addresses: LoadedAddresses,
}

impl Deref for LoadedMessage {
    type Target = v0::Message;
    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

/// Collection of addresses loaded from on-chain lookup tables, split
/// by readonly and writable.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
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

impl LoadedMessage {
    /// Returns an iterator of account key segments. The ordering of segments
    /// affects how account indexes from compiled instructions are resolved and
    /// so should not be changed.
    fn account_keys_segment_iter(&self) -> impl Iterator<Item = &Vec<Pubkey>> {
        vec![
            &self.message.account_keys,
            &self.loaded_addresses.writable,
            &self.loaded_addresses.readonly,
        ]
        .into_iter()
    }

    /// Returns the total length of loaded accounts for this message
    pub fn account_keys_len(&self) -> usize {
        let mut len = 0usize;
        for key_segment in self.account_keys_segment_iter() {
            len = len.saturating_add(key_segment.len());
        }
        len
    }

    /// Iterator for the addresses of the loaded accounts for this message
    pub fn account_keys_iter(&self) -> impl Iterator<Item = &Pubkey> {
        self.account_keys_segment_iter().flatten()
    }

    /// Returns true if any account keys are duplicates
    pub fn has_duplicates(&self) -> bool {
        let mut uniq = HashSet::new();
        self.account_keys_iter().any(|x| !uniq.insert(x))
    }

    /// Returns the address of the account at the specified index of the list of
    /// message account keys constructed from static keys, followed by dynamically
    /// loaded writable addresses, and lastly the list of dynamically loaded
    /// readonly addresses.
    pub fn get_account_key(&self, mut index: usize) -> Option<&Pubkey> {
        for key_segment in self.account_keys_segment_iter() {
            if index < key_segment.len() {
                return Some(&key_segment[index]);
            }
            index = index.saturating_sub(key_segment.len());
        }

        None
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
    pub fn is_writable(&self, key_index: usize) -> bool {
        if self.is_writable_index(key_index) {
            if let Some(key) = self.get_account_key(key_index) {
                let demote_program_id = self.is_key_called_as_program(key_index)
                    && !self.is_upgradeable_loader_present();
                return !(sysvar::is_sysvar_id(key)
                    || BUILTIN_PROGRAMS_KEYS.contains(key)
                    || demote_program_id);
            }
        }
        false
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
        self.account_keys_iter()
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

    fn check_test_loaded_message() -> (LoadedMessage, [Pubkey; 6]) {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        let message = LoadedMessage {
            message: v0::Message {
                header: MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 1,
                },
                account_keys: vec![key0, key1, key2, key3],
                ..v0::Message::default()
            },
            loaded_addresses: LoadedAddresses {
                writable: vec![key4],
                readonly: vec![key5],
            },
        };

        (message, [key0, key1, key2, key3, key4, key5])
    }

    #[test]
    fn test_account_keys_segment_iter() {
        let (message, keys) = check_test_loaded_message();

        let expected_segments = vec![
            vec![keys[0], keys[1], keys[2], keys[3]],
            vec![keys[4]],
            vec![keys[5]],
        ];

        let mut iter = message.account_keys_segment_iter();
        for expected_segment in expected_segments {
            assert_eq!(iter.next(), Some(&expected_segment));
        }
    }

    #[test]
    fn test_account_keys_len() {
        let (message, keys) = check_test_loaded_message();

        assert_eq!(message.account_keys_len(), keys.len());
    }

    #[test]
    fn test_account_keys_iter() {
        let (message, keys) = check_test_loaded_message();

        let mut iter = message.account_keys_iter();
        for expected_key in keys {
            assert_eq!(iter.next(), Some(&expected_key));
        }
    }

    #[test]
    fn test_has_duplicates() {
        let message = check_test_loaded_message().0;

        assert!(!message.has_duplicates());
    }

    #[test]
    fn test_has_duplicates_with_dupe_keys() {
        let create_message_with_dupe_keys = |mut keys: Vec<Pubkey>| LoadedMessage {
            message: v0::Message {
                account_keys: keys.split_off(2),
                ..v0::Message::default()
            },
            loaded_addresses: LoadedAddresses {
                writable: keys.split_off(2),
                readonly: keys,
            },
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
    fn test_get_account_key() {
        let (message, keys) = check_test_loaded_message();

        assert_eq!(message.get_account_key(0), Some(&keys[0]));
        assert_eq!(message.get_account_key(1), Some(&keys[1]));
        assert_eq!(message.get_account_key(2), Some(&keys[2]));
        assert_eq!(message.get_account_key(3), Some(&keys[3]));
        assert_eq!(message.get_account_key(4), Some(&keys[4]));
        assert_eq!(message.get_account_key(5), Some(&keys[5]));
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
        let mut message = check_test_loaded_message().0;

        message.message.account_keys[0] = sysvar::clock::id();
        assert!(message.is_writable_index(0));
        assert!(!message.is_writable(0));

        message.message.account_keys[0] = system_program::id();
        assert!(message.is_writable_index(0));
        assert!(!message.is_writable(0));
    }

    #[test]
    fn test_demote_writable_program() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let message = LoadedMessage {
            message: v0::Message {
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
            loaded_addresses: LoadedAddresses {
                writable: vec![key1, key2],
                readonly: vec![],
            },
        };

        assert!(message.is_writable_index(2));
        assert!(!message.is_writable(2));
    }
}
