//! Transaction scheduling code.
//!
//! This crate implements two solana-runtime traits (`InstalledScheduler` and
//! `InstalledSchedulerPool`) to provide concrete transaction scheduling implementation (including
//! executing txes and committing tx results).
//!
//! At highest level, this crate inputs SanitizedTransaction via its `schedule_execution()` and
//! commits any side-effects (i.e. on-chain state changes) into `Bank`s via `solana-ledger`'s
//! helper fun called `execute_batch()`.

use {
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

// SchedulerPool must be accessed via dyn because of its internal fields, whose type isn't
// available at solana-runtime...
#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: Mutex<Vec<SchedulerBox>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    weak: Weak<SchedulerPool>,
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
        let mut schedulers = self.schedulers.lock().expect("not poisoned");
        let maybe_scheduler = schedulers.pop();
        if let Some(scheduler) = maybe_scheduler {
            scheduler.replace_scheduler_context(context);
            scheduler
        } else {
            Box::new(Scheduler::spawn(
                self.weak.upgrade().expect("self-referencing Arc-ed pool"),
                context,
            ))
        }
    }

    fn return_to_pool(&self, scheduler: SchedulerBox) {
        self.schedulers
            .lock()
            .expect("not poisoned")
            .push(scheduler);
    }
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnnet-beta
// for that reason, tuple structs are used internally.
#[derive(Debug)]
struct Scheduler(
    Arc<SchedulerPool>,
    Mutex<(Option<SchedulingContext>, Option<TimingAndResult>)>,
);

impl Scheduler {
    fn spawn(scheduler_pool: Arc<SchedulerPool>, initial_context: SchedulingContext) -> Self {
        Self(scheduler_pool, Mutex::new((Some(initial_context), None)))
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
        let (pool, (ref context, ref mut timings_and_result)) =
            (&self.0, &mut *self.1.lock().expect("not poisoned"));
        let context = context.as_ref().expect("active context");

        let batch_with_indexes = TransactionBatchWithIndexes {
            batch: context.bank().prepare_sanitized_batch_without_locking(transaction.clone()),
            transaction_indexes: vec![index],
        };
        let (timings, result) =
            timings_and_result.get_or_insert_with(|| (ExecuteTimings::default(), Ok(())));

        if result.is_ok() {
            *result = execute_batch(
                &batch_with_indexes,
                context.bank(),
                pool.transaction_status_sender.as_ref(),
                pool.replay_vote_sender.as_ref(),
                timings,
                pool.log_messages_bytes_limit,
                &pool.prioritization_fee_cache,
            );
        }
    }

    fn schedule_termination(&mut self) {
        drop::<Option<SchedulingContext>>(self.1.lock().expect("not poisoned").0.take());
    }

    fn wait_for_termination(&mut self, wait_source: &WaitSource) -> Option<TimingAndResult> {
        match wait_source {
            WaitSource::InsideBlock => None,
            _ => self.1.lock().expect("not poisoned").1.take(),
        }
    }

    fn replace_scheduler_context(&self, context: SchedulingContext) {
        *self.1.lock().expect("not poisoned") = (Some(context), None);
    }
}
