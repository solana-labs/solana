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

        if self.override_health_check.load(Ordering::Relaxed) {
            RpcHealthStatus::Ok
        } else if let Some(known_validators) = &self.known_validators {
            match (
                self.cluster_info
                    .get_accounts_hash_for_node(&self.cluster_info.id(), |hashes| {
                        hashes
                            .iter()
                            .max_by(|a, b| a.0.cmp(&b.0))
                            .map(|slot_hash| slot_hash.0)
                    })
                    .flatten(),
                known_validators
                    .iter()
                    .filter_map(|known_validator| {
                        self.cluster_info
                            .get_accounts_hash_for_node(known_validator, |hashes| {
                                hashes
                                    .iter()
                                    .max_by(|a, b| a.0.cmp(&b.0))
                                    .map(|slot_hash| slot_hash.0)
                            })
                            .flatten()
                    })
                    .max(),
            ) {
                (
                    Some(latest_account_hash_slot),
                    Some(latest_known_validator_account_hash_slot),
                ) => {
                    // The validator is considered healthy if its latest account hash slot is within
                    // `health_check_slot_distance` of the latest known validator's account hash slot
                    if latest_account_hash_slot
                        > latest_known_validator_account_hash_slot
                            .saturating_sub(self.health_check_slot_distance)
                    {
                        RpcHealthStatus::Ok
                    } else {
                        let num_slots = latest_known_validator_account_hash_slot
                            .saturating_sub(latest_account_hash_slot);
                        warn!(
                            "health check: behind by {} slots: me={}, latest known_validator={}",
                            num_slots,
                            latest_account_hash_slot,
                            latest_known_validator_account_hash_slot
                        );
                        RpcHealthStatus::Behind { num_slots }
                    }
                }
                (latest_account_hash_slot, latest_known_validator_account_hash_slot) => {
                    if latest_account_hash_slot.is_none() {
                        warn!("health check: latest_account_hash_slot not available");
                    }
                    if latest_known_validator_account_hash_slot.is_none() {
                        warn!(
                            "health check: latest_known_validator_account_hash_slot not available"
                        );
                    }
                    RpcHealthStatus::Unknown
                }
            }
        } else {
            // No known validator point of reference available, so this validator is healthy
            // because it's running
            RpcHealthStatus::Ok
        }
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
