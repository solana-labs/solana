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
    std::collections::HashSet,
};

// Temporary until a zk token program module is added to the sdk
mod zk_token_proof_program {
    solana_sdk::declare_id!("ZkTokenProof1111111111111111111111111111111");
}

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
        ReservedAccountKey::new_pending(address_lookup_table::program::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccountKey::new_active(bpf_loader::id()),
        ReservedAccountKey::new_active(bpf_loader_deprecated::id()),
        ReservedAccountKey::new_active(bpf_loader_upgradeable::id()),
        ReservedAccountKey::new_pending(compute_budget::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccountKey::new_active(config::program::id()),
        ReservedAccountKey::new_pending(ed25519_program::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccountKey::new_active(feature::id()),
        ReservedAccountKey::new_pending(loader_v4::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccountKey::new_pending(secp256k1_program::id(), feature_set::add_new_reserved_account_keys::id()),
        #[allow(deprecated)]
        ReservedAccountKey::new_active(stake::config::id()),
        ReservedAccountKey::new_active(stake::program::id()),
        ReservedAccountKey::new_active(system_program::id()),
        ReservedAccountKey::new_active(vote::program::id()),
        ReservedAccountKey::new_pending(zk_token_proof_program::id(), feature_set::add_new_reserved_account_keys::id()),

        // sysvars
        ReservedAccountKey::new_active(sysvar::clock::id()),
        ReservedAccountKey::new_pending(sysvar::epoch_rewards::id(), feature_set::add_new_reserved_account_keys::id()),
        ReservedAccountKey::new_active(sysvar::epoch_schedule::id()),
        #[allow(deprecated)]
        ReservedAccountKey::new_active(sysvar::fees::id()),
        ReservedAccountKey::new_active(sysvar::instructions::id()),
        ReservedAccountKey::new_pending(sysvar::last_restart_slot::id(), feature_set::add_new_reserved_account_keys::id()),
        #[allow(deprecated)]
        ReservedAccountKey::new_active(sysvar::recent_blockhashes::id()),
        ReservedAccountKey::new_active(sysvar::rent::id()),
        ReservedAccountKey::new_active(sysvar::rewards::id()),
        ReservedAccountKey::new_active(sysvar::slot_hashes::id()),
        ReservedAccountKey::new_active(sysvar::slot_history::id()),
        ReservedAccountKey::new_active(sysvar::stake_history::id()),

        // other
        ReservedAccountKey::new_active(native_loader::id()),
        ReservedAccountKey::new_pending(sysvar::id(), feature_set::add_new_reserved_account_keys::id()),
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
        let old_reserved_account_key = ReservedAccountKey::new_active(Pubkey::new_unique());
        let new_reserved_account_key =
            ReservedAccountKey::new_pending(Pubkey::new_unique(), feature_id);

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
            ReservedAccountKey::new_active(key0),
            ReservedAccountKey::new_pending(key1, feature_id),
            ReservedAccountKey::new_pending(Pubkey::new_unique(), Pubkey::new_unique()),
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
