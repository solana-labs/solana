#![feature(test)]
#![allow(clippy::integer_arithmetic)]

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
            InstalledScheduler, InstalledSchedulerPoolArc, ResultWithTimings, ScheduleExecutionArg,
            SchedulerId, SchedulingContext, WaitReason, WithTransactionAndIndex,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_scheduler::SchedulingMode,
    solana_scheduler_pool::{PooledScheduler, ScheduledTransactionHandler, SchedulerPool},
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

#[derive(Debug)]
struct ScheduleExecutionArgForBench;

// use Arc-ed transaction for very cheap .clone() so that the consumer is never starved for
// incoming transactions.
type TransactionWithIndexForBench = Arc<(SanitizedTransaction, usize)>;

impl ScheduleExecutionArg for ScheduleExecutionArgForBench {
    type TransactionWithIndex<'_tx> = TransactionWithIndexForBench;
}

#[derive(Debug)]
struct BenchFriendlyHandler<SEA: ScheduleExecutionArg, const MUTATE_ARC: bool>(
    std::marker::PhantomData<SEA>,
);

impl<SEA: ScheduleExecutionArg, const MUTATE_ARC: bool> ScheduledTransactionHandler<SEA>
    for BenchFriendlyHandler<SEA, MUTATE_ARC>
{
    fn handle(
        _result: &mut Result<()>,
        _timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction_with_index: SEA::TransactionWithIndex<'_>,
        _pool: &SchedulerPool,
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
            transaction_with_index.with_transaction_and_index(|transaction, _index| {
                i += bank.get_fee_for_message_with_lamports_per_signature(transaction.message(), i)
            });
        }
        std::hint::black_box(i);
    }
}

type BenchFriendlyHandlerWithArcMutation = BenchFriendlyHandler<ScheduleExecutionArgForBench, true>;
type BenchFriendlyHandlerWithoutArcMutation =
    BenchFriendlyHandler<ScheduleExecutionArgForBench, false>;

fn run_bench<
    F: FnOnce(Arc<SchedulerPool>, SchedulingContext) -> I,
    I: InstalledScheduler<ScheduleExecutionArgForBench>,
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
    let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
    let pool = SchedulerPool::new(None, None, None, _ignored_prioritization_fee_cache);
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
        let _ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
        let pool = SchedulerPool::new(None, None, None, _ignored_prioritization_fee_cache);
        let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());

        let mut scheduler = PooledScheduler::<
            BenchFriendlyHandler<DefaultScheduleExecutionArg, false>,
            DefaultScheduleExecutionArg,
        >::spawn(pool, context.clone());
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
            BlockingScheduler::<BenchFriendlyHandlerWithArcMutation>::spawn(pool, context)
        });
    }

    #[bench]
    fn bench_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            BlockingScheduler::<BenchFriendlyHandlerWithoutArcMutation>::spawn(pool, context)
        });
    }
}

mod nonblocking {
    use super::*;

    #[derive(Debug)]
    struct NonblockingScheduler<H: ScheduledTransactionHandler<ScheduleExecutionArgForBench>> {
        id: SchedulerId,
        pool: Arc<SchedulerPool>,
        transaction_sender: crossbeam_channel::Sender<ChainedChannel>,
        result_receiver: crossbeam_channel::Receiver<(Result<()>, ExecuteTimings, usize)>,
        lane_count: usize,
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

    impl<H: ScheduledTransactionHandler<ScheduleExecutionArgForBench>> NonblockingScheduler<H> {
        fn spawn(
            pool: Arc<SchedulerPool>,
            initial_context: SchedulingContext,
            lane_count: usize,
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
                    move || {
                        let mut result = Ok(());
                        let mut timings = ExecuteTimings::default();
                        let mut count = 0;
                        while let Ok(message) = transaction_receiver.recv() {
                            match message {
                                ChainedChannel::Payload(with_transaction_and_index) => {
                                    count += 1;
                                    H::handle(
                                        &mut result,
                                        &mut timings,
                                        &bank,
                                        with_transaction_and_index,
                                        &pool,
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
                _phantom: PhantomData::default(),
            }
        }
    }
    impl<H: ScheduledTransactionHandler<ScheduleExecutionArgForBench>>
        InstalledScheduler<ScheduleExecutionArgForBench> for NonblockingScheduler<H>
    {
        fn id(&self) -> SchedulerId {
            self.id
        }

        fn pool(&self) -> InstalledSchedulerPoolArc {
            self.pool.clone()
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

        fn context(&self) -> Option<&SchedulingContext> {
            unimplemented!("not needed for this bench");
        }

        fn replace_context(&mut self, context: SchedulingContext) {
            for _ in 0..self.lane_count {
                self.transaction_sender
                    .send(ChainedChannel::NextContext(context.clone()))
                    .unwrap();
            }
        }
    }

    #[bench]
    fn bench_with_01_thread_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithArcMutation>::spawn(pool, context, 1)
        });
    }

    #[bench]
    fn bench_with_01_thread_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithoutArcMutation>::spawn(pool, context, 1)
        });
    }

    #[bench]
    fn bench_with_04_threads_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithArcMutation>::spawn(pool, context, 4)
        });
    }

    #[bench]
    fn bench_with_04_threads_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithoutArcMutation>::spawn(pool, context, 4)
        });
    }

    #[bench]
    fn bench_with_08_threads_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithArcMutation>::spawn(pool, context, 8)
        });
    }

    #[bench]
    fn bench_with_08_threads_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithoutArcMutation>::spawn(pool, context, 8)
        });
    }

    #[bench]
    fn bench_with_16_threads_with_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithArcMutation>::spawn(pool, context, 16)
        });
    }

    #[bench]
    fn bench_with_16_threads_without_arc_mutation(bencher: &mut Bencher) {
        run_bench(bencher, |pool, context| {
            NonblockingScheduler::<BenchFriendlyHandlerWithoutArcMutation>::spawn(pool, context, 16)
        });
    }
}
