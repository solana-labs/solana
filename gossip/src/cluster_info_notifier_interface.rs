use crate::legacy_contact_info::LegacyContactInfo;
use {
    solana_sdk::pubkey::Pubkey,
    std::sync::{Arc, RwLock},
};

pub trait ClusterInfoNotifierInterface: std::fmt::Debug {
    /// Notified when an cluster node is updated (added or changed).
    fn notify_clusterinfo_update(&self, cluster_info: &LegacyContactInfo);

    /// Notified when a node is removed from the cluster
    fn notify_clusterinfo_remove(&self, pubkey: &Pubkey);
}

pub type ClusterInfoUpdateNotifierLock =
    Arc<RwLock<dyn ClusterInfoNotifierInterface + Sync + Send>>;
