use {
    log::*,
    solana_ledger::blockstore_processor::TransactionStatusSender,
    solana_poh::poh_recorder::PohRecorder,
    solana_runtime::{
        installed_scheduler_pool::{InstalledScheduler, InstalledSchedulerPool, SchedulingContext},
        prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::transaction::{SanitizedTransaction, TransactionError, Result},
    std::sync::{Arc, RwLock, Weak, Mutex},
};
use solana_runtime::installed_scheduler_pool::SchedulerPoolArc;
use solana_runtime::installed_scheduler_pool::SchedulerBox;
use solana_program_runtime::timings::ExecuteTimings;

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
struct Scheduler(Arc<SchedulerPool>, Mutex<(SchedulingContext, ExecuteTimings, Result<()>)>);

impl Scheduler {
    fn spawn(scheduler_pool: Arc<SchedulerPool>, initial_context: SchedulingContext) -> Self {
        Self(scheduler_pool, Mutex::new((initial_context, None)))
    }
}

impl SchedulerPool {
    pub fn new_boxed(
        poh_recorder: Option<&Arc<RwLock<PohRecorder>>>,
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<dyn InstalledSchedulerPool> {
        Arc::new_cyclic(|weak_pool| Self {
            schedulers: Mutex::new(Vec::new()),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak: weak_pool.clone(),
        })
    }

    fn prepare_new_scheduler(&self, context: SchedulingContext) {
        // block on some max count of borrowed schdulers!
        self.schedulers
            .lock()
            .unwrap()
            .push(Box::new(Scheduler::spawn(self.weak.upgrade().unwrap(), context)));
    }

    fn take_from_pool2(
        &self,
        context: Option<SchedulingContext>,
    ) -> Box<dyn InstalledScheduler> {
        let mut schedulers = self.schedulers.lock().unwrap();
        let maybe_scheduler = schedulers.pop();
        if let Some(scheduler) = maybe_scheduler {
            trace!(
                "SchedulerPool: id_{:016x} is taken... len: {} => {}",
                scheduler.scheduler_id(),
                schedulers.len() + 1,
                schedulers.len()
            );
            drop(schedulers);

            if let Some(context) = context {
                scheduler.replace_scheduler_context(context);
            }
            scheduler
        } else {
            drop(schedulers);

            self.prepare_new_scheduler(context.unwrap());
            self.take_from_pool2(None)
        }
    }

    fn return_to_pool(self: &Arc<Self>, mut scheduler: Box<dyn InstalledScheduler>) {
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

impl InstalledSchedulerPool for SchedulerPool {
    fn take_from_pool(&self, context: SchedulingContext) -> Box<dyn InstalledScheduler> {
        self.take_from_pool2(Some(context))
    }

    fn return_to_pool(&self, scheduler: Box<dyn InstalledScheduler>) {
        self.return_to_pool(scheduler);
    }
}

impl InstalledScheduler for Scheduler {
    fn scheduler_id(&self) -> u64 {
        0
    }
    fn scheduler_pool(
        &self,
    ) -> SchedulerPoolArc {
        self.0.clone()
    }
    fn schedule_execution(&self, _: &SanitizedTransaction, _: usize) {
        use solana_ledger::blockstore_processor::execute_batch;
        use solana_ledger::blockstore_processor::TransactionBatchWithIndexes;
        let bank = self.1.lock().unwrap().0.bank();

        let b: TransactionBatchWithIndexes = {
            panic!();
        };
        execute_batch(&b, bank, self.0.transaction_status_sender.as_ref(), self.0.replay_vote_sender.as_ref(), &mut Default::default(), self.0.log_messages_bytes_limit, &self.0.prioritization_fee_cache);
    }
    fn schedule_termination(&mut self) {
        // no-op
    }
    fn wait_for_termination(
        &mut self,
        _: &solana_runtime::installed_scheduler_pool::WaitSource,
    ) -> std::option::Option<(
        solana_program_runtime::timings::ExecuteTimings,
        std::result::Result<(), TransactionError>,
    )> {
        // no-op
        None
    }
    fn replace_scheduler_context(
        &self,
        c: SchedulingContext,
    ) {
        *self.1.lock().unwrap() = (c, None);
    }
}
