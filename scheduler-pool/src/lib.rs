use {
    log::*,
    solana_ledger::blockstore_processor::{
        execute_batch, TransactionBatchWithIndexes, TransactionStatusSender,
    },
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        installed_scheduler_pool::{
            InstalledScheduler, InstalledSchedulerPool, SchedulerBox, SchedulerId,
            SchedulerPoolArc, SchedulingContext, TimingAndResult, WaitSource,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::transaction::SanitizedTransaction,
    std::sync::{Arc, Mutex, Weak},
};

#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: Mutex<Vec<SchedulerBox>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    weak: Weak<SchedulerPool>,
}

#[derive(Debug)]
struct Scheduler(
    Arc<SchedulerPool>,
    Mutex<(SchedulingContext, Option<TimingAndResult>)>,
);

impl Scheduler {
    fn spawn(scheduler_pool: Arc<SchedulerPool>, initial_context: SchedulingContext) -> Self {
        Self(scheduler_pool, Mutex::new((initial_context, None)))
    }
}

impl SchedulerPool {
    pub fn new_dyn(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> SchedulerPoolArc {
        Arc::new_cyclic(|weak_pool| Self {
            schedulers: Mutex::<Vec<SchedulerBox>>::default(),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak: weak_pool.clone(),
        })
    }
}

impl InstalledSchedulerPool for SchedulerPool {
    fn take_from_pool(&self, context: SchedulingContext) -> SchedulerBox {
        let mut schedulers = self.schedulers.lock().unwrap();
        let maybe_scheduler = schedulers.pop();
        if let Some(scheduler) = maybe_scheduler {
            scheduler.replace_scheduler_context(context);
            scheduler
        } else {
            Box::new(Scheduler::spawn(self.weak.upgrade().unwrap(), context))
        }
    }

    fn return_to_pool(&self, scheduler: SchedulerBox) {
        let mut schedulers = self.schedulers.lock().unwrap();

        trace!(
            "SchedulerPool: id_{:016x} is returned... len: {} => {}",
            scheduler.scheduler_id(),
            schedulers.len(),
            schedulers.len() + 1
        );

        schedulers.push(scheduler);
    }
}

impl InstalledScheduler for Scheduler {
    fn scheduler_id(&self) -> SchedulerId {
        0
    }

    fn scheduler_pool(&self) -> SchedulerPoolArc {
        self.0.clone()
    }

    fn schedule_execution(&self, transaction: &SanitizedTransaction, index: usize) {
        let (pool, ref mut contet_timing_and_result) = (&self.0, &*self.1.lock().unwrap());
        let (ref context, ref mut timings_and_result) = &mut contet_timing_and_result;
        let bank = context.bank();

        let transactions = [transaction.clone()];
        let batch_with_indexes = TransactionBatchWithIndexes {
            batch: bank.prepare_sanitized_batch(&transactions),
            transaction_indexes: vec![index],
        };
        let mut a = timings_and_result.get_or_insert_with(|| (ExecuteTimings::default(), Ok(())));

        if a.1.is_ok() {
            a.1 = execute_batch(
                &batch_with_indexes,
                bank,
                self.0.transaction_status_sender.as_ref(),
                self.0.replay_vote_sender.as_ref(),
                &mut a.0,
                self.0.log_messages_bytes_limit,
                &self.0.prioritization_fee_cache,
            );
        }
    }

    fn schedule_termination(&mut self) {
        // no-op
    }

    fn wait_for_termination(&mut self, wait_source: &WaitSource) -> Option<TimingAndResult> {
        match wait_source {
            WaitSource::InsideBlock => None,
            _ => self.1.lock().unwrap().1.take(),
        }
    }

    fn replace_scheduler_context(&self, context: SchedulingContext) {
        *self.1.lock().unwrap() = (context, None);
    }
}
