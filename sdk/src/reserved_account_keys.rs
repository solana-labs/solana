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
        secp256k1_program, stake, system_program, sysvar, vote, zk_token_proof_program,
    },
    lazy_static::lazy_static,
    std::collections::HashSet,
};

pub struct ReservedAccountKeys;
impl ReservedAccountKeys {
    /// Compute a set of all reserved keys, regardless of if they are active or not
    pub fn active_and_inactive() -> HashSet<Pubkey> {
        RESERVED_ACCOUNT_KEYS
            .iter()
            .map(|reserved_key| reserved_key.key)
            .collect()
    }

    /// Compute the active set of reserved keys based on activated features
    pub fn active(feature_set: &FeatureSet) -> HashSet<Pubkey> {
        Self::compute_active(&RESERVED_ACCOUNT_KEYS, feature_set)
    }

    // Method only exists for unit testing
    fn compute_active(
        account_keys: &[ReservedAccountKey],
        feature_set: &FeatureSet,
    ) -> HashSet<Pubkey> {
        account_keys
            .iter()
            .filter(|reserved_key| reserved_key.is_active(feature_set))
            .map(|reserved_key| reserved_key.key)
            .collect()
    }

    /// Return an empty list of reserved keys for visibility when using in
    /// places where the dynamic reserved key set is not available
    pub fn empty() -> HashSet<Pubkey> {
        HashSet::default()
    }
}

/// `ReservedAccountKey` represents a reserved account key that will not be
/// write-lockable by transactions. If a feature id is set, the account key will
/// become read-only only after the feature has been activated.
#[derive(Debug, Clone, Eq, PartialEq)]
struct ReservedAccountKey {
    key: Pubkey,
    feature_id: Option<Pubkey>,
}

impl ReservedAccountKey {
    fn new(key: Pubkey, feature_id: Option<Pubkey>) -> Self {
        Self { key, feature_id }
    }

    /// Returns true if no feature id is set or if the feature id is activated
    /// in the feature set.
    fn is_active(&self, feature_set: &FeatureSet) -> bool {
        self.feature_id
            .map_or(true, |feature_id| feature_set.is_active(&feature_id))
    }
}

// New reserved account keys should be added in alphabetical order and must
// specify a feature id for activation.
lazy_static! {
    static ref RESERVED_ACCOUNT_KEYS: Vec<ReservedAccountKey> = [
        // builtin programs
        ReservedAccountKey::new(address_lookup_table::program::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        ReservedAccountKey::new(bpf_loader::id(), None),
        ReservedAccountKey::new(bpf_loader_deprecated::id(), None),
        ReservedAccountKey::new(bpf_loader_upgradeable::id(), None),
        ReservedAccountKey::new(compute_budget::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        ReservedAccountKey::new(config::program::id(), None),
        ReservedAccountKey::new(ed25519_program::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        ReservedAccountKey::new(feature::id(), None),
        ReservedAccountKey::new(loader_v4::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        ReservedAccountKey::new(secp256k1_program::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        #[allow(deprecated)]
        ReservedAccountKey::new(stake::config::id(), None),
        ReservedAccountKey::new(stake::program::id(), None),
        ReservedAccountKey::new(system_program::id(), None),
        ReservedAccountKey::new(vote::program::id(), None),
        ReservedAccountKey::new(zk_token_proof_program::id(), Some(feature_set::add_new_reserved_account_keys::id())),

        // sysvars
        ReservedAccountKey::new(sysvar::clock::id(), None),
        ReservedAccountKey::new(sysvar::epoch_rewards::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        ReservedAccountKey::new(sysvar::epoch_schedule::id(), None),
        #[allow(deprecated)]
        ReservedAccountKey::new(sysvar::fees::id(), None),
        ReservedAccountKey::new(sysvar::instructions::id(), None),
        ReservedAccountKey::new(sysvar::last_restart_slot::id(), Some(feature_set::add_new_reserved_account_keys::id())),
        #[allow(deprecated)]
        ReservedAccountKey::new(sysvar::recent_blockhashes::id(), None),
        ReservedAccountKey::new(sysvar::rent::id(), None),
        ReservedAccountKey::new(sysvar::rewards::id(), None),
        ReservedAccountKey::new(sysvar::slot_hashes::id(), None),
        ReservedAccountKey::new(sysvar::slot_history::id(), None),
        ReservedAccountKey::new(sysvar::stake_history::id(), None),

        // other
        ReservedAccountKey::new(native_loader::id(), None),
        ReservedAccountKey::new(sysvar::id(), Some(feature_set::add_new_reserved_account_keys::id())),
    ].to_vec();
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program::{message::legacy::BUILTIN_PROGRAMS_KEYS, sysvar::ALL_IDS},
    };

    #[test]
    fn test_reserved_account_key_is_active() {
        let feature_id = Pubkey::new_unique();
        let old_reserved_account_key = ReservedAccountKey::new(Pubkey::new_unique(), None);
        let new_reserved_account_key =
            ReservedAccountKey::new(Pubkey::new_unique(), Some(feature_id));

        let mut feature_set = FeatureSet::default();
        assert!(
            old_reserved_account_key.is_active(&feature_set),
            "if not feature id is set, the key should be active"
        );
        assert!(
            !new_reserved_account_key.is_active(&feature_set),
            "if feature id is set but not in the feature set, the key should not be active"
        );

        feature_set.active.insert(feature_id, 0);
        assert!(
            new_reserved_account_key.is_active(&feature_set),
            "if feature id is set and is in the feature set, the key should be active"
        );
    }

    #[test]
    fn test_reserved_account_keys_compute_active() {
        let feature_id = Pubkey::new_unique();
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let test_account_keys = vec![
            ReservedAccountKey::new(key0, None),
            ReservedAccountKey::new(key1, Some(feature_id)),
            ReservedAccountKey::new(Pubkey::new_unique(), Some(Pubkey::new_unique())),
        ];

        let mut feature_set = FeatureSet::default();
        assert_eq!(
            HashSet::from_iter([key0].into_iter()),
            ReservedAccountKeys::compute_active(&test_account_keys, &feature_set),
            "should only contain keys without feature ids"
        );

        feature_set.active.insert(feature_id, 0);
        assert_eq!(
            HashSet::from_iter([key0, key1].into_iter()),
            ReservedAccountKeys::compute_active(&test_account_keys, &feature_set),
            "should only contain keys without feature ids or with active feature ids"
        );
    }

    #[test]
    fn test_static_list_compat() {
        let mut static_set = HashSet::new();
        static_set.extend(ALL_IDS.iter().cloned());
        static_set.extend(BUILTIN_PROGRAMS_KEYS.iter().cloned());

        let initial_dynamic_set = ReservedAccountKeys::active(&FeatureSet::default());

        assert_eq!(initial_dynamic_set, static_set);
    }
}
