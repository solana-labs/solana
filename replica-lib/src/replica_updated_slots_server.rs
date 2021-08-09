use tonic;
// tonic::include_proto!("accountsdb_repl");
use {
    crate::accountsdb_repl_server::{self, ReplicaUpdatedSlotsServer},
    solana_sdk::{clock::Slot, commitment_config::CommitmentLevel},
    std::{collections::HashMap, sync::RwLock},
};

/// The structure modelling the slots eligible for replication and
/// their states.
#[derive(Default)]
struct ReplicaEligibleSlotSet {
    slot_set: RwLock<HashMap<Slot, CommitmentLevel>>,
}

pub(crate) struct ReplicaUpdatedSlotsServerImpl {
    eligible_slot_set: ReplicaEligibleSlotSet,
}

impl ReplicaUpdatedSlotsServer for ReplicaUpdatedSlotsServerImpl {
    fn get_updated_slots(
        &self,
        request: &accountsdb_repl_server::ReplicaUpdatedSlotsRequest,
    ) -> Result<accountsdb_repl_server::ReplicaUpdatedSlotsResponse, tonic::Status> {
        let slot_set = self.eligible_slot_set.slot_set.read().unwrap();
        let updated_slots: Vec<u64> = slot_set
            .iter()
            .filter(|(slot, _)| **slot > request.last_replicated_slot)
            .map(|(slot, _)| *slot)
            .collect();

        Ok(accountsdb_repl_server::ReplicaUpdatedSlotsResponse { updated_slots })
    }
}

impl ReplicaUpdatedSlotsServerImpl {
    pub fn new() -> Self {
        Self {
            eligible_slot_set: ReplicaEligibleSlotSet::default(),
        }
    }

    /// Notify the slot has been optimistically confirmed
    pub fn notify_optimistically_confirmed_slot(&self, slot: Slot) {
        let mut slot_set = self.eligible_slot_set.slot_set.write().unwrap();
        slot_set.insert(slot, CommitmentLevel::Confirmed);
    }
}
