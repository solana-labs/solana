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
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        packet_deserializer_stage::DeserializedPacketBatchGetter,
        unprocessed_packet_batches::{DeserializedPacket, UnprocessedPacketBatches},
    },
    crossbeam_channel::RecvTimeoutError,
    std::{rc::Rc, time::Duration},
};

const MAX_BATCH_SIZE: usize = 128;

pub struct PriorityQueueScheduler<D: DeserializedPacketBatchGetter> {
    /// Interface for getting deserialized packets
    deserialized_packet_batch_getter: D,
    /// Priority queue of transactions with a map from message hash to the deserialized packet.
    unprocessed_packets: UnprocessedPacketBatches,
    /// Packets to be held after forwarding.
    held_packets: Vec<Rc<ImmutableDeserializedPacket>>,
    /// Determines how the scheduler should handle packets currently.
    banking_descision_maker: BankingDecisionMaker,
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

        let decision = self.banking_descision_maker.make_decision();
        // If we're consuming, we should move held packets back into the queue.
        if matches!(decision, BankPacketProcessingDecision::Consume(_)) {
            self.move_held_packets();
        }
        match decision {
            BankPacketProcessingDecision::Hold => Err(RecvTimeoutError::Timeout), // TODO: this isn't really true...
            _ => {
                // Pop max priority packets off the queue and insert into the batch
                let deserialized_packets = self
                    .unprocessed_packets
                    .packet_priority_queue
                    .drain_desc()
                    .take(MAX_BATCH_SIZE)
                    .collect();

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
}

impl<D> PriorityQueueScheduler<D>
where
    D: DeserializedPacketBatchGetter,
{
    pub fn new(
        deserialized_packet_batch_getter: D,
        banking_descision_maker: BankingDecisionMaker,
        capacity: usize,
    ) -> Self {
        Self {
            deserialized_packet_batch_getter,
            unprocessed_packets: UnprocessedPacketBatches::with_capacity(capacity),
            held_packets: Vec::default(),
            banking_descision_maker,
            current_batch: None,
            batch_id_generator: ScheduledPacketBatchIdGenerator::default(),
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
        let held_packets = self.held_packets.drain(..remaining_capacity);
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
