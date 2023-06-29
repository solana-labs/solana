use {
    super::{
        central_scheduler_banking_stage::SchedulerError,
        transaction_packet_container::TransactionPacketContainer,
        transaction_priority_id::TransactionPriorityId,
    },
    crate::banking_stage::{
        consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
        immutable_deserialized_packet::ImmutableDeserializedPacket,
        multi_iterator_scanner::{MultiIteratorScanner, ProcessingDecision},
        read_write_account_set::ReadWriteAccountSet,
        scheduler_messages::{ForwardWork, TransactionId},
        unprocessed_packet_batches::DeserializedPacket,
    },
    crossbeam_channel::Sender,
    itertools::Itertools,
    solana_perf::perf_libs,
    solana_runtime::{
        bank::{Bank, BankStatusCache},
        blockhash_queue::BlockhashQueue,
        transaction_error_metrics::TransactionErrorMetrics,
    },
    solana_sdk::{
        clock::{
            FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET, MAX_PROCESSING_AGE,
            MAX_TRANSACTION_FORWARDING_DELAY, MAX_TRANSACTION_FORWARDING_DELAY_GPU,
        },
        nonce::state::DurableNonce,
        transaction::SanitizedTransaction,
    },
    std::sync::{Arc, RwLockReadGuard},
};

/// Interface to perform scheduling for forwarding transactions.
/// Using a multi-iterator approach.
pub struct MultiIteratorForwardScheduler {
    forward_work_sender: Sender<ForwardWork>,
}

impl MultiIteratorForwardScheduler {
    pub fn new(forward_work_sender: Sender<ForwardWork>) -> Self {
        Self {
            forward_work_sender,
        }
    }

    pub(crate) fn schedule(
        &mut self,
        bank: &Bank,
        container: &mut TransactionPacketContainer,
        hold: bool,
    ) -> Result<(), SchedulerError> {
        let blockhash_queue = bank.read_blockhash_queue().unwrap();
        let status_cache = bank.status_cache.read().unwrap();
        let next_durable_nonce = DurableNonce::from_blockhash(&blockhash_queue.last_hash());
        let max_age = Self::max_forwading_age();
        let active_scheduler = ActiveMultiIteratorForwardScheduler {
            bank,
            blockhash_queue,
            status_cache,
            next_durable_nonce,
            max_age,
            error_counters: TransactionErrorMetrics::default(),
            batch: Batch::new(),
            batch_account_locks: ReadWriteAccountSet::default(),
        };

        const MAX_TRANSACTIONS_PER_SCHEDULING_PASS: usize = 100_000;
        let target_scanner_batch_size: usize = TARGET_NUM_TRANSACTIONS_PER_BATCH;
        let ids = container
            .take_top_n(MAX_TRANSACTIONS_PER_SCHEDULING_PASS)
            .collect_vec();

        let mut scanner = MultiIteratorScanner::new(
            &ids,
            target_scanner_batch_size,
            active_scheduler,
            |id, payload| payload.should_schedule(id, container),
        );

        while let Some((ids, payload)) = scanner.iterate() {
            if !ids.is_empty() {
                let (ids, packets) = payload.batch.take_batch();
                assert!(!ids.is_empty());
                assert!(!packets.is_empty());

                let work = ForwardWork { ids, packets };
                self.forward_work_sender
                    .send(work)
                    .map_err(|_| SchedulerError::DisconnectedSendChannel("forward work sender"))?;

                // clear account locks for the next iteration
                payload.batch_account_locks.clear();
            }
        }

        if !hold {
            for id in ids {
                container.remove_by_id(&id.id);
            }
        }

        Ok(())
    }

    fn max_forwading_age() -> usize {
        (MAX_PROCESSING_AGE)
            .saturating_sub(if perf_libs::api().is_some() {
                MAX_TRANSACTION_FORWARDING_DELAY
            } else {
                MAX_TRANSACTION_FORWARDING_DELAY_GPU
            })
            .saturating_sub(FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET as usize)
    }
}

struct ActiveMultiIteratorForwardScheduler<'a> {
    bank: &'a Bank,
    blockhash_queue: RwLockReadGuard<'a, BlockhashQueue>,
    status_cache: RwLockReadGuard<'a, BankStatusCache>,
    next_durable_nonce: DurableNonce,
    max_age: usize,
    error_counters: TransactionErrorMetrics,

    batch: Batch,
    batch_account_locks: ReadWriteAccountSet,
}

struct Batch {
    ids: Vec<TransactionId>,
    packets: Vec<Arc<ImmutableDeserializedPacket>>,
}

impl Batch {
    fn new() -> Self {
        Self {
            ids: Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
            packets: Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
        }
    }

    fn take_batch(&mut self) -> (Vec<TransactionId>, Vec<Arc<ImmutableDeserializedPacket>>) {
        (
            core::mem::replace(
                &mut self.ids,
                Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
            ),
            core::mem::replace(
                &mut self.packets,
                Vec::with_capacity(TARGET_NUM_TRANSACTIONS_PER_BATCH),
            ),
        )
    }
}

impl<'a> ActiveMultiIteratorForwardScheduler<'a> {
    fn should_schedule(
        &mut self,
        id: &TransactionPriorityId,
        container: &mut TransactionPacketContainer,
    ) -> ProcessingDecision {
        let transaction_ttl = container.get_transaction(&id.id);
        let packet = container.get_packet(&id.id).expect("packet must exist");
        let decision = self.make_scheduling_decision(&transaction_ttl.transaction, packet);

        match decision {
            ProcessingDecision::Now => {
                self.batch.ids.push(id.id);
                self.batch.packets.push(packet.immutable_section().clone());
            }
            ProcessingDecision::Later | ProcessingDecision::Never => {}
        }

        decision
    }

    fn make_scheduling_decision(
        &mut self,
        transaction: &SanitizedTransaction,
        packet: &DeserializedPacket,
    ) -> ProcessingDecision {
        if packet.forwarded {
            return ProcessingDecision::Never;
        }

        if self
            .bank
            .check_transaction_age(
                transaction,
                self.max_age,
                &self.next_durable_nonce,
                &self.blockhash_queue,
                &mut self.error_counters,
            )
            .0
            .is_err()
        {
            return ProcessingDecision::Never;
        }

        if self
            .bank
            .is_transaction_already_processed(transaction, &self.status_cache)
        {
            self.error_counters.already_processed += 1;
            return ProcessingDecision::Never;
        }

        if self.batch_account_locks.try_locking(transaction.message()) {
            ProcessingDecision::Now
        } else {
            ProcessingDecision::Later
        }
    }
}
