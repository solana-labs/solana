//! Transaction scheduling code.
//!
//! This crate implements two solana-runtime traits (`InstalledScheduler` and
//! `InstalledSchedulerPool`) to provide concrete transaction scheduling implementation (including
//! executing txes and committing tx results).
//!
//! At highest level, this crate takes `SanitizedTransaction`s via its `schedule_execution()` and
//! commits any side-effects (i.e. on-chain state changes) into `Bank`s via `solana-ledger`'s
//! helper fun called `execute_batch()`.

use {
    rand::{thread_rng, Rng},
    solana_ledger::blockstore_processor::{
        execute_batch, TransactionBatchWithIndexes, TransactionStatusSender,
    },
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        installed_scheduler_pool::{
            InstalledScheduler, InstalledSchedulerPool, ResultWithTiming, SchedulerBox,
            SchedulerId, SchedulerPoolArc, SchedulingContext, WaitSource,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_scheduler::{SchedulingMode, WithSchedulingMode},
    solana_sdk::transaction::SanitizedTransaction,
    std::sync::{Arc, Mutex, Weak},
};

// SchedulerPool must be accessed via dyn by solana-runtime code, because of its internal fields'
// types aren't available there...
#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: Mutex<Vec<SchedulerBox>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    weak_self: Weak<SchedulerPool>,
}

impl SchedulerPool {
    pub fn new_dyn(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> SchedulerPoolArc {
        Arc::new_cyclic(|weak_self| Self {
            schedulers: Mutex::<Vec<SchedulerBox>>::default(),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak_self: weak_self.clone(),
        })
    }

    pub fn self_arc(&self) -> Arc<Self> {
        self.weak_self
            .upgrade()
            .expect("self-referencing Arc-ed pool")
    }
}

impl InstalledSchedulerPool for SchedulerPool {
    fn take_from_pool(&self, context: SchedulingContext) -> SchedulerBox {
        assert!(!context.bank().with_scheduler());

        let mut schedulers = self.schedulers.lock().expect("not poisoned");
        // pop is intentional for filo, expecting relatively warmed-up scheduler due to having been
        // returned recently
        let maybe_scheduler = schedulers.pop();
        if let Some(mut scheduler) = maybe_scheduler {
            scheduler.replace_scheduler_context(context);
            scheduler
        } else {
            Box::new(Scheduler::spawn(self.self_arc(), context))
        }
    }

    fn return_to_pool(&self, scheduler: SchedulerBox) {
        assert!(scheduler.scheduling_context().is_none());

        self.schedulers
            .lock()
            .expect("not poisoned")
            .push(scheduler);
    }
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnnet-beta
#[derive(Debug)]
struct Scheduler {
    id: SchedulerId,
    pool: Arc<SchedulerPool>,
    context_and_result_with_timing: Mutex<(Option<SchedulingContext>, Option<ResultWithTiming>)>,
}

impl Scheduler {
    fn spawn(pool: Arc<SchedulerPool>, initial_context: SchedulingContext) -> Self {
        Self {
            id: thread_rng().gen::<SchedulerId>(),
            pool,
            context_and_result_with_timing: Mutex::new((Some(initial_context), None)),
        }
    }
}

#[automock]
impl InstalledScheduler for Scheduler {
    fn scheduler_id(&self) -> SchedulerId {
        self.id
    }

    fn scheduler_pool(&self) -> SchedulerPoolArc {
        self.pool.clone()
    }

    fn schedule_execution(&self, transaction: &SanitizedTransaction, index: usize) {
        let (ref context, ref mut result_with_timing) = &mut *self
            .context_and_result_with_timing
            .lock()
            .expect("not poisoned");
        let context = context.as_ref().expect("active context");

        let batch = context
            .bank()
            .prepare_sanitized_batch_without_locking(transaction.clone());
        let batch_with_indexes = TransactionBatchWithIndexes {
            batch,
            transaction_indexes: vec![index],
        };
        let (result, timings) =
            result_with_timing.get_or_insert_with(|| (Ok(()), ExecuteTimings::default()));

        let fail_fast = match context.mode() {
            // this should be false, for (upcoming) BlockGeneration variant .
            SchedulingMode::BlockVerification => true,
        };

        // so, we're NOT scheduling at all; rather, just execute tx straight off.  we doesn't need
        // to solve inter-tx locking deps only in the case of single-thread fifo like this....
        if !fail_fast {
            *result = execute_batch(
                &batch_with_indexes,
                context.bank(),
                self.pool.transaction_status_sender.as_ref(),
                self.pool.replay_vote_sender.as_ref(),
                timings,
                self.pool.log_messages_bytes_limit,
                &self.pool.prioritization_fee_cache,
            );
        }
    }

    fn schedule_termination(&mut self) {
        drop::<Option<SchedulingContext>>(
            self.context_and_result_with_timing
                .lock()
                .expect("not poisoned")
                .0
                .take(),
        );
    }

    fn wait_for_termination(&mut self, wait_source: &WaitSource) -> Option<ResultWithTiming> {
        let should_block_current_thread = match wait_source {
            WaitSource::InsideBlock => {
                // rustfmt...
                false
            }
            WaitSource::AcrossBlock | WaitSource::FromBankDrop | WaitSource::FromSchedulerDrop => {
                true
            }
        };

        if should_block_current_thread {
            self.schedule_termination();
            // current simplest form of this trait impl doesn't block the current thread
            // materially with the following single mutex lock....
            self.context_and_result_with_timing
                .lock()
                .expect("not poisoned")
                .1
                .take()
        } else {
            None
        }
    }

    fn scheduling_context(&self) -> Option<SchedulingContext> {
        self.context_and_result_with_timing
            .lock()
            .expect("not poisoned")
            .0
            .clone()
    }

    fn replace_scheduler_context(&mut self, context: SchedulingContext) {
        *self
            .context_and_result_with_timing
            .lock()
            .expect("not poisoned") = (Some(context), None);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::SchedulerPool,
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            installed_scheduler_pool::SchedulingContext,
            prioritization_fee_cache::PrioritizationFeeCache,
        },
        solana_sdk::pubkey::Pubkey,
        std::sync::Arc,
    };

    #[test]
    fn test_scheduler_pool_new() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);

        // this indirectly proves that there should be circular link because there's only one Arc
        // at this moment now
        assert_eq!((Arc::strong_count(&pool), Arc::weak_count(&pool)), (1, 1));
    }

    #[test]
    fn test_scheduler_pool_filo() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler1 = pool.take_from_pool(context.clone());
        let scheduler_id1 = scheduler1.scheduler_id();
        let mut scheduler2 = pool.take_from_pool(context.clone());
        let scheduler_id2 = scheduler2.scheduler_id();
        assert_ne!(scheduler_id1, scheduler_id2);

        scheduler1.wait_for_termination(&WaitSource::AcrossBlock);
        pool.return_to_pool(scheduler1);
        scheduler2.wait_for_termination(&WaitSource::AcrossBlock);
        pool.return_to_pool(scheduler2);

        let scheduler3 = pool.take_from_pool(context.clone());
        assert_eq!(scheduler_id2, scheduler3.scheduler_id());
        let scheduler4 = pool.take_from_pool(context.clone());
        assert_eq!(scheduler_id1, scheduler4.scheduler_id());

        // explicit drop just for consistent .clone()-ing above
        drop(context);
    }

    #[test]
    fn test_scheduler_pool_context_replace() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let old_bank = Arc::new(Bank::default_for_tests());
        let new_bank = Arc::new(Bank::default_for_tests());
        assert!(!Arc::ptr_eq(&old_bank, &new_bank));

        let old_context =
            SchedulingContext::new(SchedulingMode::BlockVerification, old_bank.clone());
        let new_context =
            SchedulingContext::new(SchedulingMode::BlockVerification, new_bank.clone());

        let mut scheduler = pool.take_from_pool(old_context.clone());
        let scheduler_id = scheduler.scheduler_id();
        scheduler.wait_for_termination(&WaitSource::AcrossBlock);
        pool.return_to_pool(scheduler);

        let scheduler = pool.take_from_pool(new_context.clone());
        assert_eq!(scheduler_id, scheduler.scheduler_id());
        assert!(Arc::ptr_eq(
            scheduler.scheduling_context().unwrap().bank(),
            &new_bank
        ));

        // explicit drop just for consistent .clone()-ing above
        drop((old_context, old_bank, new_context, new_bank));
    }

    #[test]
    fn test_scheduler_pool_install_into_bank_forks() {
        let bank = Bank::default_for_tests();
        let mut bank_forks = BankForks::new(bank);
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        bank_forks.install_scheduler_pool(pool);
    }

    #[test]
    fn test_scheduler_install_into_bank() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let child_bank = Bank::new_from_parent(&bank, &Pubkey::default(), 1);

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);

        let bank = Bank::default_for_tests();
        let mut bank_forks = BankForks::new(bank);
        bank_forks.install_scheduler_pool(pool);

        assert!(!child_bank.with_scheduler());
        bank_forks.insert(child_bank);
        let child_bank = bank_forks.working_bank();
        assert!(child_bank.with_scheduler());
    }
}
