use {
    crate::{message::v0::LoadedAddresses, pubkey::Pubkey},
    std::ops::Index,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(account_keys
            .key_segment_iter()
            .into_iter()
            .eq(expected_segments.iter()));
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
}
