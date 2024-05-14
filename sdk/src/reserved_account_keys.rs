//! Collection of reserved account keys that cannot be write-locked by transactions.
//! New reserved account keys may be added as long as they specify a feature
//! gate that transitions the key into read-only at an epoch boundary.

#![cfg(feature = "full")]

use {
    crate::{
        address_lookup_table, bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
        compute_budget, config, ed25519_program, feature,
        feature_set::{self, FeatureSet},
        loader_v4, native_loader,
        pubkey::Pubkey,
        secp256k1_program, stake, system_program, sysvar, vote,
    },
    lazy_static::lazy_static,
    std::collections::{HashMap, HashSet},
};

// Inline zk token program id since it isn't available in the sdk
mod zk_token_proof_program {
    solana_sdk::declare_id!("ZkTokenProof1111111111111111111111111111111");
}

/// `ReservedAccountKeys` holds the set of currently active/inactive
/// account keys that are reserved by the protocol and may not be write-locked
/// during transaction processing.
#[derive(Debug, Clone, PartialEq)]
pub struct ReservedAccountKeys {
    /// Set of currently active reserved account keys
    pub active: HashSet<Pubkey>,
    /// Set of currently inactive reserved account keys that will be moved to the
    /// active set when their feature id is activated
    inactive: HashMap<Pubkey, Pubkey>,
}

impl Default for ReservedAccountKeys {
    fn default() -> Self {
        Self::new(&RESERVED_ACCOUNTS)
    }
}

impl ReservedAccountKeys {
    /// Compute a set of active / inactive reserved account keys from a list of
    /// keys with a designated feature id. If a reserved account key doesn't
    /// designate a feature id, it's already activated and should be inserted
    /// into the active set. If it does have a feature id, insert the key and
    /// its feature id into the inactive map.
    fn new(reserved_accounts: &[ReservedAccount]) -> Self {
        Self {
            active: reserved_accounts
                .iter()
                .filter(|reserved| reserved.feature_id.is_none())
                .map(|reserved| reserved.key)
                .collect(),
            inactive: reserved_accounts
                .iter()
                .filter_map(|ReservedAccount { key, feature_id }| {
                    feature_id.as_ref().map(|feature_id| (*key, *feature_id))
                })
                .collect(),
        }
    }

    /// Compute a set with all reserved keys active, regardless of whether their
    /// feature was activated. This is not to be used by the runtime. Useful for
    /// off-chain utilities that need to filter out reserved accounts.
    pub fn new_all_activated() -> Self {
        Self {
            active: Self::all_keys_iter().copied().collect(),
            inactive: HashMap::default(),
        }
    }

    /// Returns whether the specified key is reserved
    pub fn is_reserved(&self, key: &Pubkey) -> bool {
        self.active.contains(key)
    }

    /// Move inactive reserved account keys to the active set if their feature
    /// is active.
    pub fn update_active_set(&mut self, feature_set: &FeatureSet) {
        self.inactive.retain(|reserved_key, feature_id| {
            if feature_set.is_active(feature_id) {
                self.active.insert(*reserved_key);
                false
            } else {
                true
            }
        });
    }

    /// Return an iterator over all active / inactive reserved keys. This is not
    /// to be used by the runtime. Useful for off-chain utilities that need to
    /// filter out reserved accounts.
    pub fn all_keys_iter() -> impl Iterator<Item = &'static Pubkey> {
        RESERVED_ACCOUNTS
            .iter()
            .map(|reserved_key| &reserved_key.key)
    }

    /// Return an empty set of reserved keys for visibility when using in
    /// tests where the dynamic reserved key set is not available
    pub fn empty_key_set() -> HashSet<Pubkey> {
        HashSet::default()
    }
}

/// `ReservedAccount` represents a reserved account that will not be
/// write-lockable by transactions. If a feature id is set, the account will
/// become read-only only after the feature has been activated.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ReservedAccount {
    key: Pubkey,
    feature_id: Option<Pubkey>,
}

impl ReservedAccount {
    fn new_pending(key: Pubkey, feature_id: Pubkey) -> Self {
        Self {
            key,
            feature_id: Some(feature_id),
        }
    }

    fn new_active(key: Pubkey) -> Self {
        Self {
            key,
            feature_id: None,
        }
    }
}

// New reserved accounts should be added in alphabetical order and must specify
// a feature id for activation. Reserved accounts cannot be removed from this
// list without breaking consensus.
lazy_static! {
    static ref RESERVED_ACCOUNTS: Vec<ReservedAccount> = [
        // builtin programs
        ReservedAccount::new_pending(address_lookup_table::program::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccount::new_active(bpf_loader::id()),
        ReservedAccount::new_active(bpf_loader_deprecated::id()),
        ReservedAccount::new_active(bpf_loader_upgradeable::id()),
        ReservedAccount::new_pending(compute_budget::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccount::new_active(config::program::id()),
        ReservedAccount::new_pending(ed25519_program::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccount::new_active(feature::id()),
        ReservedAccount::new_pending(loader_v4::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccount::new_pending(secp256k1_program::id(), feature_set::add_new_reserved_account_keys::id()),
        #[allow(deprecated)]
        ReservedAccount::new_active(stake::config::id()),
        ReservedAccount::new_active(stake::program::id()),
        ReservedAccount::new_active(system_program::id()),
        ReservedAccount::new_active(vote::program::id()),
        ReservedAccount::new_pending(zk_token_proof_program::id(), feature_set::add_new_reserved_account_keys::id()),

        // sysvars
        ReservedAccount::new_active(sysvar::clock::id()),
        ReservedAccount::new_pending(sysvar::epoch_rewards::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccount::new_active(sysvar::epoch_schedule::id()),
        #[allow(deprecated)]
        ReservedAccount::new_active(sysvar::fees::id()),
        ReservedAccount::new_active(sysvar::instructions::id()),
        ReservedAccount::new_pending(sysvar::last_restart_slot::id(), feature_set::add_new_reserved_account_keys::id()),
        #[allow(deprecated)]
        ReservedAccount::new_active(sysvar::recent_blockhashes::id()),
        ReservedAccount::new_active(sysvar::rent::id()),
        ReservedAccount::new_active(sysvar::rewards::id()),
        ReservedAccount::new_active(sysvar::slot_hashes::id()),
        ReservedAccount::new_active(sysvar::slot_history::id()),
        ReservedAccount::new_active(sysvar::stake_history::id()),

        // other
        ReservedAccount::new_active(native_loader::id()),
        ReservedAccount::new_pending(sysvar::id(), feature_set::add_new_reserved_account_keys::id()),
    ].to_vec();
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program::{message::legacy::BUILTIN_PROGRAMS_KEYS, sysvar::ALL_IDS},
    };

    #[test]
    fn test_is_reserved() {
        let feature_id = Pubkey::new_unique();
        let active_reserved_account = ReservedAccount::new_active(Pubkey::new_unique());
        let pending_reserved_account =
            ReservedAccount::new_pending(Pubkey::new_unique(), feature_id);
        let reserved_account_keys =
            ReservedAccountKeys::new(&[active_reserved_account, pending_reserved_account]);

        assert!(
            reserved_account_keys.is_reserved(&active_reserved_account.key),
            "active reserved accounts should be inserted into the active set"
        );
        assert!(
            !reserved_account_keys.is_reserved(&pending_reserved_account.key),
            "pending reserved accounts should NOT be inserted into the active set"
        );
    }

    #[test]
    fn test_update_active_set() {
        let feature_ids = [Pubkey::new_unique(), Pubkey::new_unique()];
        let active_reserved_key = Pubkey::new_unique();
        let pending_reserved_keys = [Pubkey::new_unique(), Pubkey::new_unique()];
        let reserved_accounts = vec![
            ReservedAccount::new_active(active_reserved_key),
            ReservedAccount::new_pending(pending_reserved_keys[0], feature_ids[0]),
            ReservedAccount::new_pending(pending_reserved_keys[1], feature_ids[1]),
        ];

        let mut reserved_account_keys = ReservedAccountKeys::new(&reserved_accounts);
        assert!(reserved_account_keys.is_reserved(&active_reserved_key));
        assert!(!reserved_account_keys.is_reserved(&pending_reserved_keys[0]));
        assert!(!reserved_account_keys.is_reserved(&pending_reserved_keys[1]));

        // Updating the active set with a default feature set should be a no-op
        let previous_reserved_account_keys = reserved_account_keys.clone();
        let mut feature_set = FeatureSet::default();
        reserved_account_keys.update_active_set(&feature_set);
        assert_eq!(reserved_account_keys, previous_reserved_account_keys);

        // Updating the active set with an activated feature should also activate
        // the corresponding reserved key from inactive to active
        feature_set.active.insert(feature_ids[0], 0);
        reserved_account_keys.update_active_set(&feature_set);

        assert!(reserved_account_keys.is_reserved(&active_reserved_key));
        assert!(reserved_account_keys.is_reserved(&pending_reserved_keys[0]));
        assert!(!reserved_account_keys.is_reserved(&pending_reserved_keys[1]));

        // Update the active set again to ensure that the inactive map is
        // properly retained
        feature_set.active.insert(feature_ids[1], 0);
        reserved_account_keys.update_active_set(&feature_set);

        assert!(reserved_account_keys.is_reserved(&active_reserved_key));
        assert!(reserved_account_keys.is_reserved(&pending_reserved_keys[0]));
        assert!(reserved_account_keys.is_reserved(&pending_reserved_keys[1]));
    }

    #[test]
    fn test_static_list_compat() {
        let mut static_set = HashSet::new();
        static_set.extend(ALL_IDS.iter().cloned());
        static_set.extend(BUILTIN_PROGRAMS_KEYS.iter().cloned());

        let initial_active_set = ReservedAccountKeys::default().active;

        assert_eq!(initial_active_set, static_set);
    }
}
