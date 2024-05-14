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
    assert_matches::assert_matches,
    crossbeam_channel::{select, unbounded, Receiver, SendError, Sender},
    derivative::Derivative,
    log::*,
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
    solana_unified_scheduler_logic::Task,
    solana_vote::vote_sender_types::ReplayVoteSender,
    std::{
        fmt::Debug,
        marker::PhantomData,
        sync::{
            atomic::{AtomicU64, Ordering::Relaxed},
            Arc, Mutex, OnceLock, Weak,
        },
        thread::{self, JoinHandle},
    },
};

type AtomicSchedulerId = AtomicU64;

// SchedulerPool must be accessed as a dyn trait from solana-runtime, because SchedulerPool
// contains some internal fields, whose types aren't available in solana-runtime (currently
// TransactionStatusSender; also, PohRecorder in the future)...
#[derive(Debug)]
pub struct SchedulerPool<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    scheduler_inners: Mutex<Vec<S::Inner>>,
    handler_count: usize,
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
        handler_count: Option<usize>,
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> Arc<Self> {
        let handler_count = handler_count.unwrap_or(1);
        // we're hard-coding the number of handler thread to 1, meaning this impl is currently
        // single-threaded still.
        assert_eq!(handler_count, 1); // replace this with assert!(handler_count >= 1) later

        Arc::new_cyclic(|weak_self| Self {
            scheduler_inners: Mutex::default(),
            handler_count,
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
        handler_count: Option<usize>,
        log_messages_bytes_limit: Option<usize>,
        transaction_status_sender: Option<TransactionStatusSender>,
        replay_vote_sender: Option<ReplayVoteSender>,
        prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    ) -> InstalledSchedulerPoolArc {
        Self::new(
            handler_count,
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

    pub fn default_handler_count() -> usize {
        Self::calculate_default_handler_count(
            thread::available_parallelism()
                .ok()
                .map(|non_zero| non_zero.get()),
        )
    }

    pub fn calculate_default_handler_count(detected_cpu_core_count: Option<usize>) -> usize {
        // Divide by 4 just not to consume all available CPUs just with handler threads, sparing for
        // other active forks and other subsystems.
        // Also, if available_parallelism fails (which should be very rare), use 4 threads,
        // as a relatively conservatism assumption of modern multi-core systems ranging from
        // engineers' laptops to production servers.
        detected_cpu_core_count
            .map(|core_count| (core_count / 4).max(1))
            .unwrap_or(4)
    }

    pub fn cli_message() -> &'static str {
        static MESSAGE: OnceLock<String> = OnceLock::new();

        MESSAGE.get_or_init(|| {
            format!(
                "Change the number of the unified scheduler's transaction execution threads \
                 dedicated to each block, otherwise calculated as cpu_cores/4 [default: {}]",
                Self::default_handler_count()
            )
        })
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

struct ExecutedTask {
    task: Task,
    result_with_timings: ResultWithTimings,
}

impl ExecutedTask {
    fn new_boxed(task: Task) -> Box<Self> {
        Box::new(Self {
            task,
            result_with_timings: initialized_result_with_timings(),
        })
    }
}

// A very tiny generic message type to signal about opening and closing of subchannels, which are
// logically segmented series of Payloads (P1) over a single continuous time-span, potentially
// carrying some subchannel metadata (P2) upon opening a new subchannel.
// Note that the above properties can be upheld only when this is used inside MPSC or SPSC channels
// (i.e. the consumer side needs to be single threaded). For the multiple consumer cases,
// ChainedChannel can be used instead.
enum SubchanneledPayload<P1, P2> {
    Payload(P1),
    OpenSubchannel(P2),
    CloseSubchannel,
}

type NewTaskPayload = SubchanneledPayload<Task, SchedulingContext>;

// A tiny generic message type to synchronize multiple threads everytime some contextual data needs
// to be switched (ie. SchedulingContext), just using a single communication channel.
//
// Usually, there's no way to prevent one of those threads from mixing current and next contexts
// while processing messages with a multiple-consumer channel. A condvar or other
// out-of-bound mechanism is needed to notify about switching of contextual data. That's because
// there's no way to block those threads reliably on such a switching event just with a channel.
//
// However, if the number of consumer can be determined, this can be accomplished just over a
// single channel, which even carries an in-bound control meta-message with the contexts. The trick
// is that identical meta-messages as many as the number of threads are sent over the channel,
// along with new channel receivers to be used (hence the name of _chained_). Then, the receiving
// thread drops the old channel and is now blocked on receiving from the new channel. In this way,
// this switching can happen exactly once for each thread.
//
// Overall, this greatly simplifies the code, reduces CAS/syscall overhead per messaging to the
// minimum at the cost of a single channel recreation per switching. Needless to say, such an
// allocation can be amortized to be negligible.
mod chained_channel {
    use super::*;

    // hide variants by putting this inside newtype
    enum ChainedChannelPrivate<P, C> {
        Payload(P),
        ContextAndChannel(C, Receiver<ChainedChannel<P, C>>),
    }

    pub(super) struct ChainedChannel<P, C>(ChainedChannelPrivate<P, C>);

    impl<P, C> ChainedChannel<P, C> {
        fn chain_to_new_channel(context: C, receiver: Receiver<Self>) -> Self {
            Self(ChainedChannelPrivate::ContextAndChannel(context, receiver))
        }
    }

    pub(super) struct ChainedChannelSender<P, C> {
        sender: Sender<ChainedChannel<P, C>>,
    }

    impl<P, C: Clone> ChainedChannelSender<P, C> {
        fn new(sender: Sender<ChainedChannel<P, C>>) -> Self {
            Self { sender }
        }

        pub(super) fn send_payload(
            &self,
            payload: P,
        ) -> std::result::Result<(), SendError<ChainedChannel<P, C>>> {
            self.sender
                .send(ChainedChannel(ChainedChannelPrivate::Payload(payload)))
        }

        pub(super) fn send_chained_channel(
            &mut self,
            context: C,
            count: usize,
        ) -> std::result::Result<(), SendError<ChainedChannel<P, C>>> {
            let (chained_sender, chained_receiver) = crossbeam_channel::unbounded();
            for _ in 0..count {
                self.sender.send(ChainedChannel::chain_to_new_channel(
                    context.clone(),
                    chained_receiver.clone(),
                ))?
            }
            self.sender = chained_sender;
            Ok(())
        }
    }

    // P doesn't need to be `: Clone`, yet rustc derive can't handle it.
    // see https://github.com/rust-lang/rust/issues/26925
    #[derive(Derivative)]
    #[derivative(Clone(bound = "C: Clone"))]
    pub(super) struct ChainedChannelReceiver<P, C: Clone> {
        receiver: Receiver<ChainedChannel<P, C>>,
        context: C,
    }

    impl<P, C: Clone> ChainedChannelReceiver<P, C> {
        fn new(receiver: Receiver<ChainedChannel<P, C>>, initial_context: C) -> Self {
            Self {
                receiver,
                context: initial_context,
            }
        }

        pub(super) fn context(&self) -> &C {
            &self.context
        }

        pub(super) fn for_select(&self) -> &Receiver<ChainedChannel<P, C>> {
            &self.receiver
        }

        pub(super) fn after_select(&mut self, message: ChainedChannel<P, C>) -> Option<P> {
            match message.0 {
                ChainedChannelPrivate::Payload(payload) => Some(payload),
                ChainedChannelPrivate::ContextAndChannel(context, channel) => {
                    self.context = context;
                    self.receiver = channel;
                    None
                }
            }
        }
    }

    pub(super) fn unbounded<P, C: Clone>(
        initial_context: C,
    ) -> (ChainedChannelSender<P, C>, ChainedChannelReceiver<P, C>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        (
            ChainedChannelSender::new(sender),
            ChainedChannelReceiver::new(receiver, initial_context),
        )
    }
}

fn initialized_result_with_timings() -> ResultWithTimings {
    (Ok(()), ExecuteTimings::default())
}

// Currently, simplest possible implementation (i.e. single-threaded)
// this will be replaced with more proper implementation...
// not usable at all, especially for mainnet-beta
#[derive(Debug)]
pub struct PooledScheduler<TH: TaskHandler> {
    inner: PooledSchedulerInner<Self, TH>,
    context: SchedulingContext,
}

#[derive(Debug)]
pub struct PooledSchedulerInner<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    thread_manager: ThreadManager<S, TH>,
}

// This type manages the OS threads for scheduling and executing transactions. The term
// `session` is consistently used to mean a group of Tasks scoped under a single SchedulingContext.
// This is equivalent to a particular bank for block verification. However, new terms is introduced
// here to mean some continuous time over multiple continuous banks/slots for the block production,
// which is planned to be implemented in the future.
#[derive(Debug)]
struct ThreadManager<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    scheduler_id: SchedulerId,
    pool: Arc<SchedulerPool<S, TH>>,
    new_task_sender: Sender<NewTaskPayload>,
    new_task_receiver: Receiver<NewTaskPayload>,
    session_result_sender: Sender<Option<ResultWithTimings>>,
    session_result_receiver: Receiver<Option<ResultWithTimings>>,
    session_result_with_timings: Option<ResultWithTimings>,
    scheduler_thread: Option<JoinHandle<()>>,
    handler_threads: Vec<JoinHandle<()>>,
}

impl<TH: TaskHandler> PooledScheduler<TH> {
    fn do_spawn(pool: Arc<SchedulerPool<Self, TH>>, initial_context: SchedulingContext) -> Self {
        Self::from_inner(
            PooledSchedulerInner::<Self, TH> {
                thread_manager: ThreadManager::new(pool),
            },
            initial_context,
        )
    }
}

impl<S: SpawnableScheduler<TH>, TH: TaskHandler> ThreadManager<S, TH> {
    fn new(pool: Arc<SchedulerPool<S, TH>>) -> Self {
        let (new_task_sender, new_task_receiver) = unbounded();
        let (session_result_sender, session_result_receiver) = unbounded();
        let handler_count = pool.handler_count;

        Self {
            scheduler_id: pool.new_scheduler_id(),
            pool,
            new_task_sender,
            new_task_receiver,
            session_result_sender,
            session_result_receiver,
            session_result_with_timings: None,
            scheduler_thread: None,
            handler_threads: Vec::with_capacity(handler_count),
        }
    }

    fn execute_task_with_handler(
        bank: &Arc<Bank>,
        executed_task: &mut Box<ExecutedTask>,
        handler_context: &HandlerContext,
    ) {
        debug!("handling task at {:?}", thread::current());
        TH::handle(
            &mut executed_task.result_with_timings.0,
            &mut executed_task.result_with_timings.1,
            bank,
            executed_task.task.transaction(),
            executed_task.task.task_index(),
            handler_context,
        );
    }

    fn accumulate_result_with_timings(
        (result, timings): &mut ResultWithTimings,
        executed_task: Box<ExecutedTask>,
    ) {
        match executed_task.result_with_timings.0 {
            Ok(()) => {}
            Err(error) => {
                error!("error is detected while accumulating....: {error:?}");
                // Override errors intentionally for simplicity, not retaining the
                // first error unlike the block verification in the
                // blockstore_processor. This will be addressed with more
                // full-fledged impl later.
                *result = Err(error);
            }
        }
        timings.accumulate(&executed_task.result_with_timings.1);
    }

    fn take_session_result_with_timings(&mut self) -> ResultWithTimings {
        self.session_result_with_timings.take().unwrap()
    }

    fn put_session_result_with_timings(&mut self, result_with_timings: ResultWithTimings) {
        assert_matches!(
            self.session_result_with_timings
                .replace(result_with_timings),
            None
        );
    }

    fn start_threads(&mut self, context: &SchedulingContext) {
        let (mut runnable_task_sender, runnable_task_receiver) =
            chained_channel::unbounded::<Task, SchedulingContext>(context.clone());
        let (finished_task_sender, finished_task_receiver) = unbounded::<Box<ExecutedTask>>();

        let mut result_with_timings = self.session_result_with_timings.take();

        // High-level flow of new tasks:
        // 1. the replay stage thread send a new task.
        // 2. the scheduler thread accepts the task.
        // 3. the scheduler thread dispatches the task after proper locking.
        // 4. the handler thread processes the dispatched task.
        // 5. the handler thread reply back to the scheduler thread as an executed task.
        // 6. the scheduler thread post-processes the executed task.
        let scheduler_main_loop = || {
            let handler_count = self.pool.handler_count;
            let session_result_sender = self.session_result_sender.clone();
            let new_task_receiver = self.new_task_receiver.clone();

            let mut session_ending = false;
            let mut active_task_count: usize = 0;

            // Now, this is the main loop for the scheduler thread, which is a special beast.
            //
            // That's because it could be the most notable bottleneck of throughput in the future
            // when there are ~100 handler threads. Unified scheduler's overall throughput is
            // largely dependant on its ultra-low latency characteristic, which is the most
            // important design goal of the scheduler in order to reduce the transaction
            // confirmation latency for end users.
            //
            // Firstly, the scheduler thread must handle incoming messages from thread(s) owned by
            // the replay stage or the banking stage. It also must handle incoming messages from
            // the multi-threaded handlers. This heavily-multi-threaded whole processing load must
            // be coped just with the single-threaded scheduler, to attain ideal cpu cache
            // friendliness and main memory bandwidth saturation with its shared-nothing
            // single-threaded account locking implementation. In other words, the per-task
            // processing efficiency of the main loop codifies the upper bound of horizontal
            // scalability of the unified scheduler.
            //
            // Moreover, the scheduler is designed to handle tasks without batching at all in the
            // pursuit of saturating all of the handler threads with maximally-fine-grained
            // concurrency density for throughput as the second design goal. This design goal
            // relies on the assumption that there's no considerable penalty arising from the
            // unbatched manner of processing.
            //
            // Note that this assumption isn't true as of writing. The current code path
            // underneath execute_batch() isn't optimized for unified scheduler's load pattern (ie.
            // batches just with a single transaction) at all. This will be addressed in the
            // future.
            //
            // These two key elements of the design philosophy lead to the rather unforgiving
            // implementation burden: Degraded performance would acutely manifest from an even tiny
            // amount of individual cpu-bound processing delay in the scheduler thread, like when
            // dispatching the next conflicting task after receiving the previous finished one from
            // the handler.
            //
            // Thus, it's fatal for unified scheduler's advertised superiority to squeeze every cpu
            // cycles out of the scheduler thread. Thus, any kinds of unessential overhead sources
            // like syscalls, VDSO, and even memory (de)allocation should be avoided at all costs
            // by design or by means of offloading at the last resort.
            move || loop {
                let mut is_finished = false;
                while !is_finished {
                    select! {
                        recv(finished_task_receiver) -> executed_task => {
                            let executed_task = executed_task.unwrap();

                            active_task_count = active_task_count.checked_sub(1).unwrap();
                            let result_with_timings = result_with_timings.as_mut().unwrap();
                            Self::accumulate_result_with_timings(result_with_timings, executed_task);
                        },
                        recv(new_task_receiver) -> message => {
                            assert!(!session_ending);

                            match message.unwrap() {
                                NewTaskPayload::Payload(task) => {
                                    // so, we're NOT scheduling at all here; rather, just execute
                                    // tx straight off. the inter-tx locking deps aren't needed to
                                    // be resolved in the case of single-threaded FIFO like this.
                                    runnable_task_sender
                                        .send_payload(task)
                                        .unwrap();
                                    active_task_count = active_task_count.checked_add(1).unwrap();
                                }
                                NewTaskPayload::OpenSubchannel(context) => {
                                    // signal about new SchedulingContext to handler threads
                                    runnable_task_sender
                                        .send_chained_channel(context, handler_count)
                                        .unwrap();
                                    assert_matches!(
                                        result_with_timings.replace(initialized_result_with_timings()),
                                        None
                                    );
                                }
                                NewTaskPayload::CloseSubchannel => {
                                    session_ending = true;
                                }
                            }
                        },
                    };

                    // a really simplistic termination condition, which only works under the
                    // assumption of single handler thread...
                    is_finished = session_ending && active_task_count == 0;
                }

                if session_ending {
                    session_result_sender
                        .send(Some(
                            result_with_timings
                                .take()
                                .unwrap_or_else(initialized_result_with_timings),
                        ))
                        .unwrap();
                    session_ending = false;
                }
            }
        };

        let handler_main_loop = || {
            let pool = self.pool.clone();
            let mut runnable_task_receiver = runnable_task_receiver.clone();
            let finished_task_sender = finished_task_sender.clone();

            move || loop {
                let (task, sender) = select! {
                    recv(runnable_task_receiver.for_select()) -> message => {
                        if let Some(task) = runnable_task_receiver.after_select(message.unwrap()) {
                            (task, &finished_task_sender)
                        } else {
                            continue;
                        }
                    },
                };
                let mut task = ExecutedTask::new_boxed(task);
                Self::execute_task_with_handler(
                    runnable_task_receiver.context().bank(),
                    &mut task,
                    &pool.handler_context,
                );
                sender.send(task).unwrap();
            }
        };

        self.scheduler_thread = Some(
            thread::Builder::new()
                .name("solScheduler".to_owned())
                .spawn(scheduler_main_loop())
                .unwrap(),
        );

        self.handler_threads = (0..self.pool.handler_count)
            .map({
                |thx| {
                    thread::Builder::new()
                        .name(format!("solScHandler{:02}", thx))
                        .spawn(handler_main_loop())
                        .unwrap()
                }
            })
            .collect();
    }

    fn send_task(&self, task: Task) {
        debug!("send_task()");
        self.new_task_sender
            .send(NewTaskPayload::Payload(task))
            .unwrap()
    }

    fn end_session(&mut self) {
        if self.session_result_with_timings.is_some() {
            debug!("end_session(): already result resides within thread manager..");
            return;
        }
        debug!("end_session(): will end session...");

        self.new_task_sender
            .send(NewTaskPayload::CloseSubchannel)
            .unwrap();

        if let Some(result_with_timings) = self.session_result_receiver.recv().unwrap() {
            self.put_session_result_with_timings(result_with_timings);
        }
    }

    fn start_session(&mut self, context: &SchedulingContext) {
        assert_matches!(self.session_result_with_timings, None);
        self.new_task_sender
            .send(NewTaskPayload::OpenSubchannel(context.clone()))
            .unwrap();
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

    fn into_inner(mut self) -> (ResultWithTimings, Self::Inner) {
        let result_with_timings = {
            let manager = &mut self.inner.thread_manager;
            manager.end_session();
            manager.take_session_result_with_timings()
        };
        (result_with_timings, self.inner)
    }

    fn from_inner(mut inner: Self::Inner, context: SchedulingContext) -> Self {
        inner.thread_manager.start_session(&context);
        Self { inner, context }
    }

    fn spawn(pool: Arc<SchedulerPool<Self, TH>>, initial_context: SchedulingContext) -> Self {
        let mut scheduler = Self::do_spawn(pool, initial_context);
        scheduler
            .inner
            .thread_manager
            .start_threads(&scheduler.context);
        scheduler
    }
}

impl<TH: TaskHandler> InstalledScheduler for PooledScheduler<TH> {
    fn id(&self) -> SchedulerId {
        self.inner.thread_manager.scheduler_id
    }

    fn context(&self) -> &SchedulingContext {
        &self.context
    }

    fn schedule_execution(&self, &(transaction, index): &(&SanitizedTransaction, usize)) {
        let task = Task::create_task(transaction.clone(), index);
        self.inner.thread_manager.send_task(task);
    }

    fn wait_for_termination(
        self: Box<Self>,
        _is_dropped: bool,
    ) -> (ResultWithTimings, UninstalledSchedulerBox) {
        let (result_with_timings, uninstalled_scheduler) = self.into_inner();
        (result_with_timings, Box::new(uninstalled_scheduler))
    }

    fn pause_for_recent_blockhash(&mut self) {
        self.inner.thread_manager.end_session();
    }
}

impl<S, TH> UninstalledScheduler for PooledSchedulerInner<S, TH>
where
    S: SpawnableScheduler<TH, Inner = PooledSchedulerInner<S, TH>>,
    TH: TaskHandler,
{
    fn return_to_pool(self: Box<Self>) {
        self.thread_manager.pool.clone().return_scheduler(*self)
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
            DefaultSchedulerPool::new_dyn(None, None, None, None, ignored_prioritization_fee_cache);

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
            DefaultSchedulerPool::new_dyn(None, None, None, None, ignored_prioritization_fee_cache);
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
        let pool =
            DefaultSchedulerPool::new(None, None, None, None, ignored_prioritization_fee_cache);
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
        let pool =
            DefaultSchedulerPool::new(None, None, None, None, ignored_prioritization_fee_cache);
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
        let pool =
            DefaultSchedulerPool::new(None, None, None, None, ignored_prioritization_fee_cache);
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
            DefaultSchedulerPool::new_dyn(None, None, None, None, ignored_prioritization_fee_cache);
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
            DefaultSchedulerPool::new_dyn(None, None, None, None, ignored_prioritization_fee_cache);

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
        bank.set_fork_graph_in_program_cache(bank_fork);
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
            DefaultSchedulerPool::new_dyn(None, None, None, None, ignored_prioritization_fee_cache);
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
            DefaultSchedulerPool::new_dyn(None, None, None, None, ignored_prioritization_fee_cache);
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
        // simulate the task-sending thread is stalled for some reason.
        std::thread::sleep(std::time::Duration::from_secs(1));
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
        // That's because we're testing the serialized failing execution case in this test.
        // However, currently threaded impl can't properly abort in this situtation..
        // so, 1 should be observed, intead of 0.
        // Also note that bank.transaction_count() is generally racy by nature, because
        // blockstore_processor and unified_scheduler both tend to process non-conflicting batches
        // in parallel as part of the normal operation.
        assert_eq!(bank.transaction_count(), 1);

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
        Mutex<ResultWithTimings>,
        Mutex<Vec<JoinHandle<ResultWithTimings>>>,
        SchedulingContext,
        Arc<SchedulerPool<Self, DefaultTaskHandler>>,
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
            *self.0.lock().unwrap() = (overall_result, overall_timings);
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool> InstalledScheduler
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn id(&self) -> SchedulerId {
            unimplemented!();
        }

        fn context(&self) -> &SchedulingContext {
            &self.2
        }

        fn schedule_execution(&self, &(transaction, index): &(&SanitizedTransaction, usize)) {
            let transaction_and_index = (transaction.clone(), index);
            let context = self.context().clone();
            let pool = self.3.clone();

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
            _is_dropped: bool,
        ) -> (ResultWithTimings, UninstalledSchedulerBox) {
            self.do_wait();
            let result_with_timings = std::mem::replace(
                &mut *self.0.lock().unwrap(),
                initialized_result_with_timings(),
            );
            (result_with_timings, self)
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

    impl<const TRIGGER_RACE_CONDITION: bool> UninstalledScheduler
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        fn return_to_pool(self: Box<Self>) {
            self.3.clone().return_scheduler(*self)
        }
    }

    impl<const TRIGGER_RACE_CONDITION: bool> SpawnableScheduler<DefaultTaskHandler>
        for AsyncScheduler<TRIGGER_RACE_CONDITION>
    {
        // well, i wish i can use ! (never type).....
        type Inner = Self;

        fn into_inner(self) -> (ResultWithTimings, Self::Inner) {
            unimplemented!();
        }

        fn from_inner(_inner: Self::Inner, _context: SchedulingContext) -> Self {
            unimplemented!();
        }

        fn spawn(
            pool: Arc<SchedulerPool<Self, DefaultTaskHandler>>,
            initial_context: SchedulingContext,
        ) -> Self {
            AsyncScheduler::<TRIGGER_RACE_CONDITION>(
                Mutex::new(initialized_result_with_timings()),
                Mutex::new(vec![]),
                initial_context,
                pool,
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

    #[test]
    fn test_default_handler_count() {
        for (detected, expected) in [(32, 8), (4, 1), (2, 1)] {
            assert_eq!(
                DefaultSchedulerPool::calculate_default_handler_count(Some(detected)),
                expected
            );
        }
        assert_eq!(
            DefaultSchedulerPool::calculate_default_handler_count(None),
            4
        );
    }
}
