use {
    crate::optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
    solana_ledger::blockstore::Blockstore,
    solana_sdk::clock::Slot,
    std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum RpcHealthStatus {
    Ok,
    Behind { num_slots: Slot }, // Validator is behind its known validators
    Unknown,
}

pub struct RpcHealth {
    optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
    blockstore: Arc<Blockstore>,
    health_check_slot_distance: u64,
    override_health_check: Arc<AtomicBool>,
    startup_verification_complete: Arc<AtomicBool>,
    #[cfg(test)]
    stub_health_status: std::sync::RwLock<Option<RpcHealthStatus>>,
}

impl RpcHealth {
    pub fn new(
        optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
        blockstore: Arc<Blockstore>,
        health_check_slot_distance: u64,
        override_health_check: Arc<AtomicBool>,
        startup_verification_complete: Arc<AtomicBool>,
    ) -> Self {
        Self {
            optimistically_confirmed_bank,
            blockstore,
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

        if self.override_health_check.load(Ordering::Relaxed) {
            return RpcHealthStatus::Ok;
        }
        if !self.startup_verification_complete.load(Ordering::Acquire) {
            return RpcHealthStatus::Unknown;
        }

        // A node can observe votes by both replaying blocks and observing gossip.
        //
        // ClusterInfoVoteListener receives votes from both of these sources and then records
        // optimistically confirmed slots in the Blockstore via OptimisticConfirmationVerifier.
        // Thus, it is possible for a node to record an optimistically confirmed slot before the
        // node has replayed and validated the slot for itself.
        //
        // OptimisticallyConfirmedBank holds a bank for the latest optimistically confirmed slot
        // that the node has replayed. It is true that the node will have replayed that slot by
        // virtue of having a bank available. Observing that the cluster has optimistically
        // confirmed a slot through gossip is not enough to reconstruct the bank.
        //
        // So, comparing the latest optimistic slot from the Blockstore vs. the slot from the
        // OptimisticallyConfirmedBank bank allows a node to see where it stands in relation to the
        // tip of the cluster.
        let my_latest_optimistically_confirmed_slot = self
            .optimistically_confirmed_bank
            .read()
            .unwrap()
            .bank
            .slot();

        let mut optimistic_slot_infos = match self.blockstore.get_latest_optimistic_slots(1) {
            Ok(infos) => infos,
            Err(err) => {
                warn!("health check: blockstore error: {err}");
                return RpcHealthStatus::Unknown;
            }
        };
        let Some((cluster_latest_optimistically_confirmed_slot, _, _)) =
            optimistic_slot_infos.pop()
        else {
            warn!("health check: blockstore does not contain any optimistically confirmed slots");
            return RpcHealthStatus::Unknown;
        };

        if my_latest_optimistically_confirmed_slot
            >= cluster_latest_optimistically_confirmed_slot
                .saturating_sub(self.health_check_slot_distance)
        {
            RpcHealthStatus::Ok
        } else {
            let num_slots = cluster_latest_optimistically_confirmed_slot
                .saturating_sub(my_latest_optimistically_confirmed_slot);
            warn!(
                "health check: behind by {num_slots} \
                slots: me={my_latest_optimistically_confirmed_slot}, \
                latest cluster={cluster_latest_optimistically_confirmed_slot}",
            );
            RpcHealthStatus::Behind { num_slots }
        }
    }

    #[cfg(test)]
    pub(crate) fn stub(
        optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
        blockstore: Arc<Blockstore>,
    ) -> Arc<Self> {
        Arc::new(Self::new(
            optimistically_confirmed_bank,
            blockstore,
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

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_ledger::{
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            get_tmp_ledger_path_auto_delete,
        },
        solana_runtime::{bank::Bank, bank_forks::BankForks},
        solana_sdk::{clock::UnixTimestamp, hash::Hash, pubkey::Pubkey},
    };

    #[test]
    fn test_get_health() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let bank0 = bank_forks.read().unwrap().root_bank();
        assert!(bank0.slot() == 0);

        let health_check_slot_distance = 10;
        let override_health_check = Arc::new(AtomicBool::new(true));
        let startup_verification_complete = Arc::clone(bank0.get_startup_verification_complete());
        let health = RpcHealth::new(
            optimistically_confirmed_bank.clone(),
            blockstore.clone(),
            health_check_slot_distance,
            override_health_check.clone(),
            startup_verification_complete,
        );

        // Override health check set to true - status is ok
        assert_eq!(health.check(), RpcHealthStatus::Ok);

        // Remove the override - status now unknown with incomplete startup verification
        override_health_check.store(false, Ordering::Relaxed);
        assert_eq!(health.check(), RpcHealthStatus::Unknown);

        // Mark startup verification complete - status still unknown as no slots have been
        // optimistically confirmed yet
        bank0.set_startup_verification_complete();
        assert_eq!(health.check(), RpcHealthStatus::Unknown);

        // Mark slot 15 as being optimistically confirmed in the Blockstore, this could
        // happen if the cluster confirmed the slot and this node became aware through gossip,
        // but this node has not yet replayed slot 15. The local view of the latest optimistic
        // slot is still slot 0 so status will be behind
        blockstore
            .insert_optimistic_slot(15, &Hash::default(), UnixTimestamp::default())
            .unwrap();
        assert_eq!(health.check(), RpcHealthStatus::Behind { num_slots: 15 });

        // Simulate this node observing slot 4 as optimistically confirmed - status still behind
        let bank4 = Arc::new(Bank::new_from_parent(bank0, &Pubkey::default(), 4));
        optimistically_confirmed_bank.write().unwrap().bank = bank4.clone();
        assert_eq!(health.check(), RpcHealthStatus::Behind { num_slots: 11 });

        // Simulate this node observing slot 5 as optimistically confirmed - status now ok
        // as distance is <= health_check_slot_distance
        let bank5 = Arc::new(Bank::new_from_parent(bank4, &Pubkey::default(), 5));
        optimistically_confirmed_bank.write().unwrap().bank = bank5.clone();
        assert_eq!(health.check(), RpcHealthStatus::Ok);

        // Node now up with tip of cluster
        let bank15 = Arc::new(Bank::new_from_parent(bank5, &Pubkey::default(), 15));
        optimistically_confirmed_bank.write().unwrap().bank = bank15.clone();
        assert_eq!(health.check(), RpcHealthStatus::Ok);

        // Node "beyond" tip of cluster - this technically isn't possible but could be
        // observed locally due to a race between updates to Blockstore and
        // OptimisticallyConfirmedBank. Either way, not a problem and status is ok.
        let bank16 = Arc::new(Bank::new_from_parent(bank15, &Pubkey::default(), 16));
        optimistically_confirmed_bank.write().unwrap().bank = bank16.clone();
        assert_eq!(health.check(), RpcHealthStatus::Ok);
    }
}
