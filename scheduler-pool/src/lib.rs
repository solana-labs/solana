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
        bank::Bank,
        installed_scheduler_pool::{
            DefaultScheduleExecutionArg, InstalledScheduler, InstalledSchedulerPool,
            InstalledSchedulerPoolArc, ResultWithTimings, ScheduleExecutionArg, SchedulerId,
            SchedulingContext, WaitReason, WithTransactionAndIndex,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_scheduler::{SchedulingMode, WithSchedulingMode},
    solana_sdk::transaction::{Result, SanitizedTransaction},
    std::{
        fmt::Debug,
        marker::PhantomData,
        sync::{Arc, Mutex, Weak},
    },
};

// SchedulerPool must be accessed via dyn by solana-runtime code, because of its internal fields'
// types aren't available there...
#[derive(Debug)]
pub struct SchedulerPool<
    T: SpawnableScheduler<TH, SEA>,
    TH: ScheduledTransactionHandler<SEA>,
    SEA: ScheduleExecutionArg,
> {
    schedulers: Mutex<Vec<Box<dyn InstalledScheduler<SEA>>>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    weak_self: Weak<Self>,
    _phantom: PhantomData<(T, SEA, TH)>,
}

pub type DefaultSchedulerPool = SchedulerPool<
    PooledScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>,
    DefaultTransactionHandler,
    DefaultScheduleExecutionArg,
>;

impl<
        T: SpawnableScheduler<TH, SEA>,
        TH: ScheduledTransactionHandler<SEA>,
        SEA: ScheduleExecutionArg,
    > SchedulerPool<T, TH, SEA>
{
    pub fn new(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            schedulers: Mutex::<Vec<Box<dyn InstalledScheduler<SEA>>>>::default(),
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
            weak_self: weak_self.clone(),
            _phantom: PhantomData,
        })
    }

    pub fn new_dyn(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> InstalledSchedulerPoolArc<SEA> {
        Self::new(
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
        )
    }

    pub fn self_arc(&self) -> Arc<Self> {
        self.weak_self
            .upgrade()
            .expect("self-referencing Arc-ed pool")
    }
}

impl<
        T: SpawnableScheduler<TH, SEA>,
        TH: ScheduledTransactionHandler<SEA>,
        SEA: ScheduleExecutionArg,
    > InstalledSchedulerPool<SEA> for SchedulerPool<T, TH, SEA>
{
    fn take_from_pool(&self, context: SchedulingContext) -> Box<dyn InstalledScheduler<SEA>> {
        // pop is intentional for filo, expecting relatively warmed-up scheduler due to having been
        // returned recently
        if let Some(mut scheduler) = self.schedulers.lock().expect("not poisoned").pop() {
            scheduler.replace_context(context);
            scheduler
        } else {
            T::spawn_boxed(self.self_arc(), context, TH::create(self))
        }
    }

    fn return_to_pool(&self, scheduler: Box<dyn InstalledScheduler<SEA>>) {
        assert!(scheduler.context().is_none());

        self.schedulers
            .lock()
            .expect("not poisoned")
            .push(scheduler);
    }
}

pub trait ScheduledTransactionHandler<SEA: ScheduleExecutionArg>:
    Send + Sync + Debug + Sized + 'static
{
    fn create<T: SpawnableScheduler<Self, SEA>>(pool: &SchedulerPool<T, Self, SEA>) -> Self;

    fn handle<T: SpawnableScheduler<Self, SEA>>(
        &self,
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        pool: &SchedulerPool<T, Self, SEA>,
    );
}

#[derive(Debug)]
pub struct DefaultTransactionHandler;

impl<SEA: ScheduleExecutionArg> ScheduledTransactionHandler<SEA> for DefaultTransactionHandler {
    fn create<T: SpawnableScheduler<Self, SEA>>(_pool: &SchedulerPool<T, Self, SEA>) -> Self {
        Self
    }

    fn handle<T: SpawnableScheduler<Self, SEA>>(
        &self,
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        pool: &SchedulerPool<T, Self, SEA>,
    ) {
        // scheduler must properly prevent conflicting tx executions, so locking isn't needed
        // here
        let batch = bank.prepare_sanitized_batch_without_locking(transaction);
        let batch_with_indexes = TransactionBatchWithIndexes {
            batch,
            transaction_indexes: vec![index],
        };

        *result = execute_batch(
            &batch_with_indexes,
            bank,
            pool.transaction_status_sender.as_ref(),
            pool.replay_vote_sender.as_ref(),
            timings,
            pool.log_messages_bytes_limit,
            &pool.prioritization_fee_cache,
        );
    }
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnet-beta
#[derive(Debug)]
pub struct PooledScheduler<TH: ScheduledTransactionHandler<SEA>, SEA: ScheduleExecutionArg> {
    id: SchedulerId,
    pool: Arc<SchedulerPool<Self, TH, SEA>>,
    context: Option<SchedulingContext>,
    result_with_timings: Mutex<Option<ResultWithTimings>>,
    handler: TH,
    _phantom: PhantomData<SEA>,
}

impl<TH: ScheduledTransactionHandler<SEA>, SEA: ScheduleExecutionArg> PooledScheduler<TH, SEA> {
    pub fn spawn(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Self {
        Self {
            id: thread_rng().gen::<SchedulerId>(),
            pool,
            context: Some(initial_context),
            result_with_timings: Mutex::default(),
            handler,
            _phantom: PhantomData::default(),
        }
    }
}

pub trait SpawnableScheduler<TH: ScheduledTransactionHandler<SEA>, SEA: ScheduleExecutionArg>:
    InstalledScheduler<SEA>
{
    fn spawn_boxed(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Box<dyn InstalledScheduler<SEA>>
    where
        Self: Sized;
}

impl<TH: ScheduledTransactionHandler<SEA>, SEA: ScheduleExecutionArg> SpawnableScheduler<TH, SEA>
    for PooledScheduler<TH, SEA>
{
    fn spawn_boxed(
        pool: Arc<SchedulerPool<Self, TH, SEA>>,
        initial_context: SchedulingContext,
        handler: TH,
    ) -> Box<dyn InstalledScheduler<SEA>> {
        Box::new(Self::spawn(pool, initial_context, handler))
    }
}

impl<TH: ScheduledTransactionHandler<SEA>, SEA: ScheduleExecutionArg> InstalledScheduler<SEA>
    for PooledScheduler<TH, SEA>
{
    fn id(&self) -> SchedulerId {
        self.id
    }

    fn pool(&self) -> InstalledSchedulerPoolArc<SEA> {
        self.pool.clone()
    }

    fn schedule_execution(&self, transaction_with_index: SEA::TransactionWithIndex<'_>) {
        let context = self.context.as_ref().expect("active context should exist");

        let fail_fast = match context.mode() {
            // this should be false, for (upcoming) BlockProduction variant.
            SchedulingMode::BlockVerification => true,
        };

        let result_with_timings = &mut *self.result_with_timings.lock().expect("not poisoned");
        let (result, timings) =
            result_with_timings.get_or_insert_with(|| (Ok(()), ExecuteTimings::default()));

        // so, we're NOT scheduling at all; rather, just execute tx straight off.  we doesn't need
        // to solve inter-tx locking deps only in the case of single-thread fifo like this....
        if result.is_ok() || !fail_fast {
            transaction_with_index.with_transaction_and_index(|transaction, index| {
                TH::handle(
                    &self.handler,
                    result,
                    timings,
                    context.bank(),
                    transaction,
                    index,
                    &self.pool,
                );
            })
        }
    }

    fn wait_for_termination(&mut self, wait_reason: &WaitReason) -> Option<ResultWithTimings> {
        let keep_result_with_timings = match wait_reason {
            WaitReason::ReinitializedForRecentBlockhash => true,
            WaitReason::TerminatedToFreeze | WaitReason::DroppedFromBankForks => false,
        };

        if keep_result_with_timings {
            None
        } else {
            drop::<Option<SchedulingContext>>(self.context.take());
            // current simplest form of this trait impl doesn't block the current thread materially
            // just with the following single mutex lock. Suppose more elaborated synchronization
            // across worker threads here in the future...
            self.result_with_timings
                .lock()
                .expect("not poisoned")
                .take()
        }
    }

    fn context(&self) -> Option<&SchedulingContext> {
        self.context.as_ref()
    }

    fn replace_context(&mut self, context: SchedulingContext) {
        self.context = Some(context);
        *self.result_with_timings.lock().expect("not poisoned") = None;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
            installed_scheduler_pool::{
                BankWithScheduler, DefaultInstalledSchedulerBox, SchedulingContext,
            },
            prioritization_fee_cache::PrioritizationFeeCache,
        },
        solana_sdk::{
            clock::MAX_PROCESSING_AGE,
            pubkey::Pubkey,
            signer::keypair::Keypair,
            system_transaction,
            transaction::{SanitizedTransaction, TransactionError},
        },
        std::{sync::Arc, thread::JoinHandle},
    };

    #[test]
    fn test_scheduler_pool_new() {
        solana_logger::setup();

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);

        // this indirectly proves that there should be circular link because there's only one Arc
        // at this moment now
        assert_eq!((Arc::strong_count(&pool), Arc::weak_count(&pool)), (1, 1));
        let debug = format!("{pool:#?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_spawn() {
        solana_logger::setup();

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank);
        let scheduler = pool.take_from_pool(context);

        let debug = format!("{scheduler:#?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_pool_filo() {
        solana_logger::setup();

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler1 = pool.take_from_pool(context.clone());
        let scheduler_id1 = scheduler1.id();
        let mut scheduler2 = pool.take_from_pool(context.clone());
        let scheduler_id2 = scheduler2.id();
        assert_ne!(scheduler_id1, scheduler_id2);

        assert_matches!(
            scheduler1.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        pool.return_to_pool(scheduler1);
        assert_matches!(
            scheduler2.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        pool.return_to_pool(scheduler2);

        let scheduler3 = pool.take_from_pool(context.clone());
        assert_eq!(scheduler_id2, scheduler3.id());
        let scheduler4 = pool.take_from_pool(context.clone());
        assert_eq!(scheduler_id1, scheduler4.id());
    }

    #[test]
    fn test_scheduler_pool_context_drop_unless_reinitialized() {
        solana_logger::setup();

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);

        let mut scheduler = pool.take_from_pool(context.clone());

        assert!(scheduler.context().is_some());
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::ReinitializedForRecentBlockhash),
            None
        );
        assert!(scheduler.context().is_some());
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        assert!(scheduler.context().is_none());
    }

    #[test]
    fn test_scheduler_pool_context_replace() {
        solana_logger::setup();

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let old_bank = &Arc::new(Bank::default_for_tests());
        let new_bank = &Arc::new(Bank::default_for_tests());
        assert!(!Arc::ptr_eq(old_bank, new_bank));

        let old_context =
            &SchedulingContext::new(SchedulingMode::BlockVerification, old_bank.clone());
        let new_context =
            &SchedulingContext::new(SchedulingMode::BlockVerification, new_bank.clone());

        let mut scheduler = pool.take_from_pool(old_context.clone());
        let scheduler_id = scheduler.id();
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
            None
        );
        pool.return_to_pool(scheduler);

        let scheduler = pool.take_from_pool(new_context.clone());
        assert_eq!(scheduler_id, scheduler.id());
        assert!(Arc::ptr_eq(scheduler.context().unwrap().bank(), new_bank));
    }

    #[test]
    fn test_scheduler_pool_install_into_bank_forks() {
        solana_logger::setup();

        let bank = Bank::default_for_tests();
        let mut bank_forks = BankForks::new(bank);
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        bank_forks.install_scheduler_pool(pool);
    }

    #[test]
    fn test_scheduler_install_into_bank() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let child_bank = Bank::new_from_parent(&bank, &Pubkey::default(), 1);

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);

        let bank = Bank::default_for_tests();
        let mut bank_forks = BankForks::new(bank);

        // existing banks in bank_forks shouldn't process transactions anymore in general, so
        // shouldn't be touched
        assert!(!bank_forks
            .working_bank_with_scheduler()
            .has_installed_scheduler());
        bank_forks.install_scheduler_pool(pool);
        assert!(!bank_forks
            .working_bank_with_scheduler()
            .has_installed_scheduler());

        let child_bank = bank_forks.insert(child_bank);
        assert!(child_bank.has_installed_scheduler());
        bank_forks.remove(child_bank.slot());
        child_bank.drop_scheduler();
        assert!(!child_bank.has_installed_scheduler());
    }

    #[test]
    fn test_scheduler_schedule_execution_success() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let tx0 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            2,
            genesis_config.hash(),
        ));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        assert_eq!(bank.transaction_count(), 0);
        let scheduler = pool.take_from_pool(context);
        scheduler.schedule_execution(&(tx0, 0));
        let bank = BankWithScheduler::new_for_test(bank, Some(scheduler));
        assert_matches!(bank.wait_for_completed_scheduler(), Some((Ok(()), _)));
        assert_eq!(bank.transaction_count(), 1);
    }

    #[test]
    fn test_scheduler_schedule_execution_failure() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let unfunded_keypair = Keypair::new();
        let tx0 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &unfunded_keypair,
            &solana_sdk::pubkey::new_rand(),
            2,
            genesis_config.hash(),
        ));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        assert_eq!(bank.transaction_count(), 0);
        let scheduler = pool.take_from_pool(context);
        scheduler.schedule_execution(&(tx0, 0));
        assert_eq!(bank.transaction_count(), 0);

        let tx1 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            3,
            genesis_config.hash(),
        ));
        assert_matches!(
            bank.simulate_transaction_unchecked(tx1.clone()).result,
            Ok(_)
        );
        scheduler.schedule_execution(&(tx1, 0));
        // transaction_count should remain same as scheduler should be bailing out.
        assert_eq!(bank.transaction_count(), 0);

        let bank = BankWithScheduler::new_for_test(bank, Some(scheduler));
        assert_matches!(
            bank.wait_for_completed_scheduler(),
            Some((
                Err(solana_sdk::transaction::TransactionError::AccountNotFound),
                _timings
            ))
        );
    }

    #[derive(Debug)]
    struct AsyncScheduler<const TRIGGER_RACE_CONDITION: bool>(
        PooledScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>,
        Mutex<Vec<JoinHandle<ResultWithTimings>>>,
    );

    impl<const TRIGGER_RACE_CONDITION: bool> InstalledScheduler<DefaultScheduleExecutionArg>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn id(&self) -> SchedulerId {
            self.0.id()
        }

        fn pool(&self) -> InstalledSchedulerPoolArc<DefaultScheduleExecutionArg> {
            self.0.pool()
        }

        fn context(&self) -> Option<&SchedulingContext> {
            self.0.context()
        }

        fn replace_context(&mut self, context: SchedulingContext) {
            self.0.replace_context(context)
        }

        fn schedule_execution<'a>(
            &'a self,
            &(transaction, index): <DefaultScheduleExecutionArg as ScheduleExecutionArg>::TransactionWithIndex<'a>,
        ) {
            let transaction_and_index = (transaction.clone(), index);
            let context = self.context().unwrap().clone();
            let pool = self.0.pool.clone();

            self.1.lock().unwrap().push(std::thread::spawn(move || {
                // intentionally sleep to simulate race condition where register_recent_blockhash
                // is run before finishing executing scheduled transactions
                std::thread::sleep(std::time::Duration::from_secs(1));

                let mut result = Ok(());
                let mut timings = ExecuteTimings::default();

                <DefaultTransactionHandler as ScheduledTransactionHandler<
                    DefaultScheduleExecutionArg,
                >>::handle(
                    &DefaultTransactionHandler,
                    &mut result,
                    &mut timings,
                    context.bank(),
                    &transaction_and_index.0,
                    transaction_and_index.1,
                    &pool,
                );
                (result, timings)
            }));
        }

        fn wait_for_termination(&mut self, reason: &WaitReason) -> Option<ResultWithTimings> {
            if TRIGGER_RACE_CONDITION
                && matches!(reason, WaitReason::ReinitializedForRecentBlockhash)
            {
                // this is equivalent to NOT calling wait_for_reusable_scheduler() in
                // register_recent_blockhash().
                return None;
            }

            let mut overall_result = Ok(());
            let mut overall_timings = ExecuteTimings::default();
            for handle in self.1.lock().unwrap().drain(..) {
                let (result, timings) = handle.join().unwrap();
                match result {
                    Ok(()) => {}
                    Err(e) => overall_result = Err(e),
                }
                overall_timings.accumulate(&timings);
            }
            *self.0.result_with_timings.lock().unwrap() = Some((overall_result, overall_timings));

            self.0.wait_for_termination(reason)
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool>
        SpawnableScheduler<DefaultTransactionHandler, DefaultScheduleExecutionArg>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn spawn_boxed(
            pool: Arc<SchedulerPool<Self, DefaultTransactionHandler, DefaultScheduleExecutionArg>>,
            initial_context: SchedulingContext,
            handler: DefaultTransactionHandler,
        ) -> DefaultInstalledSchedulerBox {
            Box::new(AsyncScheduler::<TRIGGER_RACE_CONDITION>(
                PooledScheduler::<DefaultTransactionHandler, DefaultScheduleExecutionArg> {
                    id: thread_rng().gen::<SchedulerId>(),
                    pool: SchedulerPool::new(
                        pool.log_messages_bytes_limit,
                        pool.transaction_status_sender.clone(),
                        pool.replay_vote_sender.clone(),
                        pool.prioritization_fee_cache.clone(),
                    ),
                    context: Some(initial_context),
                    result_with_timings: Mutex::default(),
                    handler,
                    _phantom: PhantomData::default(),
                },
                Mutex::new(vec![]),
            ))
        }
    }

    fn do_test_scheduler_schedule_execution_recent_blockhash_edge_case<
        const TRIGGER_RACE_CONDITION: bool,
    >() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let very_old_valid_tx =
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &mint_keypair,
                &solana_sdk::pubkey::new_rand(),
                2,
                genesis_config.hash(),
            ));
        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config));
        for _ in 0..MAX_PROCESSING_AGE {
            bank.fill_bank_with_ticks_for_tests();
            bank.freeze();
            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                bank.slot().checked_add(1).unwrap(),
            ));
        }
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::<
            AsyncScheduler<TRIGGER_RACE_CONDITION>,
            DefaultTransactionHandler,
            DefaultScheduleExecutionArg,
        >::new_dyn(None, None, None, _ignored_prioritization_fee_cache);
        let scheduler = pool.take_from_pool(context);

        let bank = BankWithScheduler::new_for_test(bank, Some(scheduler));
        assert_eq!(bank.transaction_count(), 0);

        // schedule but not immediately execute transaction
        bank.schedule_transaction_executions(&[very_old_valid_tx], [0].iter());
        // this calls register_recent_blockhash internally
        bank.fill_bank_with_ticks_for_tests();

        if TRIGGER_RACE_CONDITION {
            // very_old_valid_tx is wrongly handled as expired!
            assert_matches!(
                bank.wait_for_completed_scheduler(),
                Some((Err(TransactionError::BlockhashNotFound), _))
            );
            assert_eq!(bank.transaction_count(), 0);
        } else {
            assert_matches!(bank.wait_for_completed_scheduler(), Some((Ok(()), _)));
            assert_eq!(bank.transaction_count(), 1);
        }
    }

    #[test]
    fn test_scheduler_schedule_execution_recent_blockhash_edge_case_with_race() {
        do_test_scheduler_schedule_execution_recent_blockhash_edge_case::<true>();
    }

    #[test]
    fn test_scheduler_schedule_execution_recent_blockhash_edge_case_without_race() {
        do_test_scheduler_schedule_execution_recent_blockhash_edge_case::<false>();
    }
}
