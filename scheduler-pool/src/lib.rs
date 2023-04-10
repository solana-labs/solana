use solana_runtime::vote_sender_types::ReplayVoteSender;
use std::sync::Arc;
use solana_runtime::prioritization_fee_cache::PrioritizationFeeCache;
use solana_ledger::blockstore_processor::TransactionStatusSender;
use solana_runtime::installed_scheduler_pool::InstalledScheduler;
use solana_poh::poh_recorder::PohRecorder;
use std::sync::RwLock;
use solana_runtime::installed_scheduler_pool::InstalledSchedulerPool;

#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: std::sync::Mutex<Vec<Box<dyn InstalledScheduler>>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
}

struct SchedulerPool {
}

impl SchedulerPool {
    fn new(poh_recorder: Option<&Arc<RwLock<PohRecorder>>>, log_messages_bytes_limit: Option<usize>, transaction_status_sender: Option<TransactionStatusSender>, replay_vote_sender: Option<ReplayVoteSender>, prioritization_fee_cache: Arc<PrioritizationFeeCache>) -> Self {
        Self {
            schedulers: std::sync::Mutex::new(Vec::new()),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
        }
    }
    pub fn new_boxed(poh_recorder: Option<&Arc<RwLock<PohRecorder>>>, log_messages_bytes_limit: Option<usize>, transaction_status_sender: Option<TransactionStatusSender>, replay_vote_sender: Option<ReplayVoteSender>, prioritization_fee_cache: Arc<PrioritizationFeeCache>) -> Box<dyn InstalledSchedulerPool> {
        Box::new(SchedulerPoolWrapper::new(poh_recorder, log_messages_bytes_limit, transaction_status_sender, replay_vote_sender, prioritization_fee_cache))
    }

    fn prepare_new_scheduler(self: &Arc<Self>, context: SchedulingContext) {
        // block on some max count of borrowed schdulers!
        self.schedulers.lock().unwrap().push(Box::new(Scheduler::spawn(self.clone(), context)));
    }

    fn take_from_pool(self: &Arc<Self>, context: Option<SchedulingContext>) -> Box<dyn InstalledScheduler> {
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
            self.take_from_pool(None)
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

#[derive(Debug)]
struct SchedulerPoolWrapper(Arc<SchedulerPool>);

impl SchedulerPoolWrapper {
    fn new(poh_recorder: Option<&Arc<RwLock<PohRecorder>>>, log_messages_bytes_limit: Option<usize>, transaction_status_sender: Option<TransactionStatusSender>, replay_vote_sender: Option<ReplayVoteSender>, prioritization_fee_cache: Arc<PrioritizationFeeCache>) -> Self {
        Self(Arc::new(SchedulerPool::new(poh_recorder, log_messages_bytes_limit, transaction_status_sender, replay_vote_sender, prioritization_fee_cache)))
    }
}

impl InstalledSchedulerPool for SchedulerPoolWrapper {
    fn take_from_pool(&self, context: SchedulingContext) -> Box<dyn InstalledScheduler> {
        self.0.take_from_pool(Some(context))
    }

    fn return_to_pool(&self, scheduler: Box<dyn InstalledScheduler>) {
        self.0.return_to_pool(scheduler);
    }
}
