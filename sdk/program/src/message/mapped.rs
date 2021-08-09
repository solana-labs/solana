use {
    crate::{
        message::{legacy::BUILTIN_PROGRAMS_KEYS, v0},
        pubkey::Pubkey,
        sysvar,
    },
    std::collections::HashSet,
};

/// Combination of a version #0 message and its mapped addresses
#[derive(Debug, Clone)]
pub struct MappedMessage {
    /// Message which loaded a collection of mapped addresses
    pub message: v0::Message,
    /// Collection of mapped addresses loaded by this message
    pub mapped_addresses: MappedAddresses,
}

/// Collection of mapped addresses loaded succinctly by a transaction using
/// on-chain address map accounts.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct MappedAddresses {
    /// List of addresses for writable loaded accounts
    pub writable: Vec<Pubkey>,
    /// List of addresses for read-only loaded accounts
    pub readonly: Vec<Pubkey>,
}

impl MappedMessage {
    /// Returns true if any account keys are duplicates
    pub fn has_duplicates(&self) -> bool {
        let mut uniq = HashSet::new();
        self.account_keys_iter().all(|x| uniq.insert(x))
    }

    /// Returns the address of the account at the specified index of the list of
    /// message account keys constructed from unmapped keys, followed by mapped
    /// writable addresses, and lastly the list of mapped readonly addresses.
    pub fn get_account_key(&self, mut index: usize) -> Option<&Pubkey> {
        for key_segment in self.account_keys_segment_iter() {
            if index < key_segment.len() {
                return Some(&key_segment[index]);
            }
            index = index.saturating_sub(key_segment.len());
        }

        None
    }

    /// Returns the total length of loaded accounts for this message
    pub fn account_keys_len(&self) -> usize {
        self.message
            .account_keys
            .len()
            .saturating_add(self.mapped_addresses.writable.len())
            .saturating_add(self.mapped_addresses.readonly.len())
    }

    /// Iterator for the addresses of the loaded accounts for this message
    pub fn account_keys_iter(&self) -> impl Iterator<Item = &Pubkey> {
        self.account_keys_segment_iter().flatten()
    }

    fn account_keys_segment_iter(&self) -> impl Iterator<Item = &Vec<Pubkey>> {
        vec![
            &self.message.account_keys,
            &self.mapped_addresses.writable,
            &self.mapped_addresses.readonly,
        ]
        .into_iter()
    }

    fn is_writable_index(&self, i: usize) -> bool {
        let header = &self.message.header;
        let num_unmapped_keys = self.message.account_keys.len();
        i >= num_unmapped_keys && i < self.mapped_addresses.writable.len()
            || i < header
                .num_required_signatures
                .saturating_sub(header.num_readonly_signed_accounts) as usize
            || (i >= header.num_required_signatures as usize
                && i < num_unmapped_keys
                    .saturating_sub(header.num_readonly_unsigned_accounts as usize))
    }

    /// Returns true if the account at the specified index was loaded as writable
    pub fn is_writable(&self, key_index: usize) -> bool {
        if self.is_writable_index(key_index) {
            if let Some(key) = self.get_account_key(key_index) {
                return !(sysvar::is_sysvar_id(key) || BUILTIN_PROGRAMS_KEYS.contains(key));
            }
        }
        false
    }
}
