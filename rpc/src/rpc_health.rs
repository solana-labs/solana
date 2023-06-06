use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    },
};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum RpcHealthStatus {
    Ok,
    Behind { num_slots: Slot }, // Validator is behind its known validators
    Unknown,
}

pub struct RpcHealth {
    cluster_info: Arc<ClusterInfo>,
    known_validators: Option<HashSet<Pubkey>>,
    health_check_slot_distance: u64,
    override_health_check: Arc<AtomicBool>,
    startup_verification_complete: Arc<AtomicBool>,
    #[cfg(test)]
    stub_health_status: std::sync::RwLock<Option<RpcHealthStatus>>,
}

impl RpcHealth {
    pub fn new(
        cluster_info: Arc<ClusterInfo>,
        known_validators: Option<HashSet<Pubkey>>,
        health_check_slot_distance: u64,
        override_health_check: Arc<AtomicBool>,
        startup_verification_complete: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cluster_info,
            known_validators,
            health_check_slot_distance,
            override_health_check,
            startup_verification_complete,
            #[cfg(test)]
            stub_health_status: std::sync::RwLock::new(None),
        }
    }

    pub fn check(&self) -> RpcHealthStatus {
        #[cfg(test)]
        {
            if let Some(stub_health_status) = *self.stub_health_status.read().unwrap() {
                return stub_health_status;
            }
        }

        if !self.startup_verification_complete.load(Ordering::Acquire) {
            return RpcHealthStatus::Unknown;
        }

        RpcHealthStatus::Ok
    }

    #[cfg(test)]
    pub(crate) fn stub() -> Arc<Self> {
        use crate::rpc::tests::new_test_cluster_info;
        Arc::new(Self::new(
            Arc::new(new_test_cluster_info()),
            None,
            42,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(true)),
        ))
    }

    #[cfg(test)]
    pub(crate) fn stub_set_health_status(&self, stub_health_status: Option<RpcHealthStatus>) {
        *self.stub_health_status.write().unwrap() = stub_health_status;
    }
}
