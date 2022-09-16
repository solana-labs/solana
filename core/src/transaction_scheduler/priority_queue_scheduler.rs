//! A simple transaction scheduler that uses a priority queue to order transactions
//! for processing.
//!

use {
    super::{
        ProcessedPacketBatch, ScheduledPacketBatch, ScheduledPacketBatchIdGenerator,
        TransactionSchedulerBankingHandle,
    },
    crate::{
        bank_process_decision::{BankPacketProcessingDecision, BankingDecisionMaker},
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        packet_deserializer_stage::DeserializedPacketBatchGetter,
        unprocessed_packet_batches::{DeserializedPacket, UnprocessedPacketBatches},
    },
    crossbeam_channel::RecvTimeoutError,
    solana_runtime::bank_forks::BankForks,
    std::{
        rc::Rc,
        sync::{Arc, RwLock},
        time::Duration,
    },
};

const MAX_BATCH_SIZE: usize = 128;

pub struct PriorityQueueScheduler<D: DeserializedPacketBatchGetter> {
    /// Interface for getting deserialized packets
    deserialized_packet_batch_getter: D,
    /// Priority queue of transactions with a map from message hash to the deserialized packet.
    unprocessed_packets: UnprocessedPacketBatches,
    /// Packets to be held after forwarding.
    held_packets: Vec<Rc<ImmutableDeserializedPacket>>,
    /// Bank forks to be used to create the forward filter
    bank_forks: Arc<RwLock<BankForks>>,
    /// Forward packet filter
    forward_filter: Option<ForwardPacketBatchesByAccounts>,
    /// Determines how the scheduler should handle packets currently.
    banking_decision_maker: Arc<BankingDecisionMaker>,
    /// Scheduled batch currently being processed.
    current_batch: Option<(Rc<ScheduledPacketBatch>, BankPacketProcessingDecision)>,
    /// Generator for unique batch identifiers.
    batch_id_generator: ScheduledPacketBatchIdGenerator,
}

impl<D> TransactionSchedulerBankingHandle for PriorityQueueScheduler<D>
where
    D: DeserializedPacketBatchGetter,
{
    fn get_next_transaction_batch(
        &mut self,
        timeout: Duration,
    ) -> Result<Rc<ScheduledPacketBatch>, RecvTimeoutError> {
        assert!(self.current_batch.is_none());

        // If there are no packets, wait for some (timeout)
        if self.unprocessed_packets.is_empty() {
            self.receive_and_buffer_packets(timeout)?;
        }

        let decision = self.banking_decision_maker.make_decision();
        // If we're consuming, we should move held packets back into the queue.
        if matches!(decision, BankPacketProcessingDecision::Consume(_)) {}
        match decision {
            BankPacketProcessingDecision::Hold => {
                self.forward_filter = None;
                // TODO: this isn't really true...
                Err(RecvTimeoutError::Timeout)
            }
            BankPacketProcessingDecision::Consume(_) => {
                // Clear the forwarding filter
                self.forward_filter = None;
                // Move held packets back into the queue if they exist.
                self.move_held_packets();
                // Pop the next batch of packets off the queue
                let mut deserialized_packets =
                    Vec::with_capacity(self.unprocessed_packets.len().min(MAX_BATCH_SIZE));
                for _ in 0..deserialized_packets.capacity() {
                    deserialized_packets.push(
                        self.unprocessed_packets
                            .packet_priority_queue
                            .pop_max()
                            .unwrap(),
                    );
                }
                self.current_batch = Some((
                    Rc::new(ScheduledPacketBatch {
                        id: self.batch_id_generator.generate_id(),
                        processing_instruction: decision.clone().into(),
                        deserialized_packets,
                    }),
                    decision,
                ));

                Ok(self.current_batch.as_ref().unwrap().0.clone())
            }
            BankPacketProcessingDecision::Forward
            | BankPacketProcessingDecision::ForwardAndHold => {
                // Take the forwarding filter (will replace at the end of the function)
                let current_bank = self.bank_forks.read().unwrap().root_bank();
                let mut forward_filter = match self.forward_filter.take() {
                    Some(mut forward_filter) => {
                        forward_filter.current_bank = current_bank;
                        forward_filter
                    }
                    None => {
                        ForwardPacketBatchesByAccounts::new_with_default_batch_limits(current_bank)
                    }
                };

                // Use the forward filter to determine which packets should be filtered vs not.
                let mut forwardable_packets =
                    Vec::with_capacity(self.unprocessed_packets.len().min(MAX_BATCH_SIZE));
                while forwardable_packets.len() < MAX_BATCH_SIZE {
                    match self.unprocessed_packets.packet_priority_queue.pop_max() {
                        Some(packet) => {
                            if forward_filter.add_packet(packet.clone()) {
                                forwardable_packets.push(packet);
                            } else {
                                // remove the packet since we won't forward it.
                                // TODO: should we hold it if the option is ForwardAndHold?
                                self.unprocessed_packets
                                    .message_hash_to_transaction
                                    .remove(packet.message_hash())
                                    .expect("Message hash should exist in the map");
                            }
                        }
                        None => break,
                    }
                }

                // Move the forward filter back into the scheduler for the next iteration
                self.forward_filter = Some(forward_filter);

                self.current_batch = Some((
                    Rc::new(ScheduledPacketBatch {
                        id: self.batch_id_generator.generate_id(),
                        processing_instruction: decision.clone().into(),
                        deserialized_packets: forwardable_packets,
                    }),
                    decision,
                ));

                Ok(self.current_batch.as_ref().unwrap().0.clone())
            }
        }
    }

    fn complete_batch(&mut self, batch: ProcessedPacketBatch) {
        let (current_batch, packet_processing_decision) = self.current_batch.take().unwrap();
        assert_eq!(current_batch.id, batch.id);

        match packet_processing_decision {
            BankPacketProcessingDecision::Consume(_) | BankPacketProcessingDecision::Forward => {
                // Remove packets from the map if they are successfully completed, otherwise reinsert them
                // into the priority queue for retry.
                current_batch
                    .deserialized_packets
                    .iter()
                    .zip(batch.retryable_packets)
                    .for_each(|(packet, retry)| {
                        if retry {
                            // This scheduler is run inside banking stage, we just popped these packets
                            // off the queue, so we can push them back in and know we won't need to pop
                            // to keep under the max capacity.
                            self.unprocessed_packets
                                .packet_priority_queue
                                .push(packet.clone());
                        } else {
                            self.unprocessed_packets
                                .message_hash_to_transaction
                                .remove(packet.message_hash())
                                .expect("Message hash for completed packet should exist");
                        }
                    });
            }
            BankPacketProcessingDecision::ForwardAndHold => {
                // Mark forwarded packets as forwarded and move them to the held packets list.
                current_batch
                    .deserialized_packets
                    .iter()
                    .zip(batch.retryable_packets)
                    .for_each(|(packet, retry)| {
                        if retry {
                            // This indicates that the packet exceeded the forward buffer size. Drop it.
                            self.unprocessed_packets
                                .message_hash_to_transaction
                                .remove(packet.message_hash())
                                .expect("Message hash for completed packet should exist");
                        } else {
                            // Mark the packet as forwarded and move the immutable section to the held packets list.
                            let deserialized_packet = self
                                .unprocessed_packets
                                .message_hash_to_transaction
                                .get_mut(packet.message_hash())
                                .expect("Message hash for completed packet should exist");
                            deserialized_packet.forwarded = true;
                            self.held_packets
                                .push(deserialized_packet.immutable_section().clone());
                        }
                    });
            }
            BankPacketProcessingDecision::Hold => {
                panic!("Should not receive a completed batch if the decision was to hold")
            }
        }
    }

    fn join(self) -> std::thread::Result<()> {
        self.deserialized_packet_batch_getter.join()
    }
}

impl<D> PriorityQueueScheduler<D>
where
    D: DeserializedPacketBatchGetter,
{
    pub fn new(
        deserialized_packet_batch_getter: D,
        banking_decision_maker: Arc<BankingDecisionMaker>,
        bank_forks: Arc<RwLock<BankForks>>,
        capacity: usize,
    ) -> Self {
        Self {
            deserialized_packet_batch_getter,
            unprocessed_packets: UnprocessedPacketBatches::with_capacity(capacity),
            held_packets: Vec::default(),
            banking_decision_maker,
            current_batch: None,
            batch_id_generator: ScheduledPacketBatchIdGenerator::default(),
            bank_forks,
            forward_filter: None,
        }
    }

    /// Receive and buffer packets into the scheduler.
    fn receive_and_buffer_packets(&mut self, timeout: Duration) -> Result<(), RecvTimeoutError> {
        let remaining_capacity = self.get_remaining_capacity();
        let deserialzed_packets = self
            .deserialized_packet_batch_getter
            .get_deserialized_packets(timeout, remaining_capacity)?;
        let (_dropped_packets, _dropped_tracer_packets) = self.unprocessed_packets.insert_batch(
            deserialzed_packets
                .into_iter()
                .map(DeserializedPacket::from_immutable_section),
        );
        Ok(())
    }

    /// Move held packets into the unprocessed packets queue.
    fn move_held_packets(&mut self) {
        // Push elements until we hit capacity
        let remaining_capacity = self.get_remaining_capacity();
        let held_packets = self
            .held_packets
            .drain(..remaining_capacity.min(self.held_packets.len()));
        for packet in held_packets.take(remaining_capacity) {
            self.unprocessed_packets.packet_priority_queue.push(packet);
        }

        // Push_pop the rest.
        // let mut _dropped_packets = 0;
        // let mut _dropped_tracer_packets = 0;
        for packet in self.held_packets.drain(..) {
            let dropped_packet = self
                .unprocessed_packets
                .packet_priority_queue
                .push_pop_min(packet);
            self.unprocessed_packets
                .message_hash_to_transaction
                .remove(dropped_packet.message_hash())
                .expect("Packet in priorty queue should always exist in the hash map");

            // if dropped_packet.original_packet().meta.is_tracer_packet() {
            //     _dropped_tracer_packets += 1;
            // }
            // _dropped_packets += 1;
        }
    }

    /// Get the remaining capacity of the unprocessed packets queue.
    /// Note: We use the map's length since our held packets are not included in the queue.
    fn get_remaining_capacity(&self) -> usize {
        self.unprocessed_packets.capacity()
            - self.unprocessed_packets.message_hash_to_transaction.len()
    }
}
