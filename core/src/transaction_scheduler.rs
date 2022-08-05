//! Implements a transaction scheduler

use {
    crate::{
        transaction_priority_details::GetTransactionPriorityDetails,
        unprocessed_packet_batches::{self, ImmutableDeserializedPacket},
    },
    crossbeam_channel::{Receiver, Sender, TryRecvError},
    dashmap::DashMap,
    solana_measure::{measure, measure::Measure},
    solana_perf::packet::PacketBatch,
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, TransactionError},
    },
    std::{
        collections::{BinaryHeap, HashMap, HashSet, VecDeque},
        hash::Hash,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex, RwLock,
        },
        thread::JoinHandle,
        time::Instant,
    },
};
/// Wrapper to store a sanitized transaction and priority
#[derive(Debug)]
pub struct TransactionPriority {
    /// Transaction priority
    pub priority: u64,
    /// Sanitized transaction
    pub transaction: Box<SanitizedTransaction>,
    /// Timestamp the scheduler received the transaction - only used for ordering
    pub timestamp: Instant,
}

impl Ord for TransactionPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.priority.cmp(&other.priority) {
            std::cmp::Ordering::Equal => match self
                .transaction
                .message_hash()
                .cmp(other.transaction.message_hash())
            {
                std::cmp::Ordering::Equal => self.timestamp.cmp(&other.timestamp),
                ordering => ordering,
            },
            ordering => ordering,
        }
    }
}

impl PartialOrd for TransactionPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TransactionPriority {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
            && self.transaction.message_hash() == other.transaction.message_hash()
            && self.timestamp == other.timestamp
    }
}

impl Eq for TransactionPriority {}

impl Hash for TransactionPriority {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.transaction.signature().hash(state);
        self.timestamp.hash(state);
        self.priority.hash(state);
    }
}

type TransactionRef = TransactionPriority;

impl TransactionPriority {
    fn new(transaction: SanitizedTransaction, num_conflicts: u64) -> TransactionRef {
        let packet_priority = transaction
            .get_transaction_priority_details()
            .unwrap()
            .priority;
        let priority = Self::modify_priority(packet_priority, num_conflicts);

        Self {
            transaction: Box::new(transaction),
            priority,
            timestamp: Instant::now(),
        }
    }

    fn build_and_verify_transaction(
        packet: &ImmutableDeserializedPacket,
        bank: &Bank,
    ) -> Result<SanitizedTransaction, TransactionError> {
        let transaction = SanitizedTransaction::try_new(
            packet.transaction().clone(),
            *packet.message_hash(),
            packet.is_simple_vote(),
            bank,
        )?;
        transaction.verify_precompiles(&bank.feature_set)?;
        Ok(transaction)
    }

    fn modify_priority(packet_priority: u64, _num_conflicts: u64) -> u64 {
        // (packet_priority * 1_000_000_000) / (1 + num_conflicts)
        packet_priority
    }
}

type PacketBatchMessage = Vec<PacketBatch>;
type TransactionMessage = Box<SanitizedTransaction>;
type CompletedTransactionMessage = (usize, TransactionMessage);
type TransactionBatchMessage = Vec<TransactionMessage>;

/// Separate packet deserialization and ordering
struct PacketBatchHandler {
    /// Exit signal
    exit: Arc<AtomicBool>,
    /// Bank
    bank_forks: Arc<RwLock<BankForks>>,
    /// Channel for receiving deserialized packet batches from SigVerify
    packet_batch_receiver: Receiver<PacketBatchMessage>,
    /// Pending transactions to be send to the scheduler
    pending_transactions: Arc<Mutex<BinaryHeap<TransactionRef>>>,
    /// Account Queues
    transactions_by_account: Arc<DashMap<Pubkey, AccountTransactionQueue>>,
}

impl PacketBatchHandler {
    /// Driving loop
    fn main(mut self) {
        loop {
            if self.exit.load(Ordering::Relaxed) {
                break;
            }
            self.iter();
        }
    }

    /// Try receiving packets or send out buffered transactions
    fn iter(&mut self) {
        if let Ok(packet_batches) = self.packet_batch_receiver.try_recv() {
            self.handle_packet_batches(packet_batches);
        }
    }

    /// Handle received packet batches - deserialize and put into the buffer
    fn handle_packet_batches(&mut self, packet_batches: Vec<PacketBatch>) {
        let bank = self.bank_forks.read().unwrap().working_bank();
        for packet_batch in packet_batches {
            let packet_indices = packet_batch
                .into_iter()
                .enumerate()
                .filter_map(|(idx, p)| if !p.meta.discard() { Some(idx) } else { None })
                .collect::<Vec<_>>();
            let transactions =
                unprocessed_packet_batches::deserialize_packets(&packet_batch, &packet_indices)
                    .filter_map(|deserialized_packet| {
                        TransactionPriority::build_and_verify_transaction(
                            deserialized_packet.immutable_section(),
                            &bank,
                        )
                        .ok()
                    })
                    .map(|transaction| {
                        let num_conflicts = self.get_num_conflicts(&transaction);
                        TransactionPriority::new(transaction, num_conflicts)
                    })
                    .collect::<Vec<_>>();
            self.insert_transactions(transactions);
        }
    }

    /// Count conflicts
    fn get_num_conflicts(&self, transaction: &SanitizedTransaction) -> u64 {
        let account_locks = transaction.get_account_locks().unwrap();

        // let mut read_conflicts = 0;
        for account in account_locks.readonly.into_iter() {
            let mut queue = self.transactions_by_account.entry(*account).or_default();
            // read_conflicts += queue.writes;
            queue.reads += 1;
        }

        let mut write_conflicts = 0;
        for account in account_locks.writable.into_iter() {
            let mut queue = self.transactions_by_account.entry(*account).or_default();
            write_conflicts += queue.writes + queue.reads;
            queue.writes += 1;
        }

        // read_conflicts + write_conflicts
        write_conflicts
    }

    /// Insert transactions into queues and pending
    fn insert_transactions(&self, transactions: Vec<TransactionRef>) {
        for tx in &transactions {
            // Get account locks
            let account_locks = tx.transaction.get_account_locks().unwrap();
            for account in account_locks.readonly.into_iter() {
                self.transactions_by_account.entry(*account).or_default();
                // .reads
                // .insert(tx.clone());
            }

            for account in account_locks.writable.into_iter() {
                self.transactions_by_account.entry(*account).or_default();
                // .writes
                // .insert(tx.clone());
            }
        }
        self.pending_transactions
            .lock()
            .unwrap()
            .extend(transactions.into_iter());
    }
}

/// Stores state for scheduling transactions and channels for communicating
/// with other threads: SigVerify and Banking
pub struct TransactionScheduler {
    /// Channels for sending transaction batches to banking threads
    transaction_batch_senders: Vec<Sender<TransactionBatchMessage>>,
    /// Channel for receiving completed transactions from any banking thread
    completed_transaction_receiver: Receiver<CompletedTransactionMessage>,
    /// Max number of transactions to send to a single banking-thread in a batch
    max_batch_size: usize,
    /// Exit signal
    exit: Arc<AtomicBool>,

    /// Pending transactions that are not known to be blocked
    pending_transactions: Arc<Mutex<BinaryHeap<TransactionRef>>>,
    /// Transaction queues and locks by account key
    transactions_by_account: Arc<DashMap<Pubkey, AccountTransactionQueue>>,
    /// Map from transaction signature to transactions blocked by the signature
    // blocked_transactions: HashMap<Signature, Vec<TransactionRef>>,
    /// Map from blocking BatchId to transactions
    blocked_transactions_by_batch_id: HashMap<TransactionBatchId, HashSet<TransactionRef>>,
    /// Transactions blocked by batches need to count how many they're blocked by
    // blocked_transactions_batch_count: HashMap<TransactionRef, usize>,
    /// Tracks the current number of blocked transactions
    num_blocked_transactions: usize,
    /// Tracks the current number of executing transacitons
    num_executing_transactions: usize,

    /// Generates TransactionBatchIds
    next_transaction_batch_id: TransactionBatchId,
    /// Tracks TransactionBatchDetails by TransactionBatchId
    transaction_batches: HashMap<TransactionBatchId, TransactionBatch>,
    /// Currently in-progress batches (references into `transaction_batches`)
    in_progress_batches: HashSet<TransactionBatchId>,
    oldest_in_progress_batch: Option<TransactionBatchId>,

    /// Tracks status of exeuction threads
    execution_thread_stats: Vec<ExecutionThreadStats>,

    /// Track metrics for scheduler thread
    metrics: SchedulerMetrics,
}

impl TransactionScheduler {
    /// Create and start transaction scheduler thread
    pub fn spawn_scheduler(
        packet_batch_receiver: Receiver<PacketBatchMessage>,
        transaction_batch_senders: Vec<Sender<TransactionBatchMessage>>,
        completed_transaction_receiver: Receiver<CompletedTransactionMessage>,
        bank_forks: Arc<RwLock<BankForks>>,
        max_batch_size: usize,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let pending_transactions = Arc::new(Mutex::new(BinaryHeap::default()));
        let transactions_by_account = Arc::new(DashMap::default());

        let packet_handler = PacketBatchHandler {
            exit: exit.clone(),
            bank_forks,
            packet_batch_receiver,
            pending_transactions: pending_transactions.clone(),
            transactions_by_account: transactions_by_account.clone(),
        };

        std::thread::spawn(move || packet_handler.main());
        let num_execution_threads = transaction_batch_senders.len();
        let execution_thread_stats = (0..num_execution_threads)
            .into_iter()
            .map(|_| ExecutionThreadStats::default())
            .collect();

        let mut scheduler = TransactionScheduler {
            transaction_batch_senders,
            completed_transaction_receiver,
            max_batch_size,
            exit,
            pending_transactions,
            transactions_by_account,
            // blocked_transactions: HashMap::default(),
            blocked_transactions_by_batch_id: HashMap::default(),
            // blocked_transactions_batch_count: HashMap::default(),
            num_blocked_transactions: 0,
            num_executing_transactions: 0,
            next_transaction_batch_id: 0,
            transaction_batches: HashMap::default(),
            in_progress_batches: HashSet::with_capacity(num_execution_threads),
            oldest_in_progress_batch: None,
            execution_thread_stats,
            metrics: SchedulerMetrics::default(),
        };

        // Initialize batch for thread 0
        scheduler.create_new_batch();

        std::thread::spawn(move || scheduler.main())
    }

    /// Driving loop
    fn main(mut self) {
        loop {
            if self.exit.load(Ordering::Relaxed) {
                break;
            }
            self.iter();
        }
    }

    /// Performs work in a loop - Handles different channel receives/timers and performs scheduling
    fn iter(&mut self) {
        fn try_recv<T>(receiver: &Receiver<T>) -> (Result<T, TryRecvError>, Measure) {
            measure!(receiver.try_recv())
        }

        // Try receiving completed batches
        let (_, completed_transaction_time) = measure!({
            let (maybe_completed_transaction, recv_time) =
                try_recv(&self.completed_transaction_receiver);
            let (_, handle_batch_time) = measure!({
                if let Ok((thread_index, completed_transaction)) = maybe_completed_transaction {
                    self.handle_completed_transaction(thread_index, completed_transaction);
                }
            });

            self.metrics.compeleted_batch_try_recv_time_us += recv_time.as_us();
            self.metrics.completed_batch_handle_batch_time_us += handle_batch_time.as_us();
        });
        self.metrics.completed_transactions_time_us += completed_transaction_time.as_us();

        // Scheduling time
        let (_, scheduling_time) = measure!(self.do_scheduling());
        self.metrics.scheduling_time_us += scheduling_time.as_us();

        // Check if oldest batch should be sent due to age
        if let Some(oldest_batch_id) = self.oldest_in_progress_batch {
            let batch = self.transaction_batches.get(&oldest_batch_id).unwrap();
            if batch.start_time.elapsed() >= std::time::Duration::from_millis(1000) {
                self.send_batch(oldest_batch_id);
            }
        }

        let (_, metrics_time) = measure!(self.metrics.report());
        self.metrics.metrics_us += metrics_time.as_us();
    }

    /// Handle completed transaction batch
    fn handle_completed_transaction(
        &mut self,
        execution_thread_index: usize,
        _transaction: TransactionMessage,
    ) {
        // TODO: verify transaction signature matches expectation?
        // Update queued stats
        self.num_executing_transactions -= 1;
        let execution_thread_stats = &mut self.execution_thread_stats[execution_thread_index];
        // TODO: actually track CUs instead of just num transactions
        execution_thread_stats.queued_compute_units -= 1;
        execution_thread_stats.queued_transactions -= 1;

        // Check if the batch is complete
        let batch_id = *execution_thread_stats
            .queued_batches
            .front()
            .expect("should not receive compelted transaction without a queued batch");
        let batch = self
            .transaction_batches
            .get_mut(&batch_id)
            .expect("queued batch id should exist");
        batch.num_transactions = batch.num_transactions.saturating_sub(1);

        if batch.num_transactions == 0 {
            // Remove the batch
            let batch = self.transaction_batches.remove(&batch_id).unwrap();
            execution_thread_stats.queued_batches.pop_front().unwrap();

            // Remove account locks
            let (_, remove_account_locks_time) = measure!({
                for (account, _lock) in batch.account_locks {
                    self.unlock_account(account, batch_id);
                }
            });

            // Push transactions blocked (by batch) back into the pending queue
            let (_, unblock_transactions_time) = measure!({
                if let Some(blocked_transactions) =
                    self.blocked_transactions_by_batch_id.remove(&batch_id)
                {
                    self.num_blocked_transactions -= blocked_transactions.len();
                    self.pending_transactions
                        .lock()
                        .unwrap()
                        .extend(blocked_transactions.into_iter());
                }
            });

            // Create a new batch for the thread (if necessary)
            self.create_new_batch();

            self.metrics.completed_transactions_remove_account_locks_us +=
                remove_account_locks_time.as_us();
            self.metrics.completed_transactions_unblock_transactions_us +=
                unblock_transactions_time.as_us();
        }
    }

    /// Remove account locks for the batch id
    fn unlock_account(&mut self, account: Pubkey, batch_id: TransactionBatchId) {
        self.transactions_by_account
            .get_mut(&account)
            .unwrap()
            .unlock(batch_id)
    }

    fn create_new_batch(&mut self) {
        // Find first thread with no work on it and NO threads before that have an in-progress
        let mut execution_thread_index = None;
        for (thread_index, execution_thread_stats) in self.execution_thread_stats.iter().enumerate()
        {
            let num_queued_batches = execution_thread_stats.queued_batches.len();
            // If no scheduled work on this thread, create the new batch here
            if num_queued_batches == 0 {
                execution_thread_index = Some(thread_index);
                break;
            } else if num_queued_batches == 1 {
                if self
                    .in_progress_batches
                    .contains(execution_thread_stats.queued_batches.back().unwrap())
                {
                    return; // don't create a new batch since we already have one that's not been filled up yet
                } else {
                    // This thread has a free-queue spot, create a new batch here
                    execution_thread_index = Some(thread_index);
                    break;
                }
            }
        }

        if let Some(execution_thread_index) = execution_thread_index {
            self.metrics.max_thread_index_used = self
                .metrics
                .max_thread_index_used
                .max(execution_thread_index);

            let batch_id = self.next_transaction_batch_id;
            self.next_transaction_batch_id += 1;
            self.transaction_batches.insert(
                batch_id,
                TransactionBatch {
                    scheduled: false,
                    start_time: Instant::now(),
                    id: batch_id,
                    num_transactions: 0,
                    transactions: Vec::with_capacity(self.max_batch_size),
                    account_locks: HashMap::default(),
                    execution_thread_index,
                },
            );
            self.execution_thread_stats[execution_thread_index]
                .queued_batches
                .push_back(batch_id);
            self.in_progress_batches.insert(batch_id);
            self.oldest_in_progress_batch.get_or_insert(batch_id);
        }
    }

    /// Send an in-progress batch
    fn send_batch(&mut self, batch_id: usize) {
        let batch = self.transaction_batches.get_mut(&batch_id).unwrap();
        self.metrics.max_batch_age = self
            .metrics
            .max_batch_age
            .max(batch.start_time.elapsed().as_micros() as u64);
        let execution_thread_index = batch.execution_thread_index;

        if !batch.transactions.is_empty() {
            // 1. Send the batch to thread
            // Build the batch
            let transactions = batch.build_account_locks_on_send();

            // Update execution thread stats
            let execution_thread_stats = &mut self.execution_thread_stats[execution_thread_index];
            execution_thread_stats.queued_transactions += transactions.len();

            // Update metrics
            self.num_executing_transactions += transactions.len();
            self.metrics.num_transactions_scheduled += transactions.len();
            self.metrics.max_blocked_transactions = self
                .metrics
                .max_blocked_transactions
                .max(self.num_blocked_transactions);
            self.metrics.max_executing_transactions = self
                .num_executing_transactions
                .max(self.num_executing_transactions);

            // Send the batch
            self.transaction_batch_senders[execution_thread_index]
                .send(transactions)
                .unwrap();

            // 2. Remove from in-progress batch set
            self.in_progress_batches.remove(&batch_id);

            // 3. Check if we should create a new batch for the thread
            self.create_new_batch();
            if batch_id == self.oldest_in_progress_batch.unwrap() {
                self.oldest_in_progress_batch = self.in_progress_batches.iter().min().cloned()
            }
            // 4. Possibly update the oldest batch
        } else {
            self.execution_thread_stats[execution_thread_index]
                .queued_batches
                .pop_front()
                .unwrap();
            self.transaction_batches.remove(&batch_id).unwrap();
            self.in_progress_batches.remove(&batch_id);
            self.create_new_batch();
            if batch_id == self.oldest_in_progress_batch.unwrap() {
                self.oldest_in_progress_batch = self.in_progress_batches.iter().min().cloned()
            }
        }
    }

    /// Performs scheduling operations on currently pending transactions
    fn do_scheduling(&mut self) {
        if self.in_progress_batches.is_empty() {
            return;
        }
        let num_pending_transactions = self.pending_transactions.lock().unwrap().len();
        self.metrics.max_pending_transactions = self
            .metrics
            .max_pending_transactions
            .max(num_pending_transactions);
        let maybe_transaction = self.pending_transactions.lock().unwrap().pop();
        if let Some(transaction) = maybe_transaction {
            self.try_schedule_transaction(transaction);
        }
    }

    /// Try to schedule a transaction
    fn try_schedule_transaction(&mut self, transaction: TransactionRef) {
        // Check for blocking transactions batches in scheduled locks (this includes batches currently being built)
        let (conflicting_batches, get_conflicting_batches_time) =
            measure!(self.get_conflicting_batches(&transaction));
        self.metrics.get_conflicting_batches_time += get_conflicting_batches_time.as_us();

        let maybe_batch_id = if let Some(conflicting_batches) = conflicting_batches.as_ref() {
            let mut schedulable_thread_index = None;
            for batch_id in conflicting_batches {
                let thread_index = self
                    .transaction_batches
                    .get(batch_id)
                    .unwrap()
                    .execution_thread_index;

                if thread_index != *schedulable_thread_index.get_or_insert(thread_index) {
                    schedulable_thread_index = None;
                    break;
                }
            }
            schedulable_thread_index
                .map(|thread_index| {
                    self.execution_thread_stats[thread_index]
                        .queued_batches
                        .back()
                        .unwrap()
                })
                .filter(|batch_id| self.in_progress_batches.contains(*batch_id))
                .cloned()
        } else {
            // Find the lowest-thread in-progress batch
            self.in_progress_batches
                .iter()
                .map(|x| {
                    (
                        self.transaction_batches
                            .get(x)
                            .unwrap()
                            .execution_thread_index,
                        *x,
                    )
                })
                .min()
                .map(|(_, x)| x)
        };

        if maybe_batch_id.is_none() {
            if let Some(conflicting_batches) = conflicting_batches {
                self.num_blocked_transactions += 1;
                let newest_conflicting_batch = conflicting_batches.into_iter().max().unwrap();
                assert!(self
                    .blocked_transactions_by_batch_id
                    .entry(newest_conflicting_batch)
                    .or_default()
                    .insert(transaction));
            }
            return;
        }

        let batch_id = maybe_batch_id.unwrap();
        // Schedule the transaction:
        let (_, batching_time) = measure!({
            // 1. Add account locks with the batch id
            self.lock_accounts_for_transaction(&transaction, batch_id);
            // 2. Add to Batch
            let batch = self.transaction_batches.get_mut(&batch_id).unwrap();
            assert!(!batch.scheduled);
            batch.transactions.push(transaction);
            // 3. Update queued execution stats
            self.execution_thread_stats[batch.execution_thread_index].queued_compute_units += 1; // TODO: actually use CU instead of # tx
            self.execution_thread_stats[batch.execution_thread_index].queued_transactions += 1;

            // Check if batch should be sent
            if batch.transactions.len() == self.max_batch_size {
                let batch_id = batch.id;
                self.send_batch(batch_id);
            }
        });
        self.metrics.batching_time += batching_time.as_us();
    }

    /// Gets batches that conflict with the current transaction
    ///     - Conflict does not necessarily mean block as they can be scheduled on the same thread
    fn get_conflicting_batches(
        &self,
        transaction: &TransactionRef,
    ) -> Option<HashSet<TransactionBatchId>> {
        let mut conflicting_batches = HashSet::default();

        let account_locks = transaction.transaction.get_account_locks().unwrap();

        // Read accounts will only be blocked by writes on other threads
        for account in account_locks.readonly.into_iter() {
            for batch_id in self
                .transactions_by_account
                .get(account)
                .unwrap()
                .scheduled_lock
                .write_batches
                .iter()
            {
                conflicting_batches.insert(*batch_id);
            }
        }

        // Write accounts will be blocked by reads or writes on other threads
        for account in account_locks.writable.into_iter() {
            let scheduled_lock = &self
                .transactions_by_account
                .get(account)
                .unwrap()
                .scheduled_lock;
            for batch_id in scheduled_lock.write_batches.iter() {
                conflicting_batches.insert(*batch_id);
            }
            for batch_id in scheduled_lock.read_batches.iter() {
                conflicting_batches.insert(*batch_id);
            }
        }

        (!conflicting_batches.is_empty()).then(|| conflicting_batches)
    }

    /// Lock accounts for a transaction by batch id
    fn lock_accounts_for_transaction(
        &mut self,
        transaction: &TransactionRef,
        batch_id: TransactionBatchId,
    ) {
        let accounts = transaction.transaction.get_account_locks().unwrap();
        for account in accounts.readonly {
            let mut queue = self.transactions_by_account.get_mut(account).unwrap();
            queue.scheduled_lock.lock_on_batch(batch_id, false);
            queue.reads -= 1;
        }
        for account in accounts.writable {
            let mut queue = self.transactions_by_account.get_mut(account).unwrap();
            queue.scheduled_lock.lock_on_batch(batch_id, true);
            queue.writes -= 1;
        }
    }
}

/// Tracks all pending and blocked transacitons, ordered by priority, for a single account
#[derive(Default)]
struct AccountTransactionQueue {
    reads: u64,  // unscheduled number of transactions reading
    writes: u64, // unscheduled number of transactions writing

    // /// Tree of read transactions on the account ordered by fee-priority
    // reads: BTreeSet<TransactionRef>,
    // /// Tree of write transactions on the account ordered by fee-priority
    // writes: BTreeSet<TransactionRef>,
    /// Tracks currently scheduled transactions on the account
    scheduled_lock: AccountLock,
}

impl AccountTransactionQueue {
    /// Unlocks the account queue for `batch_id`
    fn unlock(&mut self, batch_id: TransactionBatchId) {
        self.scheduled_lock.unlock_on_batch(batch_id);
    }

    // /// Find the minimum-priority transaction that blocks this transaction if there is one
    // fn get_min_blocking_transaction(
    //     &self,
    //     transaction: &TransactionRef,
    //     is_write: bool,
    // ) -> Option<TransactionRef> {
    //     let mut min_blocking_transaction = None;
    //     // Write transactions will be blocked by higher-priority reads, but read transactions will not
    //     if is_write {
    //         min_blocking_transaction = option_min(
    //             min_blocking_transaction,
    //             upper_bound(&self.reads, transaction.clone()),
    //         );
    //     }

    //     // All transactions are blocked by higher-priority write-transactions
    //     option_min(
    //         min_blocking_transaction,
    //         upper_bound(&self.writes, transaction.clone()),
    //     )
    //     .map(|txr| txr.clone())
    // }
}

/// Tracks the lock status of an account by batch id
#[derive(Debug, Default)]
struct AccountLock {
    read_batches: HashSet<TransactionBatchId>,
    write_batches: HashSet<TransactionBatchId>,
}

impl AccountLock {
    fn lock_on_batch(&mut self, batch_id: TransactionBatchId, is_write: bool) {
        if is_write {
            // override read lock
            self.read_batches.remove(&batch_id);
            self.write_batches.insert(batch_id);
        } else {
            // underride write lock
            if !self.write_batches.contains(&batch_id) {
                self.read_batches.insert(batch_id);
            }
        }
    }

    fn unlock_on_batch(&mut self, batch_id: TransactionBatchId) {
        self.read_batches.remove(&batch_id);
        self.write_batches.remove(&batch_id);
    }
}

#[derive(Debug, Clone)]
enum AccountLockKind {
    Read,
    Write,
}

/// Identified for TransactionBatches
pub type TransactionBatchId = usize;

/// Transactions in a batch
#[derive(Debug)]
struct TransactionBatch {
    /// Has the transaction been sent
    scheduled: bool,
    /// Timestamp of the batch starting to be built
    start_time: Instant,
    /// Identifier
    id: TransactionBatchId,
    /// Number of transactions
    num_transactions: usize,
    /// Transactions (only valid before send)
    transactions: Vec<TransactionRef>,
    /// Locked Accounts and Kind Set (only built on send)
    account_locks: HashMap<Pubkey, AccountLockKind>,
    /// Thread it is scheduled on
    execution_thread_index: usize,
}

impl TransactionBatch {
    fn build_account_locks_on_send(&mut self) -> TransactionBatchMessage {
        self.scheduled = true;
        self.num_transactions = self.transactions.len();
        for transaction in self.transactions.iter() {
            let account_locks = transaction.transaction.get_account_locks().unwrap();

            for account in account_locks.readonly.into_iter() {
                self.account_locks
                    .entry(*account)
                    .or_insert(AccountLockKind::Read);
            }
            for account in account_locks.writable.into_iter() {
                self.account_locks.insert(*account, AccountLockKind::Write);
            }
        }

        let mut transactions = Vec::new();
        std::mem::swap(&mut transactions, &mut self.transactions);

        transactions.into_iter().map(|tx| tx.transaction).collect()
    }
}

/// Track stats for the execution threads - Order of members matters for derived implementations of PartialCmp
#[derive(Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct ExecutionThreadStats {
    /// Currently queued compute-units
    queued_compute_units: usize,
    /// Currently queued number of transactions
    queued_transactions: usize,
    /// Currently queued batch ids
    queued_batches: VecDeque<TransactionBatchId>,
}

/// Track metrics for the scheduler thread
struct SchedulerMetrics {
    /// Last timestamp reported
    last_reported: Instant,
    /// Number of transactions scheduled
    num_transactions_scheduled: usize,
    /// Maximum pending_transactions length
    max_pending_transactions: usize,
    /// Maximum number of blocked transactions
    max_blocked_transactions: usize,
    /// Maximum executing transactions
    max_executing_transactions: usize,

    /// Total time spent processing completed transactions in microseconds
    completed_transactions_time_us: u64,
    /// Completed transactions - TryRecv time
    compeleted_batch_try_recv_time_us: u64,
    /// Completed transactions - Handle completed batch
    completed_batch_handle_batch_time_us: u64,
    completed_transactions_remove_account_locks_us: u64,
    completed_transactions_unblock_transactions_us: u64,
    /// Total time spent scheduling transactions in microseconds
    scheduling_time_us: u64,
    scheduling_try_schedule_time_us: u64,
    check_blocking_transactions_time_us: u64,
    get_conflicting_batches_time: u64,
    find_thread_index_time: u64,
    batching_time: u64,
    max_batch_age: u64,
    max_thread_index_used: usize,

    /// Time spent checking and reporting metrics
    metrics_us: u64,
}

impl Default for SchedulerMetrics {
    fn default() -> Self {
        Self {
            last_reported: Instant::now(),
            num_transactions_scheduled: Default::default(),
            max_pending_transactions: Default::default(),
            max_blocked_transactions: Default::default(),
            max_executing_transactions: Default::default(),
            completed_transactions_time_us: Default::default(),
            scheduling_time_us: Default::default(),
            compeleted_batch_try_recv_time_us: Default::default(),
            completed_batch_handle_batch_time_us: Default::default(),
            metrics_us: Default::default(),
            completed_transactions_remove_account_locks_us: Default::default(),
            completed_transactions_unblock_transactions_us: Default::default(),
            scheduling_try_schedule_time_us: Default::default(),
            check_blocking_transactions_time_us: Default::default(),
            get_conflicting_batches_time: Default::default(),
            find_thread_index_time: Default::default(),
            batching_time: Default::default(),
            max_batch_age: Default::default(),
            max_thread_index_used: Default::default(),
        }
    }
}

impl SchedulerMetrics {
    /// Report metrics if the interval has passed and reset metrics
    fn report(&mut self) {
        const REPORT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1000);

        if self.last_reported.elapsed() >= REPORT_INTERVAL {
            datapoint_info!(
                "transaction-scheduler",
                (
                    "num_transactions_scheduled",
                    self.num_transactions_scheduled as i64,
                    i64
                ),
                (
                    "max_pending_transactions",
                    self.max_pending_transactions as i64,
                    i64
                ),
                (
                    "max_blocked_transactions",
                    self.max_blocked_transactions as i64,
                    i64
                ),
                (
                    "max_executing_transactions",
                    self.max_executing_transactions as i64,
                    i64
                ),
                (
                    "completed_transactions_time_us",
                    self.completed_transactions_time_us as i64,
                    i64
                ),
                (
                    "compeleted_batch_try_recv_time_us",
                    self.compeleted_batch_try_recv_time_us as i64,
                    i64
                ),
                (
                    "completed_batch_handle_batch_time_us",
                    self.completed_batch_handle_batch_time_us as i64,
                    i64
                ),
                (
                    "completed_transactions_remove_account_locks_us",
                    self.completed_transactions_remove_account_locks_us as i64,
                    i64
                ),
                (
                    "completed_transactions_unblock_transactions_us",
                    self.completed_transactions_unblock_transactions_us as i64,
                    i64
                ),
                ("scheduling_time_us", self.scheduling_time_us as i64, i64),
                (
                    "scheduling_try_schedule_time_us",
                    self.scheduling_try_schedule_time_us as i64,
                    i64
                ),
                (
                    "check_blocking_transactions_time_us",
                    self.check_blocking_transactions_time_us as i64,
                    i64
                ),
                (
                    "get_conflicting_batches_time",
                    self.get_conflicting_batches_time as i64,
                    i64
                ),
                (
                    "find_thread_index_time",
                    self.find_thread_index_time as i64,
                    i64
                ),
                ("batching_time", self.batching_time as i64, i64),
                ("metrics_us", self.metrics_us as i64, i64),
                ("max_batch_age", self.max_batch_age as i64, i64),
                (
                    "max_thread_index_used",
                    self.max_thread_index_used as i64,
                    i64
                ),
            );

            *self = Self::default();
        }
    }
}
