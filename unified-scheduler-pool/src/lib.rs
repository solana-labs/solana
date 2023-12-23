//! Transaction scheduling code.
//!
//! This crate implements 3 solana-runtime traits (`InstalledScheduler`, `UninstalledScheduler` and
//! `InstalledSchedulerPool`) to provide a concrete transaction scheduling implementation
//! (including executing txes and committing tx results).
//!
//! At the highest level, this crate takes `SanitizedTransaction`s via its `schedule_execution()`
//! and commits any side-effects (i.e. on-chain state changes) into the associated `Bank` via
//! `solana-ledger`'s helper function called `execute_batch()`.

use {
    solana_ledger::blockstore_processor::{
        execute_batch, TransactionBatchWithIndexes, TransactionStatusSender,
    },
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        bank::Bank,
        installed_scheduler_pool::{
            InstalledScheduler, InstalledSchedulerBox, InstalledSchedulerPool,
            InstalledSchedulerPoolArc, ResultWithTimings, SchedulerId, SchedulingContext,
            UninstalledScheduler, UninstalledSchedulerBox,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::transaction::{Result, SanitizedTransaction},
    solana_vote::vote_sender_types::ReplayVoteSender,
    std::{
        fmt::Debug,
        marker::PhantomData,
        sync::{
            atomic::{AtomicU64, Ordering::Relaxed},
            Arc, Mutex, Weak,
        },
    },
};

type AtomicSchedulerId = AtomicU64;

// SchedulerPool must be accessed as a dyn trait from solana-runtime, because SchedulerPool
// contains some internal fields, whose types aren't available in solana-runtime (currently
// TransactionStatusSender; also, PohRecorder in the future)...
#[derive(Debug)]
pub struct SchedulerPool<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    scheduler_inners: Mutex<Vec<S::Inner>>,
    handler_context: HandlerContext,
    // weak_self could be elided by changing InstalledScheduler::take_scheduler()'s receiver to
    // Arc<Self> from &Self, because SchedulerPool is used as in the form of Arc<SchedulerPool>
    // almost always. But, this would cause wasted and noisy Arc::clone()'s at every call sites.
    //
    // Alternatively, `impl InstalledScheduler for Arc<SchedulerPool>` approach could be explored
    // but it entails its own problems due to rustc's coherence and necessitated newtype with the
    // type graph of InstalledScheduler being quite elaborate.
    //
    // After these considerations, this weak_self approach is chosen at the cost of some additional
    // memory increase.
    weak_self: Weak<Self>,
    next_scheduler_id: AtomicSchedulerId,
    _phantom: PhantomData<TH>,
}

#[derive(Debug)]
pub struct HandlerContext {
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
}

pub type DefaultSchedulerPool =
    SchedulerPool<PooledScheduler<DefaultTaskHandler>, DefaultTaskHandler>;

impl<S, TH> SchedulerPool<S, TH>
where
    S: SpawnableScheduler<TH>,
    TH: TaskHandler,
{
    // Some internal impl and test code want an actual concrete type, NOT the
    // `dyn InstalledSchedulerPool`. So don't merge this into `Self::new_dyn()`.
    fn new(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            scheduler_inners: Mutex::default(),
            handler_context: HandlerContext {
                log_messages_bytes_limit,
                transaction_status_sender,
                replay_vote_sender,
                prioritization_fee_cache,
            },
            weak_self: weak_self.clone(),
            next_scheduler_id: AtomicSchedulerId::default(),
            _phantom: PhantomData,
        })
    }

    // This apparently-meaningless wrapper is handy, because some callers explicitly want
    // `dyn InstalledSchedulerPool` to be returned for type inference convenience.
    pub fn new_dyn(
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> InstalledSchedulerPoolArc {
        Self::new(
            log_messages_bytes_limit,
            transaction_status_sender,
            replay_vote_sender,
            prioritization_fee_cache,
        )
    }

    // See a comment at the weak_self field for justification of this method's existence.
    fn self_arc(&self) -> Arc<Self> {
        self.weak_self
            .upgrade()
            .expect("self-referencing Arc-ed pool")
    }

    fn new_scheduler_id(&self) -> SchedulerId {
        self.next_scheduler_id.fetch_add(1, Relaxed)
    }

    fn return_scheduler(&self, scheduler: S::Inner) {
        self.scheduler_inners
            .lock()
            .expect("not poisoned")
            .push(scheduler);
    }

    fn do_take_scheduler(&self, context: SchedulingContext) -> S {
        // pop is intentional for filo, expecting relatively warmed-up scheduler due to having been
        // returned recently
        if let Some(inner) = self.scheduler_inners.lock().expect("not poisoned").pop() {
            S::from_inner(inner, context)
        } else {
            S::spawn(self.self_arc(), context)
        }
    }
}

impl<S, TH> InstalledSchedulerPool for SchedulerPool<S, TH>
where
    S: SpawnableScheduler<TH>,
    TH: TaskHandler,
{
    fn take_scheduler(&self, context: SchedulingContext) -> InstalledSchedulerBox {
        Box::new(self.do_take_scheduler(context))
    }
}

pub trait TaskHandler: Send + Sync + Debug + Sized + 'static {
    fn handle(
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        handler_context: &HandlerContext,
    );
}

#[derive(Debug)]
pub struct DefaultTaskHandler;

impl TaskHandler for DefaultTaskHandler {
    fn handle(
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        handler_context: &HandlerContext,
    ) {
        // scheduler must properly prevent conflicting tx executions. thus, task handler isn't
        // responsible for locking.
        let batch = bank.prepare_unlocked_batch_from_single_tx(transaction);
        let batch_with_indexes = TransactionBatchWithIndexes {
            batch,
            transaction_indexes: vec![index],
        };

        *result = execute_batch(
            &batch_with_indexes,
            bank,
            handler_context.transaction_status_sender.as_ref(),
            handler_context.replay_vote_sender.as_ref(),
            timings,
            handler_context.log_messages_bytes_limit,
            &handler_context.prioritization_fee_cache,
        );
    }
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnet-beta
#[derive(Debug)]
pub struct PooledScheduler<TH: TaskHandler> {
    inner: PooledSchedulerInner<Self, TH>,
    context: SchedulingContext,
    result_with_timings: Mutex<ResultWithTimings>,
}

#[derive(Debug)]
pub struct PooledSchedulerInner<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    id: SchedulerId,
    pool: Arc<SchedulerPool<S, TH>>,
}

impl<TH: TaskHandler> PooledScheduler<TH> {
    fn do_spawn(pool: Arc<SchedulerPool<Self, TH>>, initial_context: SchedulingContext) -> Self {
        Self::from_inner(
            PooledSchedulerInner::<Self, TH> {
                id: pool.new_scheduler_id(),
                pool,
            },
            initial_context,
        )
    }
}

pub trait SpawnableScheduler<TH: TaskHandler>: InstalledScheduler {
    type Inner: Debug + Send + Sync;

    fn into_inner(self) -> (ResultWithTimings, Self::Inner);

    fn from_inner(inner: Self::Inner, context: SchedulingContext) -> Self;

    fn spawn(pool: Arc<SchedulerPool<Self, TH>>, initial_context: SchedulingContext) -> Self
    where
        Self: Sized;
}

impl<TH: TaskHandler> SpawnableScheduler<TH> for PooledScheduler<TH> {
    type Inner = PooledSchedulerInner<Self, TH>;

    fn into_inner(self) -> (ResultWithTimings, Self::Inner) {
        (
            self.result_with_timings.into_inner().expect("not poisoned"),
            self.inner,
        )
    }

    fn from_inner(inner: Self::Inner, context: SchedulingContext) -> Self {
        Self {
            inner,
            context,
            result_with_timings: Mutex::new((Ok(()), ExecuteTimings::default())),
        }
    }

    fn spawn(pool: Arc<SchedulerPool<Self, TH>>, initial_context: SchedulingContext) -> Self {
        Self::do_spawn(pool, initial_context)
    }
}

impl<TH: TaskHandler> InstalledScheduler for PooledScheduler<TH> {
    fn id(&self) -> SchedulerId {
        self.inner.id
    }

    fn context(&self) -> &SchedulingContext {
        &self.context
    }

    fn schedule_execution(&self, &(transaction, index): &(&SanitizedTransaction, usize)) {
        let (result, timings) = &mut *self.result_with_timings.lock().expect("not poisoned");
        if result.is_err() {
            // just bail out early to short-circuit the processing altogether
            return;
        }

        // ... so, we're NOT scheduling at all here; rather, just execute tx straight off. the
        // inter-tx locking deps aren't needed to be resolved in the case of single-threaded FIFO
        // like this.
        TH::handle(
            result,
            timings,
            self.context().bank(),
            transaction,
            index,
            &self.inner.pool.handler_context,
        );
    }

    fn wait_for_termination(
        self: Box<Self>,
        _is_dropped: bool,
    ) -> (ResultWithTimings, UninstalledSchedulerBox) {
        let (result_with_timings, uninstalled_scheduler) = self.into_inner();
        (result_with_timings, Box::new(uninstalled_scheduler))
    }

    fn pause_for_recent_blockhash(&mut self) {
        // not surprisingly, there's nothing to do for this min impl!
    }
}

impl<S, TH> UninstalledScheduler for PooledSchedulerInner<S, TH>
where
    S: SpawnableScheduler<TH, Inner = PooledSchedulerInner<S, TH>>,
    TH: TaskHandler,
{
    fn return_to_pool(self: Box<Self>) {
        self.pool.clone().return_scheduler(*self)
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
            installed_scheduler_pool::{BankWithScheduler, SchedulingContext},
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

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);

        // this indirectly proves that there should be circular link because there's only one Arc
        // at this moment now
        assert_eq!((Arc::strong_count(&pool), Arc::weak_count(&pool)), (1, 1));
        let debug = format!("{pool:#?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_spawn() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = SchedulingContext::new(bank);
        let scheduler = pool.take_scheduler(context);

        let debug = format!("{scheduler:#?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_scheduler_pool_filo() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = DefaultSchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(bank);

        let scheduler1 = pool.do_take_scheduler(context.clone());
        let scheduler_id1 = scheduler1.id();
        let scheduler2 = pool.do_take_scheduler(context.clone());
        let scheduler_id2 = scheduler2.id();
        assert_ne!(scheduler_id1, scheduler_id2);

        let (result_with_timings, scheduler1) = scheduler1.into_inner();
        assert_matches!(result_with_timings, (Ok(()), _));
        pool.return_scheduler(scheduler1);
        let (result_with_timings, scheduler2) = scheduler2.into_inner();
        assert_matches!(result_with_timings, (Ok(()), _));
        pool.return_scheduler(scheduler2);

        let scheduler3 = pool.do_take_scheduler(context.clone());
        assert_eq!(scheduler_id2, scheduler3.id());
        let scheduler4 = pool.do_take_scheduler(context.clone());
        assert_eq!(scheduler_id1, scheduler4.id());
    }

    #[test]
    fn test_scheduler_pool_context_drop_unless_reinitialized() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = DefaultSchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(bank);
        let mut scheduler = pool.do_take_scheduler(context.clone());

        // should never panic.
        scheduler.pause_for_recent_blockhash();
        assert_matches!(
            Box::new(scheduler).wait_for_termination(false),
            ((Ok(()), _), _)
        );
    }

    #[test]
    fn test_scheduler_pool_context_replace() {
        solana_logger::setup();

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = DefaultSchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let old_bank = &Arc::new(Bank::default_for_tests());
        let new_bank = &Arc::new(Bank::default_for_tests());
        assert!(!Arc::ptr_eq(old_bank, new_bank));

        let old_context = &SchedulingContext::new(old_bank.clone());
        let new_context = &SchedulingContext::new(new_bank.clone());

        let scheduler = pool.do_take_scheduler(old_context.clone());
        let scheduler_id = scheduler.id();
        pool.return_scheduler(scheduler.into_inner().1);

        let scheduler = pool.take_scheduler(new_context.clone());
        assert_eq!(scheduler_id, scheduler.id());
        assert!(Arc::ptr_eq(scheduler.context().bank(), new_bank));
    }

    #[test]
    fn test_scheduler_pool_install_into_bank_forks() {
        solana_logger::setup();

        let bank = Bank::default_for_tests();
        let bank_forks = BankForks::new_rw_arc(bank);
        let mut bank_forks = bank_forks.write().unwrap();
        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        bank_forks.install_scheduler_pool(pool);
    }

    #[test]
    fn test_scheduler_install_into_bank() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let child_bank = Bank::new_from_parent(bank, &Pubkey::default(), 1);

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);

        let bank = Bank::default_for_tests();
        let bank_forks = BankForks::new_rw_arc(bank);
        let mut bank_forks = bank_forks.write().unwrap();

        // existing banks in bank_forks shouldn't process transactions anymore in general, so
        // shouldn't be touched
        assert!(!bank_forks
            .working_bank_with_scheduler()
            .has_installed_scheduler());
        bank_forks.install_scheduler_pool(pool);
        assert!(!bank_forks
            .working_bank_with_scheduler()
            .has_installed_scheduler());

        let mut child_bank = bank_forks.insert(child_bank);
        assert!(child_bank.has_installed_scheduler());
        bank_forks.remove(child_bank.slot());
        child_bank.drop_scheduler();
        assert!(!child_bank.has_installed_scheduler());
    }

    fn setup_dummy_fork_graph(bank: Bank) -> Arc<Bank> {
        let slot = bank.slot();
        let bank_fork = BankForks::new_rw_arc(bank);
        let bank = bank_fork.read().unwrap().get(slot).unwrap();
        bank.loaded_programs_cache
            .write()
            .unwrap()
            .set_fork_graph(bank_fork);
        bank
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
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = setup_dummy_fork_graph(bank);
        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(bank.clone());

        assert_eq!(bank.transaction_count(), 0);
        let scheduler = pool.take_scheduler(context);
        scheduler.schedule_execution(&(tx0, 0));
        let bank = BankWithScheduler::new(bank, Some(scheduler));
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
        let bank = Bank::new_for_tests(&genesis_config);
        let bank = setup_dummy_fork_graph(bank);

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            DefaultSchedulerPool::new_dyn(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(bank.clone());
        let mut scheduler = pool.take_scheduler(context);

        let unfunded_keypair = Keypair::new();
        let bad_tx =
            &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &unfunded_keypair,
                &solana_sdk::pubkey::new_rand(),
                2,
                genesis_config.hash(),
            ));
        assert_eq!(bank.transaction_count(), 0);
        scheduler.schedule_execution(&(bad_tx, 0));
        scheduler.pause_for_recent_blockhash();
        assert_eq!(bank.transaction_count(), 0);

        let good_tx_after_bad_tx =
            &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &mint_keypair,
                &solana_sdk::pubkey::new_rand(),
                3,
                genesis_config.hash(),
            ));
        // make sure this tx is really a good one to execute.
        assert_matches!(
            bank.simulate_transaction_unchecked(good_tx_after_bad_tx, false)
                .result,
            Ok(_)
        );
        scheduler.schedule_execution(&(good_tx_after_bad_tx, 0));
        scheduler.pause_for_recent_blockhash();
        // transaction_count should remain same as scheduler should be bailing out.
        assert_eq!(bank.transaction_count(), 0);

        let bank = BankWithScheduler::new(bank, Some(scheduler));
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
        PooledScheduler<DefaultTaskHandler>,
        Mutex<Vec<JoinHandle<ResultWithTimings>>>,
    );

    impl<const TRIGGER_RACE_CONDITION: bool> AsyncScheduler<TRIGGER_RACE_CONDITION> {
        fn do_wait(&self) {
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
            *self.0.result_with_timings.lock().unwrap() = (overall_result, overall_timings);
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool> InstalledScheduler
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn id(&self) -> SchedulerId {
            self.0.id()
        }

        fn context(&self) -> &SchedulingContext {
            self.0.context()
        }

        fn schedule_execution(&self, &(transaction, index): &(&SanitizedTransaction, usize)) {
            let transaction_and_index = (transaction.clone(), index);
            let context = self.context().clone();
            let pool = self.0.inner.pool.clone();

            self.1.lock().unwrap().push(std::thread::spawn(move || {
                // intentionally sleep to simulate race condition where register_recent_blockhash
                // is handle before finishing executing scheduled transactions
                std::thread::sleep(std::time::Duration::from_secs(1));

                let mut result = Ok(());
                let mut timings = ExecuteTimings::default();

                <DefaultTaskHandler as TaskHandler>::handle(
                    &mut result,
                    &mut timings,
                    context.bank(),
                    &transaction_and_index.0,
                    transaction_and_index.1,
                    &pool.handler_context,
                );
                (result, timings)
            }));
        }

        fn wait_for_termination(
            self: Box<Self>,
            is_dropped: bool,
        ) -> (ResultWithTimings, UninstalledSchedulerBox) {
            self.do_wait();
            Box::new(self.0).wait_for_termination(is_dropped)
        }

        fn pause_for_recent_blockhash(&mut self) {
            if TRIGGER_RACE_CONDITION {
                // this is equivalent to NOT calling wait_for_paused_scheduler() in
                // register_recent_blockhash().
                return;
            }
            self.do_wait();
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool> SpawnableScheduler<DefaultTaskHandler>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        // well, i wish i can use ! (never type).....
        type Inner = Self;

        fn into_inner(self) -> (ResultWithTimings, Self::Inner) {
            todo!();
        }

        fn from_inner(_inner: Self::Inner, _context: SchedulingContext) -> Self {
            todo!();
        }

        fn spawn(
            pool: Arc<SchedulerPool<Self, DefaultTaskHandler>>,
            initial_context: SchedulingContext,
        ) -> Self {
            AsyncScheduler::<TRIGGER_RACE_CONDITION>(
                PooledScheduler::<DefaultTaskHandler>::from_inner(
                    PooledSchedulerInner {
                        id: pool.new_scheduler_id(),
                        pool: SchedulerPool::new(
                            pool.handler_context.log_messages_bytes_limit,
                            pool.handler_context.transaction_status_sender.clone(),
                            pool.handler_context.replay_vote_sender.clone(),
                            pool.handler_context.prioritization_fee_cache.clone(),
                        ),
                    },
                    initial_context,
                ),
                Mutex::new(vec![]),
            )
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
        let mut bank = Bank::new_for_tests(&genesis_config);
        for _ in 0..MAX_PROCESSING_AGE {
            bank.fill_bank_with_ticks_for_tests();
            bank.freeze();
            let slot = bank.slot();
            bank = Bank::new_from_parent(
                Arc::new(bank),
                &Pubkey::default(),
                slot.checked_add(1).unwrap(),
            );
        }
        let bank = setup_dummy_fork_graph(bank);
        let context = SchedulingContext::new(bank.clone());

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool =
            SchedulerPool::<AsyncScheduler<TRIGGER_RACE_CONDITION>, DefaultTaskHandler>::new_dyn(
                None,
                None,
                None,
                ignored_prioritization_fee_cache,
            );
        let scheduler = pool.take_scheduler(context);

        let bank = BankWithScheduler::new(bank, Some(scheduler));
        assert_eq!(bank.transaction_count(), 0);

        // schedule but not immediately execute transaction
        bank.schedule_transaction_executions([(&very_old_valid_tx, &0)].into_iter());
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
