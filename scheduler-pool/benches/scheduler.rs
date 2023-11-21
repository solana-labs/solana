#![feature(test)]
#![allow(clippy::arithmetic_side_effects)]

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

extern crate test;

use {
    assert_matches::assert_matches,
    log::*,
    rand::{thread_rng, Rng},
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        bank::Bank,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        installed_scheduler_pool::{
            InstalledScheduler, ResultWithTimings, ScheduleExecutionArg, SchedulerId,
            SchedulingContext, WaitReason, WithTransactionAndIndex,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_scheduler::SchedulingMode,
    solana_scheduler_pool::{
        Handler, InstallableScheduler, PooledScheduler, SchedulerPool, SpawnableScheduler,
    },
    solana_sdk::{
        system_transaction,
        transaction::{Result, SanitizedTransaction},
    },
    std::{
        fmt::Debug,
        marker::{PhantomData, Send, Sync},
        mem,
        sync::Arc,
    },
    test::Bencher,
};

const TX_COUNT: usize = 10_000;

#[derive(Debug, Default, Clone)]
struct ScheduleExecutionArgForBench;

// use Arc-ed transaction for very cheap .clone() so that the consumer is never starved for
// incoming transactions.
type TransactionWithIndexForBench = Arc<(SanitizedTransaction, usize)>;

impl ScheduleExecutionArg for ScheduleExecutionArgForBench {
    type TransactionWithIndex<'_tx> = TransactionWithIndexForBench;
}

#[derive(Debug, Default, Clone)]
struct BenchFriendlyHandler<SEA: ScheduleExecutionArg + Clone, const MUTATE_ARC: bool>(
    PhantomData<SEA>,
);

impl<SEA: ScheduleExecutionArg + Clone, const MUTATE_ARC: bool> Handler<SEA>
    for BenchFriendlyHandler<SEA, MUTATE_ARC>
{
    fn create<T: SpawnableScheduler<Self, SEA>>(_pool: &SchedulerPool<T, Self, SEA>) -> Self {
        Self(PhantomData)
    }

    fn handle<T: SpawnableScheduler<Self, SEA>>(
        &self,
        _result: &mut Result<()>,
        _timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        _index: usize,
        _pool: &SchedulerPool<T, Self, SEA>,
    ) {
        //std::hint::black_box(bank.clone());
        let mut i = 0;
        for _ in 0..10 {
            if MUTATE_ARC {
                //for _ in 0..2 {
                std::hint::black_box((Arc::downgrade(bank)).upgrade().unwrap());
                //}
            }
            // call random one of Bank's lightweight-and-very-multi-threaded-friendly methods which take a
            // transaction inside this artifical tight loop.
            i += bank.get_fee_for_message_with_lamports_per_signature(transaction.message(), i)
        }
        std::hint::black_box(i);
    }
}

type BenchFriendlyHandlerWithArcMutation = BenchFriendlyHandler<ScheduleExecutionArgForBench, true>;
type BenchFriendlyHandlerWithoutArcMutation =
    BenchFriendlyHandler<ScheduleExecutionArgForBench, false>;

fn run_bench<
    F: FnOnce(Arc<SchedulerPool<I, TH, ScheduleExecutionArgForBench>>, SchedulingContext) -> I,
    I: SpawnableScheduler<TH, ScheduleExecutionArgForBench>,
    TH: Handler<ScheduleExecutionArgForBench>,
>(
    bencher: &mut Bencher,
    create_scheduler: F,
) {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(1_000_000_000);
    let bank = &Arc::new(Bank::new_for_tests(&genesis_config));
    let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
    let pool = SchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
    let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

    let mut scheduler = create_scheduler(pool, context.clone());
    let tx0 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
        &mint_keypair,
        &solana_sdk::pubkey::new_rand(),
        2,
        genesis_config.hash(),
    ));
    let tx_with_index = TransactionWithIndexForBench::new((tx0.clone(), 0));
    bencher.iter(|| {
        for _ in 0..TX_COUNT {
            scheduler.schedule_execution(tx_with_index.clone());
        }
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
            Some((Ok(()), _))
        );
        scheduler.replace_context(context.clone());
    });
}

mod blocking_ref {
    use {super::*, solana_runtime::installed_scheduler_pool::DefaultScheduleExecutionArg};

    #[bench]
    fn bench_without_arc_mutation(bencher: &mut Bencher) {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1_000_000_000);
        let bank = &Arc::new(Bank::new_for_tests(&genesis_config));
        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        let mut scheduler = PooledScheduler::<_, DefaultScheduleExecutionArg>::do_spawn(
            pool,
            context.clone(),
            BenchFriendlyHandler::<_, false>::default(),
        );
        let tx0 = &SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            2,
            genesis_config.hash(),
        ));
        let tx_with_index = &(tx0, 0);
        bencher.iter(|| {
            for _ in 0..TX_COUNT {
                scheduler.schedule_execution(tx_with_index);
            }
            assert_matches!(
                scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
                Some((Ok(()), _))
            );
            scheduler.replace_context(context.clone());
        });
    }
}

mod blocking {
    use super::*;

    type BlockingScheduler<H> = PooledScheduler<H, ScheduleExecutionArgForBench>;

    #[bench]
    fn bench_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            BlockingScheduler::do_spawn(
                pool,
                context,
                BenchFriendlyHandlerWithArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            BlockingScheduler::do_spawn(
                pool,
                context,
                BenchFriendlyHandlerWithoutArcMutation::default(),
            )
        });
    }
}

mod nonblocking {
    use super::*;

    #[derive(Debug)]
    pub(super) struct NonblockingScheduler<H: Handler<ScheduleExecutionArgForBench> + Clone> {
        id: SchedulerId,
        pub(crate) pool: Arc<SchedulerPool<Self, H, ScheduleExecutionArgForBench>>,
        transaction_sender: crossbeam_channel::Sender<ChainedChannel>,
        result_receiver: crossbeam_channel::Receiver<(Result<()>, ExecuteTimings, usize)>,
        lane_count: usize,
        context: SchedulingContext,
        _phantom: PhantomData<H>,
    }

    enum ChainedChannel {
        Payload(TransactionWithIndexForBench),
        NextContext(SchedulingContext),
        NextChannel(Box<dyn WithChannelPair + Send + Sync>),
    }

    type ChannelPair = (
        crossbeam_channel::Receiver<ChainedChannel>,
        crossbeam_channel::Sender<(Result<()>, ExecuteTimings, usize)>,
    );

    trait WithChannelPair {
        fn unwrap_channel_pair(&mut self) -> ChannelPair;
    }

    struct ChannelPairOption(Option<ChannelPair>);

    impl WithChannelPair for ChannelPairOption {
        fn unwrap_channel_pair(&mut self) -> ChannelPair {
            self.0.take().unwrap()
        }
    }

    impl<H: Handler<ScheduleExecutionArgForBench> + Clone>
        SpawnableScheduler<H, ScheduleExecutionArgForBench> for NonblockingScheduler<H>
    {
        fn spawn(
            _pool: Arc<SchedulerPool<Self, H, ScheduleExecutionArgForBench>>,
            _initial_context: SchedulingContext,
            _handler: H,
        ) -> Self {
            unimplemented!();
        }

        fn should_retain_in_pool(&mut self) -> bool {
            unimplemented!();
        }
    }

    impl<H: Handler<ScheduleExecutionArgForBench> + Clone> NonblockingScheduler<H> {
        pub(super) fn spawn(
            pool: Arc<SchedulerPool<Self, H, ScheduleExecutionArgForBench>>,
            initial_context: SchedulingContext,
            lane_count: usize,
            handler: H,
        ) -> Self {
            let (transaction_sender, transaction_receiver) =
                crossbeam_channel::unbounded::<ChainedChannel>();
            let (result_sender, result_receiver) = crossbeam_channel::unbounded();

            for _ in 0..lane_count {
                let mut bank = Arc::clone(initial_context.bank());
                let mut transaction_receiver = transaction_receiver.clone();
                let mut result_sender = result_sender.clone();
                std::thread::spawn({
                    let pool = pool.clone();
                    let handler = handler.clone();
                    move || {
                        let mut result = Ok(());
                        let mut timings = ExecuteTimings::default();
                        let mut count = 0;
                        while let Ok(message) = transaction_receiver.recv() {
                            match message {
                                ChainedChannel::Payload(with_transaction_and_index) => {
                                    count += 1;
                                    with_transaction_and_index.with_transaction_and_index(
                                        |transaction, index| {
                                            H::handle(
                                                &handler,
                                                &mut result,
                                                &mut timings,
                                                &bank,
                                                transaction,
                                                index,
                                                &pool,
                                            );
                                        },
                                    );
                                }
                                ChainedChannel::NextContext(next_context) => {
                                    bank = next_context.bank().clone();
                                }
                                ChainedChannel::NextChannel(mut next_receiver_box) => {
                                    result_sender
                                        .send((
                                            mem::replace(&mut result, Ok(())),
                                            mem::take(&mut timings),
                                            mem::take(&mut count),
                                        ))
                                        .unwrap();
                                    (transaction_receiver, result_sender) =
                                        next_receiver_box.unwrap_channel_pair();
                                }
                            }
                        }
                    }
                });
            }

            Self {
                id: thread_rng().gen::<SchedulerId>(),
                pool,
                transaction_sender,
                result_receiver,
                lane_count,
                context: initial_context,
                _phantom: PhantomData,
            }
        }
    }
    impl<H: Handler<ScheduleExecutionArgForBench> + Clone>
        InstalledScheduler<ScheduleExecutionArgForBench> for NonblockingScheduler<H>
    {
        fn id(&self) -> SchedulerId {
            self.id
        }

        fn context(&self) -> SchedulingContext {
            self.context.clone()
        }

        fn schedule_execution(&self, transaction_with_index: TransactionWithIndexForBench) {
            self.transaction_sender
                .send(ChainedChannel::Payload(transaction_with_index))
                .unwrap();
        }

        fn wait_for_termination(&mut self, _wait_reason: &WaitReason) -> Option<ResultWithTimings> {
            let (next_transaction_sender, next_transaction_receiver) =
                crossbeam_channel::unbounded::<ChainedChannel>();
            let (next_result_sender, next_result_receiver) = crossbeam_channel::unbounded();
            for _ in 0..self.lane_count {
                let (next_transaction_receiver, next_result_sender) = (
                    next_transaction_receiver.clone(),
                    next_result_sender.clone(),
                );
                self.transaction_sender
                    .send(ChainedChannel::NextChannel(Box::new(ChannelPairOption(
                        Some((next_transaction_receiver, next_result_sender)),
                    ))))
                    .unwrap();
            }
            self.transaction_sender = next_transaction_sender;

            let mut overall_result = Ok(());
            let mut overall_timings = ExecuteTimings::default();

            while let Ok((result, timings, count)) = self.result_receiver.recv() {
                match result {
                    Ok(()) => {}
                    Err(e) => overall_result = Err(e),
                }
                overall_timings.accumulate(&timings);
                trace!("received: {count:?}");
            }
            self.result_receiver = next_result_receiver;

            Some((overall_result, overall_timings))
        }

        fn return_to_pool(self: Box<Self>) {
            self.pool.clone().return_scheduler(self)
        }
    }

    impl<H: Handler<ScheduleExecutionArgForBench> + Clone>
        InstallableScheduler<ScheduleExecutionArgForBench> for NonblockingScheduler<H>
    {
        fn has_context(&self) -> bool {
            true
        }

        fn replace_context(&mut self, context: SchedulingContext) {
            for _ in 0..self.lane_count {
                self.transaction_sender
                    .send(ChainedChannel::NextContext(context.clone()))
                    .unwrap();
            }
            self.context = context;
        }
    }

    #[bench]
    fn bench_with_01_thread_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                1,
                BenchFriendlyHandlerWithArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_01_thread_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                1,
                BenchFriendlyHandlerWithoutArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_04_threads_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                4,
                BenchFriendlyHandlerWithArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_04_threads_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                4,
                BenchFriendlyHandlerWithoutArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_08_threads_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                8,
                BenchFriendlyHandlerWithArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_08_threads_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                8,
                BenchFriendlyHandlerWithoutArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_16_threads_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                16,
                BenchFriendlyHandlerWithArcMutation::default(),
            )
        });
    }

    #[bench]
    fn bench_with_16_threads_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::spawn(
                pool,
                context,
                16,
                BenchFriendlyHandlerWithoutArcMutation::default(),
            )
        });
    }
}

// demonstrate meaningfully differing performance profile regarding multi worker thread utilization
// with saturated transaction execution for each bench scenarios, with/without the existence of
// artificial and needless synchronizations.
// conversely, the whole InstallableScheduler machinery can be justified as it can eliminate these
// synchronizations altogether to bare minimum (i.e. bank freeze).
mod thread_utilization {
    use {
        super::*,
        crate::nonblocking::NonblockingScheduler,
        solana_nohash_hasher::IntSet,
        solana_sdk::{
            signature::Signature, signer::keypair::Keypair,
            system_instruction::SystemInstruction::Transfer, transaction::TransactionAccountLocks,
        },
        std::{collections::HashMap, sync::Mutex, thread::sleep, time::Duration},
    };

    #[derive(Debug, Clone)]
    struct SleepyHandler;

    impl<SEA: ScheduleExecutionArg> Handler<SEA> for SleepyHandler {
        fn create<T: SpawnableScheduler<Self, SEA>>(_pool: &SchedulerPool<T, Self, SEA>) -> Self {
            Self
        }

        fn handle<T: SpawnableScheduler<Self, SEA>>(
            &self,
            _result: &mut Result<()>,
            _timings: &mut ExecuteTimings,
            _bank: &Arc<Bank>,
            transaction: &SanitizedTransaction,
            _index: usize,
            _pool: &SchedulerPool<T, Self, SEA>,
        ) {
            let Ok(Transfer { lamports: sleep_ms }) =
                bincode::deserialize(&transaction.message().instructions()[0].data)
            else {
                panic!()
            };

            sleep(Duration::from_millis(sleep_ms));
        }
    }

    enum Step {
        Batch(Vec<TransactionWithIndexForBench>),
        // mimic periodic or contention-induced synchronization with this artificial blocking
        MaySynchronize,
    }

    const WORKER_THREAD_COUNT: usize = 10;

    fn simulate_synchronization_point<T: InstallableScheduler<ScheduleExecutionArgForBench>>(
        scheduler: &mut T,
        context: SchedulingContext,
    ) {
        assert_matches!(
            scheduler.wait_for_termination(&WaitReason::TerminatedToFreeze),
            Some((Ok(()), _))
        );
        scheduler.replace_context(context);
    }

    fn run_scenario_and_finalize<T: InstallableScheduler<ScheduleExecutionArgForBench>>(
        bencher: &mut Bencher,
        really_synchronize: bool,
        scheduler: &mut T,
        context: SchedulingContext,
        create_scenario: impl Fn() -> Vec<Step>,
    ) {
        let scenario = &create_scenario();
        bencher.iter(|| {
            for step in scenario {
                match step {
                    Step::Batch(txes) => {
                        for tx in txes {
                            scheduler.schedule_execution(tx.clone());
                        }
                    }
                    Step::MaySynchronize => {
                        if really_synchronize {
                            simulate_synchronization_point(scheduler, context.clone());
                        }
                    }
                }
            }
            simulate_synchronization_point(scheduler, context.clone());
        })
    }

    // frequent synchronization creates non-zero idling time among some of worker threads, given
    // batches with mixed transactions. then, it adds up as these kinds synchronizations occurs over
    // processing
    fn bench_random_execution_durations(bencher: &mut Bencher, really_synchronize: bool) {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1_000_000_000);
        let bank = &Arc::new(Bank::new_for_tests(&genesis_config));

        let create_tx_with_index = |index| {
            let tx0 =
                SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                    &mint_keypair,
                    &solana_sdk::pubkey::new_rand(),
                    // simulate somewhat realistic work load; txes finish at different timings
                    thread_rng().gen_range(1..10),
                    genesis_config.hash(),
                ));
            TransactionWithIndexForBench::new((tx0, index))
        };

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());
        let mut scheduler =
            NonblockingScheduler::spawn(pool, context.clone(), WORKER_THREAD_COUNT, SleepyHandler);

        run_scenario_and_finalize(bencher, really_synchronize, &mut scheduler, context, || {
            const TX_PER_BATCH: usize = 20;
            const SYNCHRONIZATION_PER_BENCH_ITER: usize = 10;

            (0..SYNCHRONIZATION_PER_BENCH_ITER)
                .flat_map(|_| {
                    [
                        Step::Batch((0..TX_PER_BATCH).map(create_tx_with_index).collect()),
                        Step::MaySynchronize,
                    ]
                })
                .collect()
        });
    }

    #[bench]
    fn bench_random_execution_durations_with_interleaved_synchronization(bencher: &mut Bencher) {
        bench_random_execution_durations(bencher, true);
    }

    #[bench]
    fn bench_random_execution_durations_without_interleaved_synchronization(bencher: &mut Bencher) {
        bench_random_execution_durations(bencher, false);
    }

    #[derive(Debug, Clone)]
    struct SleepyHandlerWithCompletionSignal(crossbeam_channel::Sender<Signature>);

    impl<SEA: ScheduleExecutionArg> Handler<SEA> for SleepyHandlerWithCompletionSignal {
        fn create<T: SpawnableScheduler<Self, SEA>>(_pool: &SchedulerPool<T, Self, SEA>) -> Self {
            // not needed for bench...
            unimplemented!();
        }

        fn handle<T: SpawnableScheduler<Self, SEA>>(
            &self,
            _result: &mut Result<()>,
            _timings: &mut ExecuteTimings,
            _bank: &Arc<Bank>,
            transaction: &SanitizedTransaction,
            _index: usize,
            _pool: &SchedulerPool<T, Self, SEA>,
        ) {
            let Ok(Transfer { lamports: sleep_ms }) =
                bincode::deserialize(&transaction.message().instructions()[0].data)
            else {
                panic!()
            };

            sleep(Duration::from_millis(sleep_ms));

            self.0.send(*transaction.signature()).unwrap();
        }
    }

    // a wrapper InstallableScheduler to integrate with dep graph scheduling logic
    #[derive(Debug)]
    struct NonblockingSchedulerWithDepGraph {
        inner_scheduler: NonblockingScheduler<SleepyHandlerWithCompletionSignal>,
        pending_transactions: Mutex<Vec<SanitizedTransaction>>,
        completion_receiver: crossbeam_channel::Receiver<Signature>,
    }

    impl InstalledScheduler<ScheduleExecutionArgForBench> for NonblockingSchedulerWithDepGraph {
        fn id(&self) -> SchedulerId {
            self.inner_scheduler.id()
        }

        fn context(&self) -> SchedulingContext {
            self.inner_scheduler.context().clone()
        }

        fn schedule_execution(&self, transaction_with_index: TransactionWithIndexForBench) {
            // just buffer all the txes to work with the dep graph outer loop nicely, which needs
            // some buffering to schedule efficiently
            // note taht the prompt execution as soon as entering into schedule_execution() isn't
            // needed for these particular bench purposes. so, buffering is okay in that regard.
            self.pending_transactions
                .lock()
                .unwrap()
                .push(transaction_with_index.0.clone());
        }

        fn wait_for_termination(&mut self, reason: &WaitReason) -> Option<ResultWithTimings> {
            // execute all the pending transactions now!
            self.execute_batches(
                self.context().bank(),
                &std::mem::take(&mut *self.pending_transactions.lock().unwrap()),
                &self.completion_receiver,
            )
            .unwrap();

            self.inner_scheduler.wait_for_termination(reason)
        }

        fn return_to_pool(self: Box<Self>) {
            Box::new(self.inner_scheduler).return_to_pool()
        }
    }

    impl InstallableScheduler<ScheduleExecutionArgForBench> for NonblockingSchedulerWithDepGraph {
        fn has_context(&self) -> bool {
            self.inner_scheduler.has_context()
        }

        fn replace_context(&mut self, context: SchedulingContext) {
            self.inner_scheduler.replace_context(context)
        }
    }

    // adapted from https://github.com/jito-foundation/jito-solana/pull/294; retained to be as-is
    // as much as possible by the use of some wrapper type hackery.
    impl NonblockingSchedulerWithDepGraph {
        // for each index, builds a transaction dependency graph of indices that need to execute before
        // the current one.
        // The returned Vec<HashSet<usize>> is a 1:1 mapping for the indices that need to be executed
        // before that index can be executed
        fn build_dependency_graph(
            tx_account_locks: &[TransactionAccountLocks],
        ) -> Vec<IntSet<usize>> {
            // build a map whose key is a pubkey + value is a sorted vector of all indices that
            // lock that account
            let mut indices_read_locking_account = HashMap::new();
            let mut indicies_write_locking_account = HashMap::new();
            tx_account_locks
                .iter()
                .enumerate()
                .for_each(|(idx, tx_account_locks)| {
                    for account in &tx_account_locks.readonly {
                        indices_read_locking_account
                            .entry(**account)
                            .and_modify(|indices: &mut Vec<usize>| indices.push(idx))
                            .or_insert_with(|| vec![idx]);
                    }
                    for account in &tx_account_locks.writable {
                        indicies_write_locking_account
                            .entry(**account)
                            .and_modify(|indices: &mut Vec<usize>| indices.push(idx))
                            .or_insert_with(|| vec![idx]);
                    }
                });

            tx_account_locks
                .iter()
                .enumerate()
                .map(|(idx, account_locks)| {
                    let mut dep_graph: IntSet<usize> = IntSet::default();

                    let readlock_conflict_accs = account_locks.writable.iter();
                    let writelock_conflict_accs = account_locks
                        .readonly
                        .iter()
                        .chain(account_locks.writable.iter());

                    for acc in readlock_conflict_accs {
                        if let Some(indices) = indices_read_locking_account.get(acc) {
                            dep_graph.extend(indices.iter().take_while(|l_idx| **l_idx < idx));
                        }
                    }

                    for acc in writelock_conflict_accs {
                        if let Some(indices) = indicies_write_locking_account.get(acc) {
                            dep_graph.extend(indices.iter().take_while(|l_idx| **l_idx < idx));
                        }
                    }
                    dep_graph
                })
                .collect()
        }

        fn execute_batches(
            &self,
            bank: &Arc<Bank>,
            pending_transactions: &[SanitizedTransaction],
            receiver: &crossbeam_channel::Receiver<Signature>,
        ) -> Result<()> {
            if pending_transactions.is_empty() {
                return Ok(());
            }

            let mut tx_account_locks: Vec<_> = Vec::with_capacity(pending_transactions.len());
            for tx in pending_transactions {
                tx_account_locks
                    .push(tx.get_account_locks(bank.get_transaction_account_lock_limit())?);
            }

            // the dependency graph contains the indices that must be executed (marked with
            // State::Done) before they can be executed
            let dependency_graph = Self::build_dependency_graph(&tx_account_locks);

            #[derive(Clone)]
            enum State {
                Blocked,
                Processing,
                Done,
            }

            let mut processing_states: Vec<State> = vec![State::Blocked; dependency_graph.len()];
            let mut signature_indices: HashMap<&Signature, usize> =
                HashMap::with_capacity(dependency_graph.len());
            signature_indices.extend(
                pending_transactions
                    .iter()
                    .enumerate()
                    .map(|(idx, tx)| (tx.signature(), idx)),
            );

            loop {
                let mut is_done = true;
                for idx in 0..processing_states.len() {
                    match processing_states[idx] {
                        State::Blocked => {
                            is_done = false;

                            // if all the dependent txs are executed, this transaction can be
                            // scheduled for execution.
                            if dependency_graph[idx]
                                .iter()
                                .all(|idx| matches!(processing_states[*idx], State::Done))
                            {
                                self.inner_scheduler.schedule_execution(Arc::new((
                                    pending_transactions[idx].clone(),
                                    idx,
                                )));
                                // this idx can be scheduled and moved to processing
                                processing_states[idx] = State::Processing;
                            }
                        }
                        State::Processing => {
                            is_done = false;
                        }
                        State::Done => {}
                    }
                }

                if is_done {
                    break;
                }

                let mut executor_responses: Vec<_> = vec![receiver.recv().unwrap()];
                executor_responses.extend(receiver.try_iter());
                for r in &executor_responses {
                    processing_states[*signature_indices.get(r).unwrap()] = State::Done;
                }
            }
            Ok(())
        }
    }

    // frequent synchronizations hampers efficient (= parallelizable) scheduling of several chunks
    // of txes which are tied together for each common account locks. Ideally those independent chunks can be
    // executed in parallel, which each is consuming one worker thread as a form of serialized runs
    // of processing. However, should synchronizations occurs between boundaries of those chunks
    // arrival, it cannot schedule the later-coming one because it firstly flush out the the first
    // one
    // in other words, this is just a re-manifestation of perf. issue coming from write barriers in
    // general.
    fn bench_long_serialized_runs(bencher: &mut Bencher, really_synchronize: bool) {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(1_000_000_000);
        let bank = &Arc::new(Bank::new_for_tests(&genesis_config));
        let (kp1, kp2) = (Keypair::new(), Keypair::new());

        let create_tx_of_serialized_run1 = || {
            let tx0 =
                SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                    &kp1,
                    &solana_sdk::pubkey::new_rand(),
                    10,
                    genesis_config.hash(),
                ));
            TransactionWithIndexForBench::new((tx0, 0))
        };
        let create_tx_of_serialized_run2 = || {
            let tx0 =
                SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                    &kp2,
                    &solana_sdk::pubkey::new_rand(),
                    10,
                    genesis_config.hash(),
                ));
            TransactionWithIndexForBench::new((tx0, 0))
        };

        let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new(None, None, None, ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());
        let (completion_sender, completion_receiver) = crossbeam_channel::unbounded();
        let handler = SleepyHandlerWithCompletionSignal(completion_sender);
        let tx_lock_ignoring_scheduler =
            NonblockingScheduler::spawn(pool, context.clone(), WORKER_THREAD_COUNT, handler);
        let tx_lock_adhering_scheduler = NonblockingSchedulerWithDepGraph {
            inner_scheduler: tx_lock_ignoring_scheduler,
            pending_transactions: Mutex::new(Vec::default()),
            completion_receiver,
        };
        let mut scheduler = tx_lock_adhering_scheduler;
        run_scenario_and_finalize(bencher, really_synchronize, &mut scheduler, context, || {
            (0..1)
                .flat_map(|_| {
                    [
                        Step::Batch(vec![create_tx_of_serialized_run1()]),
                        Step::Batch(vec![create_tx_of_serialized_run1()]),
                        Step::Batch(vec![create_tx_of_serialized_run1()]),
                        Step::Batch(vec![create_tx_of_serialized_run1()]),
                        Step::MaySynchronize,
                        Step::Batch(vec![create_tx_of_serialized_run2()]),
                        Step::Batch(vec![create_tx_of_serialized_run2()]),
                        Step::Batch(vec![create_tx_of_serialized_run2()]),
                        Step::Batch(vec![create_tx_of_serialized_run2()]),
                        Step::MaySynchronize,
                    ]
                })
                .collect()
        });
    }

    #[bench]
    fn bench_long_serialized_runs_with_interleaved_synchronization(bencher: &mut Bencher) {
        bench_long_serialized_runs(bencher, true);
    }

    #[bench]
    fn bench_long_serialized_runs_without_interleaved_synchronization(bencher: &mut Bencher) {
        bench_long_serialized_runs(bencher, false);
    }
}
