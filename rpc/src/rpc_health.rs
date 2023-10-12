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

        if !self.startup_verification_complete.load(Ordering::Acquire) {
            return RpcHealthStatus::Unknown;
        }
        if self.override_health_check.load(Ordering::Relaxed) {
            return RpcHealthStatus::Ok;
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
            > cluster_latest_optimistically_confirmed_slot
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
