use lazy_static::lazy_static;
use solana_sdk::{
    hash::{Hash, Hasher},
    pubkey::Pubkey,
};
use std::collections::{HashMap, HashSet};

pub mod instructions_sysvar_enabled {
    solana_sdk::declare_id!("EnvhHCLvg55P7PDtbvR1NwuTuAeodqpusV3MR5QEK8gs");
}

pub mod secp256k1_program_enabled {
    solana_sdk::declare_id!("E3PHP7w8kB7np3CTQ1qQ2tW3KCtjRSXBQgW9vM2mWv2Y");
}

lazy_static! {
    pub static ref FEATURE_NAMES: HashMap<Pubkey, &'static str> = [
        (instructions_sysvar_enabled::id(), "instructions sysvar"),
        (secp256k1_program_enabled::id(), "secp256k1 program")
        /*************** ADD NEW FEATURES HERE ***************/
    ]
    .iter()
    .cloned()
    .collect();

    static ref ID: Hash = {
        let mut hasher = Hasher::default();
        let mut feature_ids = FEATURE_NAMES.keys().collect::<Vec<_>>();
        feature_ids.sort();
        for feature in feature_ids {
            hasher.hash(feature.as_ref());
        }
        hasher.result()
    };
}

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
        Self {
            id: *ID,
            active: HashSet::new(),
            inactive: FEATURE_NAMES.keys().cloned().collect(),
        }
    }
}

impl FeatureSet {
    pub fn enabled() -> Self {
        Self {
            id: *ID,
            active: FEATURE_NAMES.keys().cloned().collect(),
            inactive: HashSet::new(),
        }
    }
}
