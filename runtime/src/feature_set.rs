use solana_sdk::{
    hash::{Hash, Hasher},
    pubkey::Pubkey,
};
use std::collections::HashSet;

/// The `FeatureSet` struct tracks the set of available and currently active runtime features
#[derive(AbiExample)]
pub struct FeatureSet {
    /// Unique identifier of this feature set
    pub id: Hash,

    /// Features that are currently active
    pub active: HashSet<Pubkey>,

    /// Features that are currently inactive
    pub inactive: HashSet<Pubkey>,
}

impl FeatureSet {
    pub fn active(&self, feature_id: &Pubkey) -> bool {
        self.active.contains(feature_id)
    }
}

impl Default for FeatureSet {
    // By default all features are disabled
    fn default() -> Self {
        let features: [Pubkey; 0] = [];

        Self {
            id: {
                let mut hasher = Hasher::default();
                for feature in features.iter() {
                    hasher.hash(feature.as_ref());
                }
                hasher.result()
            },
            active: HashSet::new(),
            inactive: features.iter().cloned().collect(),
        }
    }
}

impl FeatureSet {
    // New `FeatureSet` with all features enabled
    pub fn new_enabled() -> Self {
        let default = Self::default();

        Self {
            id: default.id,
            active: default
                .active
                .intersection(&default.inactive)
                .cloned()
                .collect::<HashSet<_>>(),
            inactive: HashSet::new(),
        }
    }
}
