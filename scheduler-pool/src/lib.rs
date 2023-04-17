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
            InstalledScheduler, InstalledSchedulerPool, ResultWithTimings, SchedulerBox,
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
// not usable at all, especially for mainnet-beta
#[derive(Debug)]
struct Scheduler {
    id: SchedulerId,
    pool: Arc<SchedulerPool>,
    context: Option<SchedulingContext>,
    result_with_timings: Mutex<Option<ResultWithTimings>>,
}

impl Scheduler {
    fn spawn(pool: Arc<SchedulerPool>, initial_context: SchedulingContext) -> Self {
        Self {
            id: thread_rng().gen::<SchedulerId>(),
            pool,
            context: Some(initial_context),
            result_with_timings: Mutex::default(),
        }
    }
}
impl InstalledScheduler for Scheduler {
    fn scheduler_id(&self) -> SchedulerId {
        self.id
    }

    fn scheduler_pool(&self) -> SchedulerPoolArc {
        self.pool.clone()
    }

    fn schedule_execution(&self, transaction: &SanitizedTransaction, index: usize) {
        let context = self.context.as_ref().expect("active context");

        let batch = context
            .bank()
            .prepare_sanitized_batch_without_locking(transaction.clone());
        let batch_with_indexes = TransactionBatchWithIndexes {
            batch,
            transaction_indexes: vec![index],
        };

        let fail_fast = match context.mode() {
            // this should be false, for (upcoming) BlockGeneration variant .
            SchedulingMode::BlockVerification => true,
        };

        let result_with_timings = &mut *self.result_with_timings.lock().expect("not poisoned");
        let (result, timings) =
            result_with_timings.get_or_insert_with(|| (Ok(()), ExecuteTimings::default()));

        // so, we're NOT scheduling at all; rather, just execute tx straight off.  we doesn't need
        // to solve inter-tx locking deps only in the case of single-thread fifo like this....
        if result.is_ok() || !fail_fast {
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
        drop::<Option<SchedulingContext>>(self.context.take());
    }

    fn wait_for_termination(&mut self, wait_source: &WaitSource) -> Option<ResultWithTimings> {
        let keep_result_with_timings = match wait_source {
            WaitSource::InsideBlock => {
                // rustfmt...
                true
            }
            WaitSource::AcrossBlock | WaitSource::FromBankDrop | WaitSource::FromSchedulerDrop => {
                false
            }
        };

        self.schedule_termination();

        // current simplest form of this trait impl doesn't block the current thread materially
        // just with the following single mutex lock. Suppose more elaborated synchronization
        // across worker threads here in the future...

        if keep_result_with_timings {
            None
        } else {
            self.result_with_timings
                .lock()
                .expect("not poisoned")
                .take()
        }
    }

    fn scheduling_context(&self) -> Option<&SchedulingContext> {
        self.context.as_ref()
    }

    fn replace_scheduler_context(&mut self, context: SchedulingContext) {
        self.context = Some(context);
        *self.result_with_timings.lock().expect("not poisoned") = None;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::SchedulerPool,
        assert_matches::assert_matches,
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            installed_scheduler_pool::SchedulingContext,
            prioritization_fee_cache::PrioritizationFeeCache,
        },
        solana_sdk::{pubkey::Pubkey, system_transaction},
        std::sync::Arc,
    };

    #[test]
    fn test_scheduler_pool_new() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);

        // this indirectly proves that there should be circular link because there's only one Arc
        // at this moment now
        assert_eq!((Arc::strong_count(&pool), Arc::weak_count(&pool)), (1, 1));
        let debug = format!("{pool:#?}");    
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_spawn() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);
        let scheduler = Scheduler::spawn(pool, context);

        let debug = format!("{scheduler:#?}");    
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_pool_filo() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler1 = pool.take_from_pool(context.clone());
        let scheduler_id1 = scheduler1.scheduler_id();
        let mut scheduler2 = pool.take_from_pool(context.clone());
        let scheduler_id2 = scheduler2.scheduler_id();
        assert_ne!(scheduler_id1, scheduler_id2);

        assert_matches!(
            scheduler1.wait_for_termination(&WaitSource::AcrossBlock),
            None
        );
        pool.return_to_pool(scheduler1);
        assert_matches!(
            scheduler2.wait_for_termination(&WaitSource::AcrossBlock),
            None
        );
        pool.return_to_pool(scheduler2);

        let scheduler3 = pool.take_from_pool(context.clone());
        assert_eq!(scheduler_id2, scheduler3.scheduler_id());
        let scheduler4 = pool.take_from_pool(context.clone());
        assert_eq!(scheduler_id1, scheduler4.scheduler_id());
    }

    #[test]
    fn test_scheduler_pool_implicit_schedule_termination() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler = pool.take_from_pool(context.clone());

        assert!(scheduler.scheduling_context().is_some());
        assert_matches!(
            scheduler.wait_for_termination(&WaitSource::InsideBlock),
            None
        );
        assert!(scheduler.scheduling_context().is_none());
    }

    #[test]
    fn test_scheduler_pool_context_replace() {
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let old_bank = &Arc::new(Bank::default_for_tests());
        let new_bank = &Arc::new(Bank::default_for_tests());
        assert!(!Arc::ptr_eq(old_bank, new_bank));

        let old_context =
            &SchedulingContext::new(SchedulingMode::BlockVerification, old_bank.clone());
        let new_context =
            &SchedulingContext::new(SchedulingMode::BlockVerification, new_bank.clone());

        let mut scheduler = pool.take_from_pool(old_context.clone());
        let scheduler_id = scheduler.scheduler_id();
        assert_matches!(
            scheduler.wait_for_termination(&WaitSource::AcrossBlock),
            None
        );
        pool.return_to_pool(scheduler);

        let scheduler = pool.take_from_pool(new_context.clone());
        assert_eq!(scheduler_id, scheduler.scheduler_id());
        assert!(Arc::ptr_eq(
            scheduler.scheduling_context().unwrap().bank(),
            new_bank
        ));
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

    #[test]
    fn test_schedule_execution() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let key = solana_sdk::pubkey::new_rand();
        let tx0 = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &key,
            2,
            genesis_config.hash(),
        ));
        let bank = &Arc::new(Bank::new_for_tests(&genesis_config));
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        assert_eq!(bank.transaction_count(), 0);
        let scheduler = pool.take_from_pool(context.clone());
        scheduler.schedule_execution(&tx0, 0);
        assert_eq!(bank.transaction_count(), 1);
    }
}
