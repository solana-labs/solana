use crate::banking_stage::forward_packet_batches_by_accounts::{
    ForwardBatch, FORWARDED_BLOCK_COMPUTE_RATIO,
};

use {
    super::{
        central_scheduler_banking_stage::SchedulerError,
        transaction_packet_container::TransactionPacketContainer,
        transaction_priority_id::TransactionPriorityId,
    },
    crate::banking_stage::{
        consumer::TARGET_NUM_TRANSACTIONS_PER_BATCH,
        multi_iterator_scanner::{MultiIteratorScanner, ProcessingDecision},
        scheduler_messages::ForwardWork,
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
    std::sync::RwLockReadGuard,
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
            forward_batch: ForwardBatch::new(FORWARDED_BLOCK_COMPUTE_RATIO),
        };

        const MAX_TRANSACTIONS_PER_SCHEDULING_PASS: usize = 100_000;
        const TARGET_SCANNER_BATCH_SIZE: usize = 20 * TARGET_NUM_TRANSACTIONS_PER_BATCH;
        let ids = container
            .take_top_n(MAX_TRANSACTIONS_PER_SCHEDULING_PASS)
            .collect_vec();

        let mut scanner = MultiIteratorScanner::new(
            &ids,
            TARGET_SCANNER_BATCH_SIZE,
            active_scheduler,
            |id, payload| payload.should_schedule(id, container),
        );

        while let Some((ids, payload)) = scanner.iterate() {
            let ids = ids.iter().map(|id| id.id);
            let forwardable_packets = payload.forward_batch.get_forwardable_packets();

            for chunk in forwardable_packets
                .iter()
                .zip(ids)
                .map(|(packet, id)| (packet.clone(), id))
                .chunks(64)
                .into_iter()
            {
                let (packets, ids) = chunk.unzip();
                let work = ForwardWork { ids, packets };
                self.forward_work_sender
                    .send(work)
                    .map_err(|_| SchedulerError::DisconnectedSendChannel("forward work sender"))?;
            }

            // reset the forward batch for the next batch
            payload.forward_batch = ForwardBatch::new(FORWARDED_BLOCK_COMPUTE_RATIO);
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

    forward_batch: ForwardBatch,
}

impl<'a> ActiveMultiIteratorForwardScheduler<'a> {
    fn should_schedule(
        &mut self,
        id: &TransactionPriorityId,
        container: &mut TransactionPacketContainer,
    ) -> ProcessingDecision {
        let transaction_ttl = container.get_transaction(&id.id);
        let packet = container.get_packet(&id.id).expect("packet must exist");
        self.make_scheduling_decision(&transaction_ttl.transaction, packet)
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

        match self.forward_batch.try_add(
            transaction,
            packet.immutable_section().clone(),
            &self.bank.feature_set,
        ) {
            Ok(_) => ProcessingDecision::Now,
            Err(_) => ProcessingDecision::Later,
        }
    }
}
