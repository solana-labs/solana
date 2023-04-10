use {
    log::*,
    solana_ledger::blockstore_processor::TransactionStatusSender,
    solana_poh::poh_recorder::PohRecorder,
    solana_runtime::{
        installed_scheduler_pool::{InstalledScheduler, InstalledSchedulerPool, SchedulingContext},
        prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::transaction::{SanitizedTransaction, TransactionError},
    std::sync::{Arc, RwLock, Weak},
};
use solana_runtime::installed_scheduler_pool::SchedulerPoolArc;

#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: std::sync::Mutex<Vec<Box<dyn InstalledScheduler>>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    weak: std::sync::Weak<SchedulerPool>,
}

#[derive(Debug)]
struct Scheduler {}

impl Scheduler {
    fn spawn(scheduler_pool: Arc<SchedulerPool>, initial_context: SchedulingContext) -> Self {
        panic!();
    }
}

impl SchedulerPool {
    fn new(
        weak: &Weak<Self>,
        poh_recorder: Option<&Arc<RwLock<PohRecorder>>>,
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Self {
        Self {
            schedulers: std::sync::Mutex::new(Vec::new()),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak: weak.clone(),
        }
    }
    pub fn new_boxed(
        poh_recorder: Option<&Arc<RwLock<PohRecorder>>>,
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<dyn InstalledSchedulerPool> {
        Arc::new_cyclic(|weak_pool| SchedulerPool::new(
            weak_pool,
            poh_recorder,
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
        ))
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
        todo!()
    }
    fn scheduler_pool(
        &self,
    ) -> SchedulerPoolArc {
        todo!()
    }
    fn schedule_execution(&self, _: &SanitizedTransaction, _: usize) {
        execute_batch();
    }
    fn schedule_termination(&mut self) {
        todo!()
    }
    fn wait_for_termination(
        &mut self,
        _: &solana_runtime::installed_scheduler_pool::WaitSource,
    ) -> std::option::Option<(
        solana_program_runtime::timings::ExecuteTimings,
        std::result::Result<(), TransactionError>,
    )> {
        todo!()
    }
    fn replace_scheduler_context(
        &self,
        _: solana_runtime::installed_scheduler_pool::SchedulingContext,
    ) {
        todo!()
    }
}
