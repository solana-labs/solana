use tonic;

// tonic::include_proto!("accountsdb_repl");
use {
    crate::accountsdb_repl_server::{self, ReplicaUpdatedSlotsServer},
    crossbeam_channel::{Receiver},
    solana_sdk::{clock::Slot, commitment_config::CommitmentLevel},
    std::{collections::HashMap, sync::{Arc, RwLock}, thread::{self, Builder, JoinHandle}},
};

/// The structure modelling the slots eligible for replication and
/// their states.
#[derive(Default, Clone)]
struct ReplicaEligibleSlotSet {
    slot_set: Arc<RwLock<HashMap<Slot, CommitmentLevel>>>,
}

pub(crate) struct ReplicaUpdatedSlotsServerImpl {
    eligible_slot_set: ReplicaEligibleSlotSet,
    confirmed_bank_receiver_svc: JoinHandle<()>,
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

    fn join(&self) -> thread::Result<()> {
        self.confirmed_bank_receiver_svc.join()
    }
}

impl ReplicaUpdatedSlotsServerImpl {
    pub fn new(confirmed_bank_receiver: Receiver<Slot>) -> Self {
        let eligible_slot_set = ReplicaEligibleSlotSet::default();
        Self {
            eligible_slot_set: eligible_slot_set.clone(),
            confirmed_bank_receiver_svc: Self::start_confirmed_bank_receiver(confirmed_bank_receiver, eligible_slot_set),
        }
    }

    fn start_confirmed_bank_receiver(confirmed_bank_receiver: Receiver<Slot>,
        eligible_slot_set: ReplicaEligibleSlotSet) -> JoinHandle<()> {

        Builder::new()
            .name("confirmed_bank_receiver".to_string())
            .spawn(move || {
                while let Ok(slot) = confirmed_bank_receiver.recv() {
                    let mut slot_set = eligible_slot_set.slot_set.write().unwrap();
                    slot_set.insert(slot, CommitmentLevel::Confirmed);
                }
            })
            .unwrap()
    }

}
