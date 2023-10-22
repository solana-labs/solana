use {
    super::{
        forwarder::Forwarder,
        scheduler_messages::{FinishedForwardWork, ForwardWork},
        ForwardOption,
    },
    crossbeam_channel::{Receiver, RecvError, SendError, Sender},
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum ForwardWorkerError {
    #[error("Failed to receive work from scheduler: {0}")]
    Recv(#[from] RecvError),
    #[error("Failed to send finalized forward work to scheduler: {0}")]
    Send(#[from] SendError<FinishedForwardWork>),
}

pub(crate) struct ForwardWorker {
    forward_receiver: Receiver<ForwardWork>,
    forward_option: ForwardOption,
    forwarder: Forwarder,
    forwarded_sender: Sender<FinishedForwardWork>,
}

#[allow(dead_code)]
impl ForwardWorker {
    pub fn new(
        forward_receiver: Receiver<ForwardWork>,
        forward_option: ForwardOption,
        forwarder: Forwarder,
        forwarded_sender: Sender<FinishedForwardWork>,
    ) -> Self {
        Self {
            forward_receiver,
            forward_option,
            forwarder,
            forwarded_sender,
        }
    }

    pub fn run(self) -> Result<(), ForwardWorkerError> {
        loop {
            let work = self.forward_receiver.recv()?;
            self.forward_loop(work)?;
        }
    }

    fn forward_loop(&self, work: ForwardWork) -> Result<(), ForwardWorkerError> {
        for work in try_drain_iter(work, &self.forward_receiver) {
            let (res, _num_packets, _forward_us, _leader_pubkey) = self.forwarder.forward_packets(
                &self.forward_option,
                work.packets.iter().map(|p| p.original_packet()),
            );
            match res {
                Ok(()) => self.forwarded_sender.send(FinishedForwardWork {
                    work,
                    successful: true,
                })?,
                Err(_err) => return self.failed_forward_drain(work),
            };
        }
        Ok(())
    }

    fn failed_forward_drain(&self, work: ForwardWork) -> Result<(), ForwardWorkerError> {
        for work in try_drain_iter(work, &self.forward_receiver) {
            self.forwarded_sender.send(FinishedForwardWork {
                work,
                successful: false,
            })?;
        }
        Ok(())
    }
}

/// Helper function to create an non-blocking iterator over work in the receiver,
/// starting with the given work item.
fn try_drain_iter<T>(work: T, receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    std::iter::once(work).chain(receiver.try_iter())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::{
            immutable_deserialized_packet::ImmutableDeserializedPacket,
            scheduler_messages::TransactionId,
            tests::{create_slow_genesis_config, new_test_cluster_info, simulate_poh},
        },
        crossbeam_channel::unbounded,
        solana_client::connection_cache::ConnectionCache,
        solana_ledger::{
            blockstore::Blockstore, genesis_utils::GenesisConfigInfo,
            get_tmp_ledger_path_auto_delete, leader_schedule_cache::LeaderScheduleCache,
        },
        solana_perf::packet::to_packet_batches,
        solana_poh::poh_recorder::{PohRecorder, WorkingBankEntry},
        solana_runtime::{bank::Bank, bank_forks::BankForks},
        solana_sdk::{
            genesis_config::GenesisConfig, poh_config::PohConfig, pubkey::Pubkey,
            signature::Keypair, system_transaction,
        },
        std::{
            sync::{atomic::AtomicBool, Arc, RwLock},
            thread::JoinHandle,
        },
        tempfile::TempDir,
    };

    // Helper struct to create tests that hold channels, files, etc.
    // such that our tests can be more easily set up and run.
    struct TestFrame {
        mint_keypair: Keypair,
        genesis_config: GenesisConfig,
        _ledger_path: TempDir,
        _entry_receiver: Receiver<WorkingBankEntry>,
        _poh_simulator: JoinHandle<()>,

        forward_sender: Sender<ForwardWork>,
        forwarded_receiver: Receiver<FinishedForwardWork>,
    }

    fn setup_test_frame() -> (TestFrame, ForwardWorker) {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_slow_genesis_config(10_000);
        let bank = Bank::new_no_wallclock_throttle_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank = bank_forks.read().unwrap().working_bank();

        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path())
            .expect("Expected to be able to open database ledger");
        let (poh_recorder, entry_receiver, record_receiver) = PohRecorder::new(
            bank.tick_height(),
            bank.last_blockhash(),
            bank.clone(),
            Some((4, 4)),
            bank.ticks_per_slot(),
            &Pubkey::new_unique(),
            Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &PohConfig::default(),
            Arc::new(AtomicBool::default()),
        );
        let poh_recorder = Arc::new(RwLock::new(poh_recorder));
        let poh_simulator = simulate_poh(record_receiver, &poh_recorder);

        let (_local_node, cluster_info) = new_test_cluster_info(None);
        let cluster_info = Arc::new(cluster_info);
        let forwarder = Forwarder::new(
            poh_recorder,
            bank_forks,
            cluster_info,
            Arc::new(ConnectionCache::new("test")),
            Arc::default(),
        );

        let (forward_sender, forward_receiver) = unbounded();
        let (forwarded_sender, forwarded_receiver) = unbounded();
        let worker = ForwardWorker::new(
            forward_receiver,
            ForwardOption::ForwardTransaction,
            forwarder,
            forwarded_sender,
        );

        (
            TestFrame {
                mint_keypair,
                genesis_config,
                _ledger_path: ledger_path,
                _entry_receiver: entry_receiver,
                _poh_simulator: poh_simulator,
                forward_sender,
                forwarded_receiver,
            },
            worker,
        )
    }

    #[test]
    fn test_worker_forward_simple() {
        let (test_frame, worker) = setup_test_frame();
        let TestFrame {
            mint_keypair,
            genesis_config,
            forward_sender,
            forwarded_receiver,
            ..
        } = &test_frame;
        let worker_thread = std::thread::spawn(move || worker.run());

        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let txs = vec![
            system_transaction::transfer(mint_keypair, &pubkey1, 2, genesis_config.hash()),
            system_transaction::transfer(mint_keypair, &pubkey2, 2, genesis_config.hash()),
        ];

        let id1 = TransactionId::new(1);
        let id2 = TransactionId::new(0);

        let packets = to_packet_batches(&txs, 2);
        assert_eq!(packets.len(), 1);
        let packets = packets[0]
            .into_iter()
            .cloned()
            .map(|p| ImmutableDeserializedPacket::new(p).unwrap())
            .map(Arc::new)
            .collect();
        forward_sender
            .send(ForwardWork {
                packets,
                ids: vec![id1, id2],
            })
            .unwrap();
        let forwarded = forwarded_receiver.recv().unwrap();
        assert_eq!(forwarded.work.ids, vec![id1, id2]);
        assert!(forwarded.successful);

        drop(test_frame);
        let _ = worker_thread.join().unwrap();
    }
}
