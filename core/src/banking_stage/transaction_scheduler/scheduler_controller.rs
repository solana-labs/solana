//! Control flow for BankingStage's transaction scheduler.
//!

use {
    super::{
        prio_graph_scheduler::PrioGraphScheduler, scheduler_error::SchedulerError,
        transaction_id_generator::TransactionIdGenerator,
        transaction_state::SanitizedTransactionTTL,
        transaction_state_container::TransactionStateContainer,
    },
    crate::banking_stage::{
        decision_maker::{BufferedPacketsDecision, DecisionMaker},
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        packet_deserializer::PacketDeserializer,
        TOTAL_BUFFERED_PACKETS,
    },
    crossbeam_channel::RecvTimeoutError,
    solana_runtime::bank_forks::BankForks,
    std::{
        sync::{Arc, RwLock},
        time::Duration,
    },
};

/// Controls packet and transaction flow into scheduler, and scheduling execution.
pub(crate) struct SchedulerController {
    /// Decision maker for determining what should be done with transactions.
    decision_maker: DecisionMaker,
    /// Packet/Transaction ingress.
    packet_receiver: PacketDeserializer,
    bank_forks: Arc<RwLock<BankForks>>,
    /// Generates unique IDs for incoming transactions.
    transaction_id_generator: TransactionIdGenerator,
    /// Container for transaction state.
    /// Shared resource between `packet_receiver` and `scheduler`.
    container: TransactionStateContainer,
    /// State for scheduling and communicating with worker threads.
    scheduler: PrioGraphScheduler,
}

impl SchedulerController {
    pub fn new(
        decision_maker: DecisionMaker,
        packet_deserializer: PacketDeserializer,
        bank_forks: Arc<RwLock<BankForks>>,
        scheduler: PrioGraphScheduler,
    ) -> Self {
        Self {
            decision_maker,
            packet_receiver: packet_deserializer,
            bank_forks,
            transaction_id_generator: TransactionIdGenerator::default(),
            container: TransactionStateContainer::with_capacity(TOTAL_BUFFERED_PACKETS),
            scheduler,
        }
    }

    pub fn run(mut self) -> Result<(), SchedulerError> {
        loop {
            let decision = self.decision_maker.make_consume_or_forward_decision();
            self.process_transactions(&decision)?;
            self.scheduler.receive_completed(&mut self.container)?;
            if self.receive_packets(&decision) {
                break;
            }
        }

        Ok(())
    }

    /// Process packets based on decision.
    fn process_transactions(
        &mut self,
        decision: &BufferedPacketsDecision,
    ) -> Result<(), SchedulerError> {
        match decision {
            BufferedPacketsDecision::Consume(_bank_start) => {
                let _num_scheduled = self.scheduler.schedule(&mut self.container)?;
            }
            BufferedPacketsDecision::Forward => {
                self.clear_container();
            }
            BufferedPacketsDecision::ForwardAndHold | BufferedPacketsDecision::Hold => {}
        }

        Ok(())
    }

    /// Clears the transaction state container.
    /// This only clears pending transactions, and does **not** clear in-flight transactions.
    fn clear_container(&mut self) {
        while let Some(id) = self.container.pop() {
            self.container.remove_by_id(&id.id);
        }
    }

    /// Returns whether the packet receiver was disconnected.
    fn receive_packets(&mut self, decision: &BufferedPacketsDecision) -> bool {
        let remaining_queue_capacity = self.container.remaining_queue_capacity();

        let (recv_timeout, should_buffer) = match decision {
            BufferedPacketsDecision::Consume(_) => (
                if self.container.is_empty() {
                    Duration::from_millis(100)
                } else {
                    Duration::from_millis(0)
                },
                true,
            ),
            BufferedPacketsDecision::Forward => (Duration::from_millis(100), false),
            BufferedPacketsDecision::ForwardAndHold | BufferedPacketsDecision::Hold => {
                (Duration::from_millis(100), true)
            }
        };

        let received_packet_results = self
            .packet_receiver
            .receive_packets(recv_timeout, remaining_queue_capacity);

        match (received_packet_results, should_buffer) {
            (Ok(receive_packet_results), true) => {
                self.buffer_packets(receive_packet_results.deserialized_packets)
            }
            (Ok(receive_packet_results), false) => drop(receive_packet_results),
            (Err(RecvTimeoutError::Timeout), _) => {}
            (Err(RecvTimeoutError::Disconnected), _) => return true,
        }

        false
    }

    fn buffer_packets(&mut self, packets: Vec<ImmutableDeserializedPacket>) {
        // Sanitize packets, generate IDs, and insert into the container.
        let bank = self.bank_forks.read().unwrap().working_bank();
        let last_slot_in_epoch = bank.epoch_schedule().get_last_slot_in_epoch(bank.epoch());
        let feature_set = &bank.feature_set;
        let vote_only = bank.vote_only_bank();
        for packet in packets {
            let Some(transaction) =
                packet.build_sanitized_transaction(feature_set, vote_only, bank.as_ref())
            else {
                continue;
            };

            let transaction_id = self.transaction_id_generator.next();
            let transaction_ttl = SanitizedTransactionTTL {
                transaction,
                max_age_slot: last_slot_in_epoch,
            };
            let transaction_priority_details = packet.priority_details();
            self.container.insert_new_transaction(
                transaction_id,
                transaction_ttl,
                transaction_priority_details,
            );
        }
    }
}
