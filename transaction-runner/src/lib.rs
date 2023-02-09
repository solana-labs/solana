use {
    log::*,
};
use std::time::Duration;
use solana_runtime::bank::TransactionResults;
use std::sync::Arc;
use solana_runtime::transaction_batch::TransactionBatch;
use solana_runtime::bank::Bank;
use solana_runtime::bank::LoadAndExecuteTransactionsOutput;
use solana_runtime::bank::CommitTransactionCounts;
use std::borrow::Cow;
use solana_runtime::bank::LikeScheduler;
use solana_runtime::bank_forks::LikeSchedulerPool;
use solana_runtime::bank::SchedulerContext;
use std::sync::atomic::AtomicBool;
use solana_sdk::transaction::SanitizedTransaction;
use solana_sdk::transaction::Result;
use solana_program_runtime::timings::ExecuteTimings;
use solana_runtime::transaction_priority_details::GetTransactionPriorityDetails;
use std::collections::HashMap;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::clock::MAX_PROCESSING_AGE;
use solana_sdk::transaction::TransactionError;
use std::time::Instant;
use solana_sdk::transaction::VersionedTransaction;
use solana_metrics::inc_new_counter_info;
use solana_metrics::inc_new_counter;
use solana_metrics::inc_counter;
use solana_metrics::datapoint_info_at;
use solana_metrics::create_counter;
use solana_measure::measure::Measure;

struct TransactionRunner(Arc<Bank>, solana_poh::poh_recorder::PohRecorder, solana_ledger::blockstore_processor::TransactionStatusSender);


#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: std::sync::Mutex<Vec<Box<dyn LikeScheduler>>>,
}

impl SchedulerPool {
    pub const fn new() -> Self {
        Self {
            schedulers: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn create(self: &Arc<Self>) {
        self.schedulers.lock().unwrap().push(Box::new(Scheduler::default2(self.clone())));
    }

    pub fn take_from_pool(self: &Arc<Self>) -> Box<dyn LikeScheduler> {
        if let Some(scheduler) = self.schedulers.lock().unwrap().pop() {
            trace!(
                "SchedulerPool: id_{:016x} is taken... len: {} => {}",
                scheduler.random_id(),
                self.schedulers.lock().unwrap().len() + 1,
                self.schedulers.lock().unwrap().len()
            );
            scheduler
        } else {
            self.create();
            self.take_from_pool()
        }
    }

    pub fn return_to_pool(&mut self, scheduler: Box<dyn LikeScheduler>) {
        trace!(
            "SchedulerPool: id_{:016x} is returned... len: {} => {}",
            scheduler.random_id(),
            self.schedulers.lock().unwrap().len(),
            self.schedulers.lock().unwrap().len() + 1
        );
        assert!(scheduler.collected_results().lock().unwrap().is_empty());
        //assert!(scheduler.current_checkpoint.clone_context_value().unwrap().bank.is_none());
        assert!(scheduler
            .graceful_stop_initiated()
            .load(std::sync::atomic::Ordering::SeqCst));

        scheduler
            .graceful_stop_initiated()
            .store(false, std::sync::atomic::Ordering::SeqCst);

        self.schedulers.lock().unwrap().push(scheduler);
    }
}

impl Drop for SchedulerPool {
    fn drop(&mut self) {
        let current_thread_name = std::thread::current().name().unwrap().to_string();
        warn!("SchedulerPool::drop() by {}...", current_thread_name);
        todo!();
        //info!("Scheduler::drop(): id_{:016x} begin..", self.random_id);
        //self.gracefully_stop().unwrap();
        //info!("Scheduler::drop(): id_{:016x} end...", self.random_id);
    }
}

struct A(SchedulerPool);

impl LikeSchedulerPool for A(Arc<SchedulerPool>) {
    fn take_from_pool(&self, mode: solana_scheduler::Mode) -> Box<dyn LikeScheduler> {
        panic!();
    }

    fn return_to_pool(&self, scheduler: Box<dyn LikeScheduler>) {
        panic!();
    }
}

pub static POH: std::sync::RwLock<Option<Box<dyn Fn(&Bank, Vec<VersionedTransaction>, solana_sdk::hash::Hash) -> std::result::Result<Option<usize>, ()> + Send + Sync>>> = std::sync::RwLock::new(None);
use solana_transaction_status::TransactionTokenBalance;
pub static STATUS_SENDER_CALLBACK: std::sync::RwLock<Option<(Option<usize>, Box<dyn Fn(Option<(Vec<Vec<u64>>, Vec<Vec<TransactionTokenBalance>>)>, &Arc<Bank>, &TransactionBatch, &mut HashMap<Pubkey, u8>, Option<TransactionResults>, Option<usize>) -> std::option::Option<(Vec<Vec<u64>>, Vec<Vec<TransactionTokenBalance>>)> + Send + Sync>)>> = std::sync::RwLock::new(None);

#[derive(Debug)]
pub(crate) struct Scheduler {
    random_id: u64,
    scheduler_thread_handle: Option<std::thread::JoinHandle<Result<(Duration, Duration)>>>,
    executing_thread_handles: Option<Vec<std::thread::JoinHandle<Result<(Duration, Duration)>>>>,
    error_collector_thread_handle: Option<std::thread::JoinHandle<Result<(Duration, Duration)>>>,
    transaction_sender: Option<crossbeam_channel::Sender<solana_scheduler::SchedulablePayload<ExecuteTimings, SchedulerContext>>>,
    preloader: Arc<solana_scheduler::Preloader>,
    graceful_stop_initiated: AtomicBool,
    collected_results: Arc<std::sync::Mutex<Vec<Result<ExecuteTimings>>>>,
    commit_status: Arc<CommitStatus>,
    current_checkpoint: Arc<solana_scheduler::Checkpoint<ExecuteTimings, SchedulerContext>>,
    thread_count: usize,
    scheduler_pool: Arc<SchedulerPool>, // use Weak to cut circuric dep.
}

impl Scheduler {
    fn schedule(&self, sanitized_tx: &SanitizedTransaction, index: usize, mode: solana_scheduler::Mode) {
        trace!("Scheduler::schedule()");
        #[derive(Clone, Copy, Debug)]
        struct NotAtTopOfScheduleThread;
        unsafe impl solana_scheduler::NotAtScheduleThread for NotAtTopOfScheduleThread {}
        let nast = NotAtTopOfScheduleThread;

        let locks = sanitized_tx.get_account_locks_unchecked();
        let writable_lock_iter = locks.writable.iter().map(|address| {
            solana_scheduler::LockAttempt::new(
                self.preloader.load(**address),
                solana_scheduler::RequestedUsage::Writable,
            )
        });
        let readonly_lock_iter = locks.readonly.iter().map(|address| {
            solana_scheduler::LockAttempt::new(
                self.preloader.load(**address),
                solana_scheduler::RequestedUsage::Readonly,
            )
        });
        let locks = writable_lock_iter
            .chain(readonly_lock_iter)
            .collect::<Vec<_>>();

        //assert_eq!(index, self.transaction_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
        use solana_scheduler::{Mode, UniqueWeight};
        use solana_runtime::transaction_priority_details::GetTransactionPriorityDetails;
        let uw = match mode {
            Mode::Banking => ((sanitized_tx.get_transaction_priority_details().map(|d| d.priority).unwrap_or_default() as UniqueWeight) << 64) | ((usize::max_value() - index) as UniqueWeight),
            Mode::Replaying => solana_scheduler::UniqueWeight::max_value() - index as solana_scheduler::UniqueWeight,
        };
        let t =
            solana_scheduler::Task::new_for_queue(nast, uw, (sanitized_tx.clone(), locks));
        self.transaction_sender
            .as_ref()
            .unwrap()
            .send(solana_scheduler::SchedulablePayload(
                solana_scheduler::Flushable::Payload(t),
            ))
            .unwrap();
    }
}

#[derive(Debug)]
struct CommitStatus {
    is_paused: std::sync::Mutex<bool>, // maybe should use blockheight: u64 to avoid race for races between replay and executor's poh error?
    condvar: std::sync::Condvar,
}

impl CommitStatus {
    fn new() -> Self {
        Self {
            is_paused: Default::default(),
            condvar: Default::default(),
        }
    }

    fn check_and_wait(&self) {
        let mut is_paused = self.is_paused.lock().unwrap();
        if !*is_paused {
            return
        }
        let current_thread_name = std::thread::current().name().unwrap().to_string();

        info!("CommitStatus: {current_thread_name} is paused...");
        self.condvar.wait_while(is_paused, |now_is_paused| *now_is_paused).unwrap();
        info!("CommitStatus: {current_thread_name} is resumed...");
    }

    fn notify_as_paused(&self) {
        let current_thread_name = std::thread::current().name().unwrap().to_string();
        let mut is_paused = self.is_paused.lock().unwrap();
        if *is_paused {
            info!("CommitStatus: {current_thread_name} is skipped to notify as paused...");
        } else {
            info!("CommitStatus: {current_thread_name} is notifying as paused...");
            *is_paused = true;
        }
    }

    fn notify_as_resumed(&self) {
        let current_thread_name = std::thread::current().name().unwrap().to_string();
        let mut is_paused = self.is_paused.lock().unwrap();
        if *is_paused {
            info!("CommitStatus: {current_thread_name} is notifying as resumed...");
            *is_paused = false;
            self.condvar.notify_all();
        }
    }
}

impl Scheduler {
    fn default2(scheduler_pool: Arc<SchedulerPool>) -> Self {
        let start = Instant::now();
        let mut address_book = solana_scheduler::AddressBook::default();
        let preloader = Arc::new(address_book.preloader());
        let (transaction_sender, transaction_receiver) = crossbeam_channel::unbounded();
        let (scheduled_ee_sender, scheduled_ee_receiver) = crossbeam_channel::unbounded();
        let (scheduled_high_ee_sender, scheduled_high_ee_receiver) = crossbeam_channel::unbounded();
        let (processed_ee_sender, processed_ee_receiver) = crossbeam_channel::unbounded();
        let (retired_ee_sender, retired_ee_receiver) = crossbeam_channel::unbounded();


        let executing_thread_count = std::env::var("EXECUTING_THREAD_COUNT")
            .unwrap_or(format!("{}", 8))
            .parse::<usize>()
            .unwrap();
        let base_thread_count = executing_thread_count / 2;
        let thread_count = 3 + executing_thread_count;
        let initial_checkpoint = Self::new_checkpoint(thread_count);

        let send_metrics = std::env::var("SOLANA_TRANSACTION_TIMINGS").is_ok();

        let max_thread_priority = std::env::var("MAX_THREAD_PRIORITY").is_ok();
        let commit_status = Arc::new(CommitStatus::new());

        use rand::Rng;
        let random_id = rand::thread_rng().gen::<u64>();

        let executing_thread_count = std::cmp::max(base_thread_count * 2, 1);
        let executing_thread_handles = (0..executing_thread_count).map(|thx| {
            let (scheduled_ee_receiver, scheduled_high_ee_receiver, processed_ee_sender) = (scheduled_ee_receiver.clone(), scheduled_high_ee_receiver.clone(), processed_ee_sender.clone());
            let initial_checkpoint = initial_checkpoint.clone();
            let commit_status = commit_status.clone();
            let scheduler_pool = scheduler_pool.clone();

            std::thread::Builder::new().name(format!("solScExLane{:02}", thx)).spawn(move || {
            let mut mint_decimals: HashMap<Pubkey, u8> = HashMap::new();

            let started = (cpu_time::ThreadTime::now(), std::time::Instant::now());
            if max_thread_priority {
                thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max).unwrap();
            }
            let (mut latest_checkpoint, mut latest_scheduler_context) = (Some(initial_checkpoint), None::<SchedulerContext>);

            'recv: while let Ok(r) = (if thx >= base_thread_count { scheduled_high_ee_receiver.recv() } else { scheduled_ee_receiver.recv()}) {
                match r {
                solana_scheduler::ExecutablePayload(solana_scheduler::Flushable::Payload(mut ee)) => {

                'retry: loop {
                commit_status.check_and_wait();
                if let Some(latest_checkpoint) = latest_checkpoint.take() {
                    latest_scheduler_context = latest_checkpoint.clone_context_value();
                }

                let (mut wall_time, cpu_time) = (Measure::start("process_message_time"), cpu_time::ThreadTime::now());

                let current_execute_clock = ee.task.execute_time();
                let transaction_index = ee.task.transaction_index(latest_scheduler_context.as_ref().unwrap().mode);
                trace!("execute_substage: transaction_index: {} execute_clock: {} at thread: {}", thx, transaction_index, current_execute_clock);

                let mut timings = Default::default();
                let Some(bank) = latest_scheduler_context.as_ref().unwrap().bank() else {
                    warn!("ODD");
                    processed_ee_sender.send(solana_scheduler::UnlockablePayload(ee, timings)).unwrap();
                    continue 'recv;
                };
                let slot = bank.slot();

                let tx_account_lock_limit = bank.get_transaction_account_lock_limit();
                let lock_result = ee.task.tx.0
                    .get_account_locks(tx_account_lock_limit)
                    .map(|_| ());
                let mut batch =
                    TransactionBatch::new(vec![lock_result], &bank, Cow::Owned(vec![ee.task.tx.0.clone()]));
                batch.set_needs_unlock(false);
                let status_sender_callback = STATUS_SENDER_CALLBACK.read().unwrap();
                let bb = status_sender_callback.as_ref().map(|(_, status_sender_callback)|
                    status_sender_callback(None, bank, &batch, &mut mint_decimals, None, None)
                );

                let LoadAndExecuteTransactionsOutput {
                    mut loaded_transactions,
                    mut execution_results,
                    mut executed_transactions_count,
                    executed_non_vote_transactions_count,
                    executed_with_successful_result_count,
                    mut signature_count,
                    ..
                } = bank.load_and_execute_transactions(
                    &batch,
                    MAX_PROCESSING_AGE,
                    bb.is_some(),
                    bb.is_some(),
                    bb.is_some(),
                    &mut timings,
                    None,
                    status_sender_callback.as_ref().map(|a| a.0).flatten(),
                );

                let (last_blockhash, lamports_per_signature) =
                    bank.last_blockhash_and_lamports_per_signature();

                let mode = latest_scheduler_context.as_ref().unwrap().mode;
                let commited_first_transaction_index = match mode {
                    solana_scheduler::Mode::Replaying => {
                        //info!("replaying commit! {slot}");
                        Some(ee.task.transaction_index(mode) as usize)
                   },
                    solana_scheduler::Mode::Banking => {
                        //info!("banking commit! {slot}");
                        let executed_transactions: Vec<_> = execution_results
                                .iter()
                                .zip(batch.sanitized_transactions())
                                .filter_map(|(execution_result, tx)| {
                                    if execution_result.was_executed() {
                                        Some(tx.to_versioned_transaction())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                        if !executed_transactions.is_empty() {
                            let hash = solana_entry::entry::hash_transactions(&executed_transactions);
                            let poh = POH.read().unwrap();
                            let res = poh.as_ref().unwrap()(bank.as_ref(), executed_transactions, hash);
                            match res {
                                Ok(aa) => aa,
                                Err(e) => {
                                    let current_thread_name = std::thread::current().name().unwrap().to_string();

                                    trace!("{current_thread_name} pausing due to poh error until resumed...: {:?}", e);
                                    // this is needed so that we don't enter busy loop
                                    commit_status.notify_as_paused();
                                    // meddle with checkpoint/context
                                    continue 'retry;
                                },
                            }
                        } else {
                            None
                        }
                    },
                };

                let tx_results = bank.commit_transactions(
                    batch.sanitized_transactions(),
                    &mut loaded_transactions,
                    execution_results,
                    last_blockhash,
                    lamports_per_signature,
                    CommitTransactionCounts {
                        committed_transactions_count: executed_transactions_count as u64,
                        committed_with_failure_result_count: executed_transactions_count
                            .saturating_sub(executed_with_successful_result_count)
                            as u64,
                        committed_non_vote_transactions_count: executed_non_vote_transactions_count as u64,
                        signature_count,
                    },
                    &mut timings,
                );

                let TransactionResults {
                    fee_collection_results,
                    execution_results,
                    ..
                } = &tx_results;

                let tx_result = fee_collection_results.clone().into_iter().collect::<Result<_>>();
                if tx_result.is_ok() {
                    let details = execution_results[0].details().unwrap();
                    ee.cu = details.executed_units;
                } else {
                    let sig = || ee.task.tx.0.signature().to_string();
                    match mode {
                        solana_scheduler::Mode::Replaying => {
                            error!("found odd tx error: slot: {}, signature: {}, {:?}", slot, sig(), tx_result);
                        },
                        solana_scheduler::Mode::Banking => {
                            trace!("found odd tx error: slot: {}, signature: {}, {:?}", slot, sig(), tx_result);
                        }
                    }
                };

                ee.execution_result = Some(tx_result);
                ee.finish_time = Some(std::time::SystemTime::now());
                ee.thx = thx;
                ee.execution_cpu_us = cpu_time.elapsed().as_micros();
                // make wall time is longer than cpu time, always
                wall_time.stop();
                ee.execution_us = wall_time.as_us();

                if let Some(commited_first_transaction_index) = commited_first_transaction_index {
                    if let Some(bb) = bb {
                        assert!(status_sender_callback.as_ref().unwrap().1(bb, bank, &batch, &mut mint_decimals, Some(tx_results), Some(commited_first_transaction_index)).is_none());
                    }
                }

                drop(batch);

                //ee.reindex_with_address_book();
                processed_ee_sender.send(solana_scheduler::UnlockablePayload(ee, timings)).unwrap();
                break;
                }
                },
                solana_scheduler::ExecutablePayload(solana_scheduler::Flushable::Flush(checkpoint)) => {
                    checkpoint.wait_for_restart(None);
                    latest_checkpoint = Some(checkpoint);
                    latest_scheduler_context = None;
                }
                }
            }
            todo!();

            Ok((started.0.elapsed(), started.1.elapsed()))
        }).unwrap()}).collect();

        let collected_results = Arc::new(std::sync::Mutex::new(Vec::new()));
        let collected_results_in_collector_thread = Arc::clone(&collected_results);

        let error_collector_thread_handle = std::thread::Builder::new()
            .name(format!("solScErrCol{:02}", 0))
            .spawn({
                let initial_checkpoint = initial_checkpoint.clone();

                move || {
                let started = (cpu_time::ThreadTime::now(), std::time::Instant::now());
                if max_thread_priority {
                    thread_priority::set_current_thread_priority(
                        thread_priority::ThreadPriority::Max,
                    )
                    .unwrap();
                }

                let mut cumulative_timings = ExecuteTimings::default();
                use variant_counter::VariantCount;
                let mut transaction_error_counts = TransactionError::counter();
                let (mut skipped, mut succeeded) = (0, 0);
                let (mut latest_checkpoint, mut latest_scheduler_context) = (Some(initial_checkpoint), None::<SchedulerContext>);

                loop {
                while let Ok(r) = retired_ee_receiver.recv_timeout(std::time::Duration::from_millis(20))
                {
                    use solana_runtime::transaction_priority_details::GetTransactionPriorityDetails;
                    if let Some(latest_checkpoint) = latest_checkpoint.take() {
                        latest_scheduler_context = latest_checkpoint.clone_context_value();
                    }

                    match r {
                        solana_scheduler::ExaminablePayload(solana_scheduler::Flushable::Payload((mut ee, timings))) => {
                            cumulative_timings.accumulate(&timings);

                            if send_metrics && ee.finish_time.is_some() {
                                let sig = ee.task.tx.0.signature().to_string();

                                datapoint_info_at!(
                                    ee.finish_time.unwrap(),
                                    "transaction_timings",
                                    ("slot", latest_scheduler_context.as_ref().unwrap().slot(), i64),
                                    ("index", ee.task.transaction_index(latest_scheduler_context.as_ref().unwrap().mode), i64),
                                    ("thread", format!("solScExLane{:02}", ee.thx), String),
                                    ("signature", &sig, String),
                                    ("account_locks_in_json", serde_json::to_string(&ee.task.tx.0.get_account_locks_unchecked()).unwrap(), String),
                                    (
                                        "status",
                                        format!("{:?}", ee.execution_result.as_ref().unwrap()),
                                        String
                                    ),
                                    ("duration", ee.execution_us, i64),
                                    ("cpu_duration", ee.execution_cpu_us, i64),
                                    ("compute_units", ee.cu, i64),
                                    ("priority", ee.task.tx.0.get_transaction_priority_details().map(|d| d.priority).unwrap_or_default(), i64),
                                );
                                info!("execute_substage: slot: {} transaction_index: {} timings: {:?}", latest_scheduler_context.as_ref().unwrap().slot(), ee.task.transaction_index(latest_scheduler_context.as_ref().unwrap().mode), timings);
                            }

                            if let Some(result) = ee.execution_result.take() {
                                match result {
                                    Ok(_) => {
                                        succeeded += 1;
                                        inc_new_counter_info!("bank-process_transactions", 1);
                                        inc_new_counter_info!(
                                            "bank-process_transactions-txs",
                                            1 as usize
                                        );
                                        inc_new_counter_info!("bank-process_transactions-sigs", ee.task.tx.0.signatures().len() as usize);
                                    },
                                    Err(e) => {
                                        transaction_error_counts.record(&e);
                                        match latest_scheduler_context.as_ref().unwrap().mode {
                                            solana_scheduler::Mode::Replaying => {
                                                error!(
                                                    "scheduler: Unexpected validator error: {:?}, transaction: {:?}",
                                                    e, ee.task.tx.0
                                                );
                                            }
                                            solana_scheduler::Mode::Banking => {
                                                trace!(
                                                    "scheduler: Unexpected validator error: {:?}, transaction: {:?}",
                                                    e, ee.task.tx.0
                                                );
                                            }
                                        };
                                        collected_results_in_collector_thread
                                            .lock()
                                            .unwrap()
                                            .push(Err(e));
                                    }
                                }
                            } else {
                                skipped += 1;
                            }
                            drop(ee);
                        },
                        solana_scheduler::ExaminablePayload(solana_scheduler::Flushable::Flush(checkpoint)) => {
                            info!("post_execution_handler: slot: {:?} {:?}", SchedulerContext::log_prefix(random_id, &latest_scheduler_context), transaction_error_counts.aggregate().into_iter().chain([("succeeded", succeeded), ("skipped", skipped)].into_iter()).filter(|&(k, v)| v > 0).collect::<std::collections::BTreeMap<_, _>>());
                            if let Some(solana_scheduler::Mode::Replaying) = latest_scheduler_context.as_ref().map(|c| c.mode) {
                                assert_eq!(skipped, 0);
                            }
                            transaction_error_counts.reset();
                            (succeeded, skipped) = (0, 0);
                            checkpoint.wait_for_restart(Some(std::mem::take(&mut cumulative_timings)));
                            latest_checkpoint = Some(checkpoint);
                            latest_scheduler_context = None;
                        },
                    }
                }
                }
                todo!();

                Ok((started.0.elapsed(), started.1.elapsed()))
            }})
            .unwrap();


        let scheduler_thread_handle = std::thread::Builder::new()
            .name("solScheduler".to_string())
            .spawn({
                let initial_checkpoint = initial_checkpoint.clone();

                move || {
                let started = (cpu_time::ThreadTime::now(), std::time::Instant::now());
                if max_thread_priority {
                    thread_priority::set_current_thread_priority(
                        thread_priority::ThreadPriority::Max,
                    )
                    .unwrap();
                }

                let mut latest_checkpoint = Some(initial_checkpoint);

                loop {
                    let mut runnable_queue = solana_scheduler::TaskQueue::default();
                    let maybe_checkpoint = solana_scheduler::ScheduleStage::run(
                        &mut latest_checkpoint,
                        executing_thread_count,
                        &mut runnable_queue,
                        &mut address_book,
                        &transaction_receiver,
                        &scheduled_ee_sender,
                        Some(&scheduled_high_ee_sender),
                        &processed_ee_receiver,
                        Some(&retired_ee_sender),
                        |scheduler_context| SchedulerContext::log_prefix(random_id, scheduler_context),
                    );

                    if let Some(checkpoint) = maybe_checkpoint {
                        checkpoint.wait_for_restart(None);
                        latest_checkpoint = Some(checkpoint);
                        continue;
                    } else {
                        break;
                    }
                }

                drop(transaction_receiver);
                drop(scheduled_ee_sender);
                drop(scheduled_high_ee_sender);
                drop(processed_ee_receiver);

                todo!();
                Ok((started.0.elapsed(), started.1.elapsed()))
            }})
            .unwrap();

        let s = Self {
            random_id,
            scheduler_thread_handle: Some(scheduler_thread_handle),
            executing_thread_handles: Some(executing_thread_handles),
            error_collector_thread_handle: Some(error_collector_thread_handle),
            transaction_sender: Some(transaction_sender),
            preloader,
            graceful_stop_initiated: Default::default(),
            collected_results,
            commit_status,
            current_checkpoint: initial_checkpoint,
            thread_count,
            scheduler_pool,
        };
        info!(
            "scheduler: id_{:016x} setup done with {}us",
            random_id,
            start.elapsed().as_micros()
        );

        s
    }
}

impl Scheduler {
    fn new_checkpoint(thread_count: usize) -> Arc<solana_scheduler::Checkpoint<ExecuteTimings, SchedulerContext>> {
        solana_scheduler::Checkpoint::new(thread_count)
    }

    fn checkpoint(&self) -> Arc<solana_scheduler::Checkpoint<ExecuteTimings, SchedulerContext>> {
        Self::new_checkpoint(self.thread_count)
    }

    fn gracefully_stop(&mut self) -> Result<()> {
        if self
            .graceful_stop_initiated
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            warn!(
                "Scheduler::gracefully_stop(): id_{:016x} (skipped..?)",
                self.random_id
            );
            return Ok(());
        }
        self.graceful_stop_initiated
            .store(true, std::sync::atomic::Ordering::SeqCst);

        trace!(
            "Scheduler::gracefully_stop(): id_{:016x} waiting..",
            self.random_id
        );
        //let transaction_sender = self.transaction_sender.take().unwrap();

        //drop(transaction_sender);
        let checkpoint = self.checkpoint();
        self.transaction_sender
            .as_ref()
            .unwrap()
            .send(solana_scheduler::SchedulablePayload(
                solana_scheduler::Flushable::Flush(std::sync::Arc::clone(&checkpoint)),
            ))
            .unwrap();
        checkpoint.wait_for_restart(None);
        let r = checkpoint.take_restart_value();
        self.current_checkpoint = checkpoint;
        self.current_checkpoint.update_context_value(|c| {c.bank= None;});
        self.collected_results.lock().unwrap().push(Ok(r));

        /*
        let executing_thread_duration_pairs: Result<Vec<_>> = self.executing_thread_handles.take().unwrap().into_iter().map(|executing_thread_handle| {
            executing_thread_handle.join().unwrap().map(|u| (u.0.as_micros(), u.1.as_micros()))
        }).collect();
        let mut executing_thread_duration_pairs = executing_thread_duration_pairs?;
        executing_thread_duration_pairs.sort();
        let (executing_thread_cpu_us, executing_thread_wall_time_us): (Vec<_>, Vec<_>) = executing_thread_duration_pairs.into_iter().unzip();

        let h = self.scheduler_thread_handle.take().unwrap();
        let scheduler_thread_duration_pairs = h.join().unwrap()?;
        let (scheduler_thread_cpu_us, scheduler_thread_wall_time_us) = (scheduler_thread_duration_pairs.0.as_micros(), scheduler_thread_duration_pairs.1.as_micros());
        let h = self.error_collector_thread_handle.take().unwrap();
        let error_collector_thread_duration_pairs = h.join().unwrap()?;
        let (error_collector_thread_cpu_us, error_collector_thread_wall_time_us) = (error_collector_thread_duration_pairs.0.as_micros(), error_collector_thread_duration_pairs.1.as_micros());

        info!("Scheduler::gracefully_stop(): slot: {} id_{:016x} durations 1/2 (cpu ): scheduler: {}us, error_collector: {}us, lanes: {}us = {:?}", self.slot.map(|s| format!("{}", s)).unwrap_or("-".into()), self.random_id, scheduler_thread_cpu_us, error_collector_thread_cpu_us, executing_thread_cpu_us.iter().sum::<u128>(), &executing_thread_cpu_us);
        info!("Scheduler::gracefully_stop(): slot: {} id_{:016x} durations 2/2 (wall): scheduler: {}us, error_collector: {}us, lanes: {}us = {:?}", self.slot.map(|s| format!("{}", s)).unwrap_or("-".into()), self.random_id, scheduler_thread_wall_time_us, error_collector_thread_wall_time_us, executing_thread_wall_time_us.iter().sum::<u128>(), &executing_thread_wall_time_us);
        */

        Ok(())
    }

    fn handle_aborted_executions(&self) -> Vec<Result<ExecuteTimings>> {
        std::mem::take(&mut self.collected_results.lock().unwrap())
    }

    fn pause_commit_into_bank(&self) {
        self.commit_status.notify_as_paused();
        self.current_checkpoint.update_context_value(|c| {c.bank = None;});
    }

    fn resume_commit_into_bank(&self, bank: Option<&Arc<Bank>>) {
        self.current_checkpoint.update_context_value(|c| {c.bank = bank.cloned();});
        self.commit_status.notify_as_resumed();
    }

    fn replace_scheduler_context(&self, scheduler_context: SchedulerContext) {
        self.current_checkpoint.replace_context_value(scheduler_context);
    }

    fn current_scheduler_mode(&self) -> solana_scheduler::Mode {
        self.current_checkpoint.with_context_value(|c| c.mode).unwrap()
    }

    fn has_context(&self) -> bool {
        self.current_checkpoint.with_context_value(|_| ()).is_some()
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        let current_thread_name = std::thread::current().name().unwrap().to_string();
        warn!("Scheduler::drop() by {}...", current_thread_name);
        todo!();
        //info!("Scheduler::drop(): id_{:016x} begin..", self.random_id);
        //self.gracefully_stop().unwrap();
        //info!("Scheduler::drop(): id_{:016x} end...", self.random_id);
    }
}


impl LikeScheduler for Scheduler {
    fn graceful_stop_initiated(&self) -> &AtomicBool {
        panic!();
    }

    fn random_id(&self) -> u64 {
        panic!();
    }

    fn schedule(&self, sanitized_tx: &SanitizedTransaction, index: usize, mode: solana_scheduler::Mode) {
        panic!();
    }

    fn handle_aborted_executions(&self) -> Vec<Result<ExecuteTimings>> {
        panic!();
    }

    fn pause_commit_into_bank(&self) {
        panic!();
    }

    fn resume_commit_into_bank(&self, bank: Option<&Arc<Bank>>) {
        panic!();
    }

    fn gracefully_stop(&mut self) -> Result<()> {
        panic!();
    }

    fn current_scheduler_mode(&self) -> solana_scheduler::Mode {
        panic!();
    }

    fn has_context(&self) -> bool {
        panic!();
    }

    fn collected_results(&self) -> Arc<std::sync::Mutex<Vec<Result<ExecuteTimings>>>> {
        panic!();
    }

    fn return_to_pool(&self) {
        panic!();
    }

    fn take_next_from_pool(&self) -> Box<dyn LikeScheduler> {
        panic!();
    }
}
