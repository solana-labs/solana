use {
    crate::accountsdb_repl_server::{self, ReplicaSlotConfirmationServer},
    crossbeam_channel::Receiver,
    solana_sdk::{clock::Slot, commitment_config::CommitmentLevel},
    std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
    tonic,
};

/// The structure modelling the slots eligible for replication and
/// their states.
#[derive(Default, Clone)]
struct ReplicaEligibleSlotSet {
    slot_set: Arc<RwLock<VecDeque<(Slot, CommitmentLevel)>>>,
}

pub(crate) struct ReplicaSlotConfirmationServerImpl {
    eligible_slot_set: ReplicaEligibleSlotSet,
    confirmed_bank_receiver_service: Option<JoinHandle<()>>,
    cleanup_service: Option<JoinHandle<()>>,
    exit_updated_slot_server: Arc<AtomicBool>,
}

impl ReplicaSlotConfirmationServer for ReplicaSlotConfirmationServerImpl {
    fn get_confirmed_slots(
        &self,
        request: &accountsdb_repl_server::ReplicaSlotConfirmationRequest,
    ) -> Result<accountsdb_repl_server::ReplicaSlotConfirmationResponse, tonic::Status> {
        let slot_set = self.eligible_slot_set.slot_set.read().unwrap();
        let updated_slots: Vec<u64> = slot_set
            .iter()
            .filter(|(slot, _)| *slot > request.last_replicated_slot)
            .map(|(slot, _)| *slot)
            .collect();

        Ok(accountsdb_repl_server::ReplicaSlotConfirmationResponse { updated_slots })
    }

    fn join(&mut self) -> thread::Result<()> {
        self.exit_updated_slot_server.store(true, Ordering::Relaxed);
        self.confirmed_bank_receiver_service
            .take()
            .map(JoinHandle::join)
            .unwrap()
            .expect("confirmed_bank_receiver_service");

        self.cleanup_service.take().map(JoinHandle::join).unwrap()
    }
}

const MAX_ELIGIBLE_SLOT_SET_SIZE: usize = 262144;

impl ReplicaSlotConfirmationServerImpl {
    pub fn new(confirmed_bank_receiver: Receiver<Slot>) -> Self {
        let eligible_slot_set = ReplicaEligibleSlotSet::default();
        let exit_updated_slot_server = Arc::new(AtomicBool::new(false));

        Self {
            eligible_slot_set: eligible_slot_set.clone(),
            confirmed_bank_receiver_service: Some(Self::run_confirmed_bank_receiver(
                confirmed_bank_receiver,
                eligible_slot_set.clone(),
                exit_updated_slot_server.clone(),
            )),
            cleanup_service: Some(Self::run_cleanup_service(
                eligible_slot_set,
                MAX_ELIGIBLE_SLOT_SET_SIZE,
                exit_updated_slot_server.clone(),
            )),
            exit_updated_slot_server,
        }
    }

    fn run_confirmed_bank_receiver(
        confirmed_bank_receiver: Receiver<Slot>,
        eligible_slot_set: ReplicaEligibleSlotSet,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("confirmed_bank_receiver".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    if let Ok(slot) = confirmed_bank_receiver.recv() {
                        let mut slot_set = eligible_slot_set.slot_set.write().unwrap();
                        slot_set.push_back((slot, CommitmentLevel::Confirmed));
                    }
                }
            })
            .unwrap()
    }

    fn run_cleanup_service(
        eligible_slot_set: ReplicaEligibleSlotSet,
        max_set_size: usize,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("cleanup_service".to_string())
            .spawn(move || {
                while !exit.load(Ordering::Relaxed) {
                    let mut slot_set = eligible_slot_set.slot_set.write().unwrap();
                    let count_to_drain = slot_set.len().saturating_sub(max_set_size);
                    if count_to_drain > 0 {
                        drop(slot_set.drain(..count_to_drain));
                    }
                    drop(slot_set);
                    sleep(Duration::from_millis(200));
                }
            })
            .unwrap()
    }
}
