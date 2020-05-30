use crate::cluster_info::ClusterInfo;
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
};

#[derive(PartialEq)]
pub enum RpcHealthStatus {
    Ok,
    Behind, // Validator is behind its trusted validators
}

pub struct RpcHealth {
    cluster_info: Arc<ClusterInfo>,
    trusted_validators: Option<HashSet<Pubkey>>,
    health_check_slot_distance: u64,
    override_health_check: Arc<AtomicBool>,
}

impl RpcHealth {
    pub fn new(
        cluster_info: Arc<ClusterInfo>,
        trusted_validators: Option<HashSet<Pubkey>>,
        health_check_slot_distance: u64,
        override_health_check: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cluster_info,
            trusted_validators,
            health_check_slot_distance,
            override_health_check,
        }
    }

    pub fn check(&self) -> RpcHealthStatus {
        if self.override_health_check.load(Ordering::Relaxed) {
            RpcHealthStatus::Ok
        } else if let Some(trusted_validators) = &self.trusted_validators {
            let (latest_account_hash_slot, latest_trusted_validator_account_hash_slot) = {
                (
                    self.cluster_info
                        .get_accounts_hash_for_node(&self.cluster_info.id(), |hashes| {
                            hashes
                                .iter()
                                .max_by(|a, b| a.0.cmp(&b.0))
                                .map(|slot_hash| slot_hash.0)
                        })
                        .flatten()
                        .unwrap_or(0),
                    trusted_validators
                        .iter()
                        .map(|trusted_validator| {
                            self.cluster_info
                                .get_accounts_hash_for_node(&trusted_validator, |hashes| {
                                    hashes
                                        .iter()
                                        .max_by(|a, b| a.0.cmp(&b.0))
                                        .map(|slot_hash| slot_hash.0)
                                })
                                .flatten()
                                .unwrap_or(0)
                        })
                        .max()
                        .unwrap_or(0),
                )
            };

            // This validator is considered healthy if its latest account hash slot is within
            // `health_check_slot_distance` of the latest trusted validator's account hash slot
            if latest_account_hash_slot > 0
                && latest_trusted_validator_account_hash_slot > 0
                && latest_account_hash_slot
                    > latest_trusted_validator_account_hash_slot
                        .saturating_sub(self.health_check_slot_distance)
            {
                RpcHealthStatus::Ok
            } else {
                warn!(
                    "health check: me={}, latest trusted_validator={}",
                    latest_account_hash_slot, latest_trusted_validator_account_hash_slot
                );
                RpcHealthStatus::Behind
            }
        } else {
            // No trusted validator point of reference available, so this validator is healthy
            // because it's running
            RpcHealthStatus::Ok
        }
    }

    #[cfg(test)]
    pub(crate) fn stub() -> Arc<Self> {
        Arc::new(Self::new(
            Arc::new(ClusterInfo::new_with_invalid_keypair(
                crate::contact_info::ContactInfo::default(),
            )),
            None,
            42,
            Arc::new(AtomicBool::new(false)),
        ))
    }
}
