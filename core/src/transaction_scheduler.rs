//! Implements a transaction scheduler for the three types of transaction receiving pipelines:
//! - Normal transactions
//! - TPU vote transactions
//! - Gossip vote transactions

use {
    crate::{
        banking_stage::BatchedTransactionDetails,
        qos_service::QosService,
        unprocessed_packet_batches::{self, DeserializedPacket, ImmutableDeserializedPacket},
    },
    crossbeam_channel::{select, unbounded, Receiver, RecvError, Sender},
    skiplist::OrderedSkipList,
    solana_perf::packet::PacketBatch,
    solana_runtime::{
        accounts::AccountLocks,
        bank::Bank,
        cost_model::{CostModel, TransactionCost},
        transaction_error_metrics::TransactionErrorMetrics,
    },
    solana_sdk::{
        clock::MAX_PROCESSING_AGE,
        feature_set,
        pubkey::Pubkey,
        saturating_add_assign,
        transaction::{self, AddressLoader, SanitizedTransaction, TransactionError},
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        rc::Rc,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex, RwLock,
        },
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
    thiserror::Error,
};

#[derive(Debug)]
pub enum SchedulerMessage {
    RequestBatch {
        num_txs: usize,
        bank: Arc<Bank>,
    },
    Ping {
        id: usize,
    },
    ExecutedBatchUpdate {
        executed_transactions: Vec<SanitizedTransaction>,
        rescheduled_transactions: Vec<SanitizedTransaction>,
    },
}

#[derive(Debug)]
pub struct SchedulerRequest {
    msg: SchedulerMessage,
    response_sender: Sender<SchedulerResponse>,
}

pub struct ScheduledBatch {
    pub sanitized_transactions: Vec<SanitizedTransaction>,
}

pub struct Pong {
    pub id: usize,
}

#[derive(Clone)]
pub struct ExecutedBatchResponse {}

pub enum SchedulerResponse {
    ScheduledBatch(ScheduledBatch),
    Pong(Pong),
    ExecutedBatchResponse(ExecutedBatchResponse),
}

impl SchedulerResponse {
    fn pong(self) -> Pong {
        match self {
            SchedulerResponse::Pong(pong) => pong,
            _ => {
                unreachable!("invalid response expected");
            }
        }
    }

    fn scheduled_batch(self) -> ScheduledBatch {
        match self {
            SchedulerResponse::ScheduledBatch(batch) => batch,
            _ => {
                unreachable!("invalid response expected");
            }
        }
    }

    fn executed_batch_response(self) -> ExecutedBatchResponse {
        match self {
            SchedulerResponse::ExecutedBatchResponse(response) => response,
            _ => {
                unreachable!("invalid response expected");
            }
        }
    }
}

pub enum SchedulerStage {
    // normal transactions
    Transactions,
    // votes coming in on tpu port
    TpuVotes,
    // gossip votes
    GossipVotes,
}

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum SchedulerError {
    #[error("invalid sanitized transaction")]
    InvalidSanitizedTransaction,

    #[error("irrecoverable transaction format error: {0}")]
    InvalidTransactionFormat(TransactionError),

    #[error("account in use")]
    AccountInUse,

    #[error("account is blocked by higher paying: account {0}")]
    AccountBlocked(Pubkey),

    #[error("transaction check failed {0}")]
    TransactionCheckFailed(TransactionError),

    #[error("transaction exceeded QOS limit")]
    QosExceeded,
}

pub type Result<T> = std::result::Result<T, SchedulerError>;

#[derive(Clone)]
pub struct TransactionSchedulerHandle {
    sender: Sender<SchedulerRequest>,
}

impl TransactionSchedulerHandle {
    pub fn new(sender: Sender<SchedulerRequest>) -> TransactionSchedulerHandle {
        TransactionSchedulerHandle { sender }
    }

    /// Requests a batch of num_txs transactions from one of the scheduler stages.
    pub fn request_batch(
        &self,
        num_txs: usize,
        bank: &Arc<Bank>,
    ) -> std::result::Result<ScheduledBatch, RecvError> {
        Self::make_scheduler_request(
            &self.sender,
            SchedulerMessage::RequestBatch {
                num_txs,
                bank: bank.clone(),
            },
        )
        .map(|r| r.scheduled_batch())
    }

    /// Ping-pong a scheduler stage
    pub fn send_ping(&self, id: usize) -> std::result::Result<Pong, RecvError> {
        Self::make_scheduler_request(&self.sender, SchedulerMessage::Ping { id }).map(|r| r.pong())
    }

    /// Send the scheduler an update on what was scheduled
    pub fn send_batch_execution_update(
        &self,
        executed_transactions: Vec<SanitizedTransaction>,
        rescheduled_transactions: Vec<SanitizedTransaction>,
    ) -> std::result::Result<ExecutedBatchResponse, RecvError> {
        Self::make_scheduler_request(
            &self.sender,
            SchedulerMessage::ExecutedBatchUpdate {
                executed_transactions,
                rescheduled_transactions,
            },
        )
        .map(|r| r.executed_batch_response())
    }

    /// Sends a scheduler request and blocks on waiting for a response
    fn make_scheduler_request(
        request_sender: &Sender<SchedulerRequest>,
        msg: SchedulerMessage,
    ) -> std::result::Result<SchedulerResponse, RecvError> {
        let (response_sender, response_receiver) = unbounded();
        let request = SchedulerRequest {
            msg,
            response_sender,
        };
        let _ = request_sender.send(request).unwrap();
        response_receiver.recv()
    }
}

#[derive(Clone)]
pub struct TransactionSchedulerConfig {
    // ensures T(1) is scheduled before T(2) if T(1) pays higher fees than T(2) for all account
    // accesses except where T(1) and T(2) only accounts overlap are read-only accounts.
    pub enable_state_auction: bool,

    // max packets to hold in each thread
    pub backlog_size: usize,
}

impl Default for TransactionSchedulerConfig {
    fn default() -> Self {
        return TransactionSchedulerConfig {
            enable_state_auction: false,
            backlog_size: 700_000,
        };
    }
}

pub struct TransactionScheduler {
    tx_request_handler_thread: JoinHandle<()>,
    tx_scheduler_request_sender: Sender<SchedulerRequest>,

    tpu_vote_request_handler_thread: JoinHandle<()>,
    tpu_vote_scheduler_request_sender: Sender<SchedulerRequest>,

    gossip_vote_request_handler_thread: JoinHandle<()>,
    gossip_vote_scheduler_request_sender: Sender<SchedulerRequest>,
}

impl TransactionScheduler {
    /// Creates a thread for each type of transaction and a handle to the event loop.
    pub fn new(
        verified_receiver: Receiver<Vec<PacketBatch>>,
        verified_tpu_vote_packets_receiver: Receiver<Vec<PacketBatch>>,
        verified_gossip_vote_packets_receiver: Receiver<Vec<PacketBatch>>,
        exit: Arc<AtomicBool>,
        cost_model: Arc<RwLock<CostModel>>,
        config: TransactionSchedulerConfig,
    ) -> Self {
        let scheduled_accounts = Arc::new(Mutex::new(AccountLocks::default()));

        let (tx_scheduler_request_sender, tx_scheduler_request_receiver) = unbounded();
        let tx_request_handler_thread = Self::start_event_loop(
            "tx_scheduler_insertion_thread",
            tx_scheduler_request_receiver,
            verified_receiver,
            scheduled_accounts.clone(),
            QosService::new(cost_model.clone(), 0),
            &exit,
            config.clone(),
        );

        let (tpu_vote_scheduler_request_sender, tpu_vote_scheduler_request_receiver) = unbounded();
        let tpu_vote_request_handler_thread = Self::start_event_loop(
            "tpu_vote_scheduler_tx_insertion_thread",
            tpu_vote_scheduler_request_receiver,
            verified_tpu_vote_packets_receiver,
            scheduled_accounts.clone(),
            QosService::new(cost_model.clone(), 1),
            &exit,
            config.clone(),
        );

        let (gossip_vote_scheduler_request_sender, gossip_vote_scheduler_request_receiver) =
            unbounded();
        let gossip_vote_request_handler_thread = Self::start_event_loop(
            "gossip_vote_scheduler_tx_insertion_thread",
            gossip_vote_scheduler_request_receiver,
            verified_gossip_vote_packets_receiver,
            scheduled_accounts.clone(),
            QosService::new(cost_model.clone(), 2),
            &exit,
            config.clone(),
        );

        TransactionScheduler {
            tx_request_handler_thread,
            tx_scheduler_request_sender,
            tpu_vote_request_handler_thread,
            tpu_vote_scheduler_request_sender,
            gossip_vote_request_handler_thread,
            gossip_vote_scheduler_request_sender,
        }
    }

    // ***************************************************************
    // Client methods
    // ***************************************************************

    /// Returns a handle to one of the schedulers
    pub fn get_handle(&self, scheduler_stage: SchedulerStage) -> TransactionSchedulerHandle {
        TransactionSchedulerHandle::new(self.get_sender_from_stage(scheduler_stage).clone())
    }

    /// Clean up the threads
    pub fn join(self) -> thread::Result<()> {
        self.tx_request_handler_thread.join()?;
        self.tpu_vote_request_handler_thread.join()?;
        self.gossip_vote_request_handler_thread.join()?;
        Ok(())
    }

    // ***************************************************************
    // Internal logic
    // ***************************************************************

    /// The event loop has two main responsibilities:
    /// 1. Handle incoming packets and prioritization around them.
    /// 2. Serve scheduler requests and return responses.
    fn start_event_loop(
        t_name: &str,
        scheduler_request_receiver: Receiver<SchedulerRequest>,
        packet_receiver: Receiver<Vec<PacketBatch>>,
        scheduled_accounts: Arc<Mutex<AccountLocks>>,
        qos_service: QosService,
        exit: &Arc<AtomicBool>,
        config: TransactionSchedulerConfig,
    ) -> JoinHandle<()> {
        let exit = exit.clone();
        Builder::new()
            .name(t_name.to_string())
            .spawn(move || {
                let mut unprocessed_packet_batches = OrderedSkipList::with_capacity(config.backlog_size);

                let mut last_log = Instant::now();

                loop {
                    if last_log.elapsed() > Duration::from_secs(1) {
                        let num_packets = unprocessed_packet_batches.len();
                        info!("num_packets: {}", num_packets);
                        last_log = Instant::now();
                    }
                    select! {
                        recv(packet_receiver) -> maybe_packet_batches => {
                            match maybe_packet_batches {
                                Ok(packet_batches) => {
                                    Self::handle_packet_batches(&mut unprocessed_packet_batches, packet_batches);
                                }
                                Err(_) => {
                                    break;
                                }
                            }
                        }
                        recv(scheduler_request_receiver) -> maybe_batch_request => {
                            match maybe_batch_request {
                                Ok(batch_request) => {
                                    Self::handle_scheduler_request(&mut unprocessed_packet_batches, &scheduled_accounts, batch_request, &qos_service, &config);
                                }
                                Err(_) => {
                                    break;
                                }
                            }
                        }
                        default(Duration::from_millis(100)) => {
                            if exit.load(Ordering::Relaxed) {
                                break;
                            }
                        }
                    }
                }
            })
            .unwrap()
    }

    /// Attempts to schedule a transaction to be executed.
    fn try_schedule(
        deserialized_packet: &Rc<ImmutableDeserializedPacket>,
        bank: &Arc<Bank>,
        highest_wl_blocked_account_fees: &mut HashMap<Pubkey, u64>,
        highest_rl_blocked_account_fees: &mut HashMap<Pubkey, u64>,
        scheduled_accounts: &Arc<Mutex<AccountLocks>>,
        qos_service: &QosService,
        config: &TransactionSchedulerConfig,
    ) -> Result<SanitizedTransaction> {
        let sanitized_tx = Self::transaction_from_deserialized_packet(
            deserialized_packet,
            &bank.feature_set,
            bank.vote_only_bank(),
            bank.as_ref(),
        )
        .ok_or_else(|| SchedulerError::InvalidSanitizedTransaction)?;

        let priority = deserialized_packet.priority();

        let account_locks = sanitized_tx
            .get_account_locks(&bank.feature_set)
            .map_err(|e| SchedulerError::InvalidTransactionFormat(e))?;

        trace!(
            "popped tx w/ priority: {}, readable: {:?}, writeable: {:?}",
            priority,
            account_locks.readonly,
            account_locks.writable
        );

        // pre-emptively lock the accounts associated with the transaction
        {
            let mut scheduled_accounts_l = scheduled_accounts.lock().unwrap();
            // Make sure that this transaction isn't blocked on another transaction that has a higher
            // fee for one of its accounts
            if let Err(e) = Self::check_accounts_not_blocked(
                &scheduled_accounts_l,
                highest_wl_blocked_account_fees,
                highest_rl_blocked_account_fees,
                &account_locks.writable,
                &account_locks.readonly,
                config,
            ) {
                Self::upsert_higher_fee_account_lock(
                    &account_locks.writable,
                    &account_locks.readonly,
                    highest_wl_blocked_account_fees,
                    highest_rl_blocked_account_fees,
                    priority,
                    config,
                );
                return Err(e);
            }

            // already checked we can lock accounts in check_accounts_not_blocked
            Self::lock_accounts(
                &mut scheduled_accounts_l,
                &account_locks.writable,
                &account_locks.readonly,
            )
            .unwrap();
        }

        let mut sanitized_txs = vec![sanitized_tx];
        let lock_results = vec![Ok(())];
        let mut error_counters = TransactionErrorMetrics::new();

        // Make sure transaction isn't stale or already executed
        if let Err(e) = bank
            .check_transactions(
                &sanitized_txs,
                &lock_results,
                MAX_PROCESSING_AGE,
                &mut error_counters,
            )
            .pop()
            .unwrap()
            .0
        {
            let mut scheduled_accounts_l = scheduled_accounts.lock().unwrap();
            let sanitized_tx = sanitized_txs.pop().unwrap();
            let account_locks = sanitized_tx.get_account_locks(&bank.feature_set).unwrap();
            Self::drop_account_locks(
                &mut scheduled_accounts_l,
                &account_locks.writable,
                &account_locks.readonly,
            );
            return Err(SchedulerError::TransactionCheckFailed(e));
        }

        // Try to reserve in QoS, rollback account locks if doesn't fit
        if let Err(e) = Self::reserve_tx_in_qos(&sanitized_txs, qos_service, bank) {
            let mut scheduled_accounts_l = scheduled_accounts.lock().unwrap();
            let sanitized_tx = sanitized_txs.pop().unwrap();
            let account_locks = sanitized_tx.get_account_locks(&bank.feature_set).unwrap();
            Self::drop_account_locks(
                &mut scheduled_accounts_l,
                &account_locks.writable,
                &account_locks.readonly,
            );
            return Err(e);
        }

        // TODO: check fee payer's balance is high enough
        // sanitized_tx.message().fee_payer()

        let sanitized_tx = sanitized_txs.pop().unwrap();
        Ok(sanitized_tx)
    }

    fn reserve_tx_in_qos(
        sanitized_txs: &[SanitizedTransaction],
        qos_service: &QosService,
        bank: &Arc<Bank>,
    ) -> Result<()> {
        let transaction_costs = qos_service.compute_transaction_costs(sanitized_txs.iter());

        let (transactions_qos_results, num_included) = qos_service.select_transactions_per_cost(
            sanitized_txs.iter(),
            transaction_costs.iter(),
            bank,
        );

        let cost_model_throttled_transactions_count =
            sanitized_txs.len().saturating_sub(num_included);
        if cost_model_throttled_transactions_count > 0 {
            return Err(SchedulerError::QosExceeded);
        }

        qos_service.accumulate_estimated_transaction_costs(
            &Self::accumulate_batched_transaction_costs(
                transaction_costs.iter(),
                transactions_qos_results.iter(),
            ),
        );
        Ok(())
    }

    // rollup transaction cost details, eg signature_cost, write_lock_cost, data_bytes_cost and
    // execution_cost from the batch of transactions selected for block.
    fn accumulate_batched_transaction_costs<'a>(
        transactions_costs: impl Iterator<Item = &'a TransactionCost>,
        transaction_results: impl Iterator<Item = &'a transaction::Result<()>>,
    ) -> BatchedTransactionDetails {
        let mut batched_transaction_details = BatchedTransactionDetails::default();
        transactions_costs
            .zip(transaction_results)
            .for_each(|(cost, result)| match result {
                Ok(_) => {
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_signature_cost,
                        cost.signature_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_write_lock_cost,
                        cost.write_lock_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_data_bytes_cost,
                        cost.data_bytes_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details
                            .costs
                            .batched_builtins_execute_cost,
                        cost.builtins_execution_cost
                    );
                    saturating_add_assign!(
                        batched_transaction_details.costs.batched_bpf_execute_cost,
                        cost.bpf_execution_cost
                    );
                }
                Err(transaction_error) => match transaction_error {
                    TransactionError::WouldExceedMaxBlockCostLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_block_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedMaxVoteCostLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_vote_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedMaxAccountCostLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_account_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedAccountDataBlockLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_retried_txs_per_account_data_block_limit_count,
                            1
                        );
                    }
                    TransactionError::WouldExceedAccountDataTotalLimit => {
                        saturating_add_assign!(
                            batched_transaction_details
                                .errors
                                .batched_dropped_txs_per_account_data_total_limit_count,
                            1
                        );
                    }
                    _ => {}
                },
            });
        batched_transaction_details
    }

    fn upsert_higher_fee_account_lock(
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
        highest_wl_blocked_account_fees: &mut HashMap<Pubkey, u64>,
        highest_rl_blocked_account_fees: &mut HashMap<Pubkey, u64>,
        priority: u64,
        config: &TransactionSchedulerConfig,
    ) {
        if config.enable_state_auction {
            for acc in readonly_keys {
                match highest_rl_blocked_account_fees.entry(**acc) {
                    Entry::Occupied(mut e) => {
                        if priority > *e.get() {
                            // NOTE: this should never be the case!
                            e.insert(priority);
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert(priority);
                    }
                }
            }

            for acc in writable_keys {
                match highest_wl_blocked_account_fees.entry(**acc) {
                    Entry::Occupied(mut e) => {
                        if priority > *e.get() {
                            // NOTE: this should never be the case!
                            e.insert(priority);
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert(priority);
                    }
                }
            }
        }
    }

    fn check_accounts_not_blocked(
        account_locks: &AccountLocks,
        highest_wl_blocked_account_fees: &HashMap<Pubkey, u64>,
        highest_rl_blocked_account_fees: &HashMap<Pubkey, u64>,
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
        config: &TransactionSchedulerConfig,
    ) -> Result<()> {
        if config.enable_state_auction {
            // writes are blocked if there's a blocked read or write account already
            for acc in writable_keys {
                if highest_wl_blocked_account_fees.get(*acc).is_some()
                    || highest_rl_blocked_account_fees.get(*acc).is_some()
                {
                    trace!("write locked account blocked by another tx: {:?}", acc);
                    return Err(SchedulerError::AccountBlocked(**acc));
                }
            }

            // reads are blocked if the account exists in write blocked state
            for acc in readonly_keys {
                if highest_wl_blocked_account_fees.get(*acc).is_some() {
                    trace!("read locked account blocked by another tx: {:?}", acc);
                    return Err(SchedulerError::AccountBlocked(**acc));
                }
            }
        }

        // double check to make sure we can lock this against currently executed transactions and
        // accounts
        Self::can_lock_accounts(account_locks, writable_keys, readonly_keys)?;

        Ok(())
    }

    fn get_scheduled_batch(
        unprocessed_packets: &mut OrderedSkipList<DeserializedPacket>,
        scheduled_accounts: &Arc<Mutex<AccountLocks>>,
        num_txs: usize,
        bank: &Arc<Bank>,
        qos_service: &QosService,
        config: &TransactionSchedulerConfig,
    ) -> Vec<SanitizedTransaction> {
        let mut sanitized_transactions = Vec::with_capacity(num_txs);

        // hashmap representing the highest fee of currently write-locked and read-locked blocked accounts
        // almost a pseudo AccountLocks but fees instead of hashset/read lock count
        let mut highest_wl_blocked_account_fees = HashMap::with_capacity(10_000);
        let mut highest_rl_blocked_account_fees = HashMap::with_capacity(10_000);

        unprocessed_packets.retain(|deserialized_packet| {
            if sanitized_transactions.len() >= num_txs {
                return true; // if fully-scheduled, retain rest of packets
            }

            match Self::try_schedule(
                deserialized_packet.immutable_section(),
                bank,
                &mut highest_wl_blocked_account_fees,
                &mut highest_rl_blocked_account_fees,
                scheduled_accounts,
                qos_service,
                config,
            ) {
                Ok(sanitized_tx) => {
                    // scheduled, drop for now
                    sanitized_transactions.push(sanitized_tx);
                    return false;
                }
                Err(e) => {
                    trace!("e: {:?}", e);
                    match e {
                        SchedulerError::InvalidSanitizedTransaction
                        | SchedulerError::InvalidTransactionFormat(_)
                        | SchedulerError::TransactionCheckFailed(_) => {
                            return false; // non-recoverable error, drop the packet
                        }
                        SchedulerError::AccountInUse
                        | SchedulerError::AccountBlocked(_)
                        | SchedulerError::QosExceeded => {
                            return true; // save these for later
                        }
                    }
                }
            }
        });

        sanitized_transactions
    }

    /// Handles scheduler requests and sends back a response over the channel
    fn handle_scheduler_request(
        unprocessed_packets: &mut OrderedSkipList<DeserializedPacket>,
        scheduled_accounts: &Arc<Mutex<AccountLocks>>,
        scheduler_request: SchedulerRequest,
        qos_service: &QosService,
        config: &TransactionSchedulerConfig,
    ) {
        let response_sender = scheduler_request.response_sender;
        match scheduler_request.msg {
            SchedulerMessage::RequestBatch { num_txs, bank } => {
                trace!("SchedulerMessage::RequestBatch num_txs: {}", num_txs);
                let sanitized_transactions = Self::get_scheduled_batch(
                    unprocessed_packets,
                    scheduled_accounts,
                    num_txs,
                    &bank,
                    qos_service,
                    config,
                );
                trace!(
                    "sanitized_transactions num: {}, unprocessed_packets num: {}",
                    sanitized_transactions.len(),
                    unprocessed_packets.len()
                );

                let _ = response_sender
                    .send(SchedulerResponse::ScheduledBatch(ScheduledBatch {
                        sanitized_transactions,
                    }))
                    .unwrap();
            }
            SchedulerMessage::Ping { id } => {
                let _ = response_sender
                    .send(SchedulerResponse::Pong(Pong { id }))
                    .unwrap();
            }
            SchedulerMessage::ExecutedBatchUpdate {
                executed_transactions,
                rescheduled_transactions,
            } => {
                // respond then do work so execution can get back to doing stuff
                let _ = response_sender
                    .send(SchedulerResponse::ExecutedBatchResponse(
                        ExecutedBatchResponse {},
                    ))
                    .unwrap();

                {
                    // drop account locks for ALL transactions
                    let mut account_locks = scheduled_accounts.lock().unwrap();
                    for tx in executed_transactions
                        .iter()
                        .chain(rescheduled_transactions.iter())
                    {
                        let tx_locks = tx.get_account_locks_unchecked();
                        trace!("unlocking locks: {:?}", tx_locks);
                        Self::drop_account_locks(
                            &mut account_locks,
                            &tx_locks.writable,
                            &tx_locks.readonly,
                        );
                    }
                    trace!("dropped account locks, account_locks: {:?}", account_locks);
                }

                // TODO: update QoS transaction costs for executed transactions
                // need transaction_costs, transactions_qos_results we can cache?
                // commit_transactions_result + bank we need from channel
                // QosService::update_or_remove_transaction_costs(
                //     transaction_costs.iter(),
                //     transactions_qos_results.iter(),
                //     commit_transactions_result.as_ref().ok(),
                //     bank,
                // );
                // let (cu, us) = Self::accumulate_execute_units_and_time(
                //     &execute_and_commit_timings.execute_timings,
                // );
                // qos_service.accumulate_actual_execute_cu(cu);
                // qos_service.accumulate_actual_execute_time(us);
                //
                // // reports qos service stats for this batch
                // qos_service.report_metrics(bank.clone());

                // TODO (LB): reschedule transactions as packet
                // for tx in rescheduled_transactions {
                //     unprocessed_packets.push(tx.deser)
                // }
            }
        }
    }

    fn can_lock_accounts(
        account_locks: &AccountLocks,
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
    ) -> Result<()> {
        for k in writable_keys.iter() {
            if account_locks.is_locked_write(k) || account_locks.is_locked_readonly(k) {
                debug!("Writable account in use: {:?}", k);
                return Err(SchedulerError::AccountInUse);
            }
        }
        for k in readonly_keys.iter() {
            if account_locks.is_locked_write(k) {
                debug!("Read-only account in use: {:?}", k);
                return Err(SchedulerError::AccountInUse);
            }
        }
        Ok(())
    }

    /// NOTE: this is copied from accounts.rs
    fn lock_accounts(
        account_locks: &mut AccountLocks,
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
    ) -> Result<()> {
        Self::can_lock_accounts(account_locks, &writable_keys, &readonly_keys)?;

        for k in writable_keys {
            account_locks.write_locks.insert(**k);
        }

        for k in readonly_keys {
            if !account_locks.lock_readonly(k) {
                account_locks.insert_new_readonly(k);
            }
        }

        Ok(())
    }

    fn drop_account_locks(
        account_locks: &mut AccountLocks,
        writable_keys: &[&Pubkey],
        readonly_keys: &[&Pubkey],
    ) {
        for k in writable_keys {
            account_locks.unlock_write(k);
        }
        for k in readonly_keys {
            account_locks.unlock_readonly(k);
        }
    }

    fn transaction_from_deserialized_packet(
        deserialized_packet: &ImmutableDeserializedPacket,
        feature_set: &Arc<feature_set::FeatureSet>,
        votes_only: bool,
        address_loader: impl AddressLoader,
    ) -> Option<SanitizedTransaction> {
        if votes_only && !deserialized_packet.is_simple_vote() {
            return None;
        }

        let tx = SanitizedTransaction::try_new(
            deserialized_packet.transaction().clone(),
            *deserialized_packet.message_hash(),
            deserialized_packet.is_simple_vote(),
            address_loader,
        )
        .ok()?;
        tx.verify_precompiles(feature_set).ok()?;
        Some(tx)
    }

    fn handle_packet_batches(
        unprocessed_packets: &mut OrderedSkipList<DeserializedPacket>,
        packet_batches: Vec<PacketBatch>,
    ) {
        for packet_batch in packet_batches {
            let packet_indexes: Vec<_> = packet_batch
                .packets
                .iter()
                .enumerate()
                .filter_map(|(idx, p)| if !p.meta.discard() { Some(idx) } else { None })
                .collect();
            unprocessed_packets.extend(unprocessed_packet_batches::deserialize_packets(
                &packet_batch,
                &packet_indexes,
            ));
        }
    }

    /// Returns sending side of the channel given the scheduler stage
    fn get_sender_from_stage(&self, stage: SchedulerStage) -> &Sender<SchedulerRequest> {
        match stage {
            SchedulerStage::Transactions => &self.tx_scheduler_request_sender,
            SchedulerStage::TpuVotes => &self.tpu_vote_scheduler_request_sender,
            SchedulerStage::GossipVotes => &self.gossip_vote_scheduler_request_sender,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::transaction_scheduler::{
            SchedulerStage, TransactionScheduler, TransactionSchedulerConfig,
        },
        crossbeam_channel::{unbounded, Sender},
        solana_ledger::genesis_utils::create_genesis_config,
        solana_perf::packet::PacketBatch,
        solana_runtime::{
            bank::Bank, bank_forks::BankForks, cost_model::CostModel,
            genesis_utils::GenesisConfigInfo,
        },
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            genesis_config::GenesisConfig,
            hash::Hash,
            instruction::{AccountMeta, Instruction},
            packet::Packet,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_program,
            transaction::Transaction,
        },
        std::{
            collections::HashMap,
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, RwLock,
            },
        },
    };

    struct TestHarness {
        tx_sender: Sender<Vec<PacketBatch>>,
        tpu_vote_sender: Sender<Vec<PacketBatch>>,
        gossip_vote_sender: Sender<Vec<PacketBatch>>,
        exit: Arc<AtomicBool>,
        scheduler: TransactionScheduler,
        accounts: HashMap<&'static str, Pubkey>,
        #[allow(unused)]
        genesis_config: GenesisConfig,
        bank: Arc<Bank>,
    }

    fn get_test_harness() -> TestHarness {
        let (tx_sender, tx_receiver) = unbounded();
        let (tpu_vote_sender, tpu_vote_receiver) = unbounded();
        let (gossip_vote_sender, gossip_vote_receiver) = unbounded();
        let exit = Arc::new(AtomicBool::new(false));
        let cost_model = Arc::new(RwLock::new(CostModel::default()));
        let accounts = get_random_accounts();

        let scheduler = TransactionScheduler::new(
            tx_receiver,
            tpu_vote_receiver,
            gossip_vote_receiver,
            exit.clone(),
            cost_model,
            TransactionSchedulerConfig {
                enable_state_auction: true,
                backlog_size: 700_000,
            },
        );

        let mint_total = 1_000_000_000_000;
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(mint_total);

        let bank0 = Bank::new_for_benches(&genesis_config);
        let bank_forks = BankForks::new(bank0);
        let bank = bank_forks.working_bank();
        bank.write_cost_tracker()
            .unwrap()
            .set_limits(u64::MAX, u64::MAX, u64::MAX);

        TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit,
            scheduler,
            accounts,
            genesis_config,
            bank,
        }
    }

    #[test]
    fn test_start_and_join_channel_dropped() {
        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            scheduler,
            ..
        } = get_test_harness();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);
        let gossip_vote_handle = scheduler.get_handle(SchedulerStage::GossipVotes);
        let tpu_vote_handle = scheduler.get_handle(SchedulerStage::TpuVotes);

        // check alive
        assert_eq!(tx_handle.send_ping(1).unwrap().id, 1);
        assert_eq!(gossip_vote_handle.send_ping(2).unwrap().id, 2);
        assert_eq!(tpu_vote_handle.send_ping(3).unwrap().id, 3);

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);

        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_start_and_join_channel_exit_signal() {
        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit,
            scheduler,
            ..
        } = get_test_harness();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);
        let gossip_vote_handle = scheduler.get_handle(SchedulerStage::GossipVotes);
        let tpu_vote_handle = scheduler.get_handle(SchedulerStage::TpuVotes);

        // check alive
        assert_eq!(tx_handle.send_ping(1).unwrap().id, 1);
        assert_eq!(gossip_vote_handle.send_ping(2).unwrap().id, 2);
        assert_eq!(tpu_vote_handle.send_ping(3).unwrap().id, 3);

        exit.store(true, Ordering::Relaxed);

        assert_matches!(scheduler.join(), Ok(()));
        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
    }

    #[test]
    fn test_single_tx() {
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        // main logic
        {
            let tx = get_tx(&[get_account_meta(&accounts, "A", true)], 200, &blockhash);

            send_transactions(&[&tx], &tx_sender);

            // should probably have gotten the packet by now
            let _ = tx_handle.send_ping(1).unwrap();

            // make sure the requested batch is the single packet
            let mut batch = tx_handle.request_batch(1, &bank).unwrap();
            assert_eq!(batch.sanitized_transactions.len(), 1);
            assert_eq!(
                batch.sanitized_transactions.pop().unwrap().signature(),
                &tx.signatures[0]
            );

            // make sure the batch is unlocked
            let _ = tx_handle.send_batch_execution_update(batch.sanitized_transactions, vec![]);
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_conflicting_transactions() {
        const BATCH_SIZE: usize = 128;
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        // main logic
        {
            let tx1 = get_tx(&[get_account_meta(&accounts, "A", true)], 200, &blockhash);
            let tx2 = get_tx(&[get_account_meta(&accounts, "A", true)], 250, &blockhash);

            send_transactions(&[&tx1, &tx2], &tx_sender);

            // should probably have gotten the packet by now
            let _ = tx_handle.send_ping(1).unwrap();

            // request two transactions, tx2 should be scheduled because it has higher fee for account A
            let first_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(first_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx2.signatures[0]
            );

            // attempt to request another transaction for schedule, won't schedule bc tx2 locked account A
            assert_eq!(
                tx_handle
                    .request_batch(BATCH_SIZE, &bank)
                    .unwrap()
                    .sanitized_transactions
                    .len(),
                0
            );

            // make sure the tx2 is unlocked by sending it execution results of that batch
            let _ = tx_handle
                .send_batch_execution_update(first_batch.sanitized_transactions, vec![])
                .unwrap();

            // tx1 should schedule now that tx2 is done executing
            let second_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(second_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx1.signatures[0]
            );
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_blocked_transactions() {
        const BATCH_SIZE: usize = 128;
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        // main logic
        {
            // 300: A, B(RO)
            // 200:    B,     C (RO), D (RO)
            // 100:    B(R0), C (RO), D (RO), E
            // under previous logic, the previous batches would be [(300, 100), (200)] bc 300 and 100 can be parallelized
            // under this logic, we expect [(300), (200), (100)]
            // 200 has write priority on B
            let tx1 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", false),
                ],
                300,
                &blockhash,
            );
            let tx2 = get_tx(
                &[
                    get_account_meta(&accounts, "B", true),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                ],
                200,
                &blockhash,
            );
            let tx3 = get_tx(
                &[
                    get_account_meta(&accounts, "B", false),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                    get_account_meta(&accounts, "E", true),
                ],
                100,
                &blockhash,
            );

            send_transactions(&[&tx1, &tx2, &tx3], &tx_sender);

            // should probably have gotten the packet by now
            let _ = tx_handle.send_ping(1).unwrap();

            let first_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(first_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx1.signatures[0]
            );

            // attempt to request another transaction for schedule, won't schedule bc tx2 locked account A
            assert_eq!(
                tx_handle
                    .request_batch(BATCH_SIZE, &bank)
                    .unwrap()
                    .sanitized_transactions
                    .len(),
                0
            );

            let _ = tx_handle
                .send_batch_execution_update(first_batch.sanitized_transactions, vec![])
                .unwrap();

            let second_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(second_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx2.signatures[0]
            );

            let _ = tx_handle
                .send_batch_execution_update(second_batch.sanitized_transactions, vec![])
                .unwrap();

            let third_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(third_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                third_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx3.signatures[0]
            );
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_blocked_transactions_read_locked() {
        const BATCH_SIZE: usize = 128;
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        // main logic
        {
            // 300: A, B(RO)
            // 200: A, B(R0), C (RO), D (RO)
            // 100:    B(R0), C (RO), D (RO), E
            // should schedule as [(300, 100), (200)] because while 200 is blocked on 300 bc account A, it read-locks B so the ordering
            // doesn't matter on 200, 100 or 100, 200
            let tx1 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", false),
                ],
                300,
                &blockhash,
            );
            let tx2 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", false),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                ],
                200,
                &blockhash,
            );
            let tx3 = get_tx(
                &[
                    get_account_meta(&accounts, "B", false),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                    get_account_meta(&accounts, "E", true),
                ],
                100,
                &blockhash,
            );

            send_transactions(&[&tx1, &tx2, &tx3], &tx_sender);

            // should probably have gotten the packet by now
            let _ = tx_handle.send_ping(1).unwrap();

            let first_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(first_batch.sanitized_transactions.len(), 2);
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx1.signatures[0]
            );
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(1)
                    .unwrap()
                    .signature(),
                &tx3.signatures[0]
            );

            // attempt to request another transaction for schedule, won't schedule bc tx2 locked account A
            assert_eq!(
                tx_handle
                    .request_batch(BATCH_SIZE, &bank)
                    .unwrap()
                    .sanitized_transactions
                    .len(),
                0
            );

            let _ = tx_handle
                .send_batch_execution_update(first_batch.sanitized_transactions, vec![])
                .unwrap();

            let second_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(second_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx2.signatures[0]
            );

            let _ = tx_handle
                .send_batch_execution_update(second_batch.sanitized_transactions, vec![])
                .unwrap();
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_blocked_transactions_write_lock_released() {
        const BATCH_SIZE: usize = 128;
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        // main logic
        {
            // 300: A, B(RO)
            // 200: A, B(R0), C (RO), D (RO)
            // 100:    B(R0), C (RO), D (RO), E
            //  50: A, B(R0), C (RO), D (RO), E
            // should schedule as [(300, 100), (200), (50)]
            let tx1 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", false),
                ],
                300,
                &blockhash,
            );
            let tx2 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", false),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                ],
                200,
                &blockhash,
            );
            let tx3 = get_tx(
                &[
                    get_account_meta(&accounts, "B", false),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                    get_account_meta(&accounts, "E", true),
                ],
                100,
                &blockhash,
            );
            let tx4 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", false),
                    get_account_meta(&accounts, "C", false),
                    get_account_meta(&accounts, "D", false),
                    get_account_meta(&accounts, "E", true),
                ],
                50,
                &blockhash,
            );

            send_transactions(&[&tx1, &tx2, &tx3, &tx4], &tx_sender);

            // should probably have gotten the packet by now
            let _ = tx_handle.send_ping(1);

            let first_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(first_batch.sanitized_transactions.len(), 2);
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx1.signatures[0]
            );
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(1)
                    .unwrap()
                    .signature(),
                &tx3.signatures[0]
            );

            // attempt to request another transaction for schedule, won't schedule bc tx2 locked account A
            assert_eq!(
                tx_handle
                    .request_batch(BATCH_SIZE, &bank)
                    .unwrap()
                    .sanitized_transactions
                    .len(),
                0
            );

            let _ = tx_handle
                .send_batch_execution_update(first_batch.sanitized_transactions, vec![])
                .unwrap();

            let second_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(second_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx2.signatures[0]
            );

            let _ = tx_handle
                .send_batch_execution_update(second_batch.sanitized_transactions, vec![])
                .unwrap();

            let third_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(third_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                third_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx4.signatures[0]
            );

            let _ = tx_handle
                .send_batch_execution_update(third_batch.sanitized_transactions, vec![])
                .unwrap();
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_read_locked_blocks_write() {
        const BATCH_SIZE: usize = 128;
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        {
            // 300: A, B, C
            // 200:    B, C, D(RO)
            // 100:        , D,     E
            //  50:                 E(RO), F
            // request schedule: 300
            // return 300
            // request schedule: 200
            // return 200
            // request schedule: 100
            // return schedule
            // request schedule: 50
            // return schedule
            let tx1 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", true),
                    get_account_meta(&accounts, "C", true),
                ],
                300,
                &blockhash,
            );
            let tx2 = get_tx(
                &[
                    get_account_meta(&accounts, "B", true),
                    get_account_meta(&accounts, "C", true),
                    get_account_meta(&accounts, "D", false),
                ],
                200,
                &blockhash,
            );
            let tx3 = get_tx(
                &[
                    get_account_meta(&accounts, "D", true),
                    get_account_meta(&accounts, "E", true),
                ],
                100,
                &blockhash,
            );
            let tx4 = get_tx(
                &[
                    get_account_meta(&accounts, "E", false),
                    get_account_meta(&accounts, "F", true),
                ],
                50,
                &blockhash,
            );

            send_transactions(&[&tx1, &tx2, &tx3, &tx4], &tx_sender);

            let first_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(first_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx1.signatures[0]
            );
            let _ = tx_handle
                .send_batch_execution_update(first_batch.sanitized_transactions, vec![])
                .unwrap();

            let second_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(second_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx2.signatures[0]
            );
            let _ = tx_handle
                .send_batch_execution_update(second_batch.sanitized_transactions, vec![])
                .unwrap();

            let third_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(third_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                third_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx3.signatures[0]
            );
            let _ = tx_handle
                .send_batch_execution_update(third_batch.sanitized_transactions, vec![])
                .unwrap();

            let fourth_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(fourth_batch.sanitized_transactions.len(), 1);
            assert_eq!(
                fourth_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx4.signatures[0]
            );
            let _ = tx_handle
                .send_batch_execution_update(fourth_batch.sanitized_transactions, vec![])
                .unwrap();
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    #[test]
    fn test_read_locked_does_not_block_red() {
        const BATCH_SIZE: usize = 128;
        solana_logger::setup_with_default("solana_core::transaction_scheduler=trace");

        let TestHarness {
            tx_sender,
            tpu_vote_sender,
            gossip_vote_sender,
            exit: _,
            scheduler,
            accounts,
            genesis_config: _,
            bank,
        } = get_test_harness();
        let blockhash = bank.last_blockhash();

        let tx_handle = scheduler.get_handle(SchedulerStage::Transactions);

        {
            // 300: A, B, C
            // 200:    B, C, D(RO)
            // 100:          D(RO), E
            //  50:                 E(RO), F
            // request schedule: 300, 100
            // return 300, 100
            // request schedule: 200, 50
            // return 200, 50
            let tx1 = get_tx(
                &[
                    get_account_meta(&accounts, "A", true),
                    get_account_meta(&accounts, "B", true),
                    get_account_meta(&accounts, "C", true),
                ],
                300,
                &blockhash,
            );
            let tx2 = get_tx(
                &[
                    get_account_meta(&accounts, "B", true),
                    get_account_meta(&accounts, "C", true),
                    get_account_meta(&accounts, "D", false),
                ],
                200,
                &blockhash,
            );
            let tx3 = get_tx(
                &[
                    get_account_meta(&accounts, "D", false),
                    get_account_meta(&accounts, "E", true),
                ],
                100,
                &blockhash,
            );
            let tx4 = get_tx(
                &[
                    get_account_meta(&accounts, "E", false),
                    get_account_meta(&accounts, "F", true),
                ],
                50,
                &blockhash,
            );

            send_transactions(&[&tx1, &tx2, &tx3, &tx4], &tx_sender);

            let first_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(first_batch.sanitized_transactions.len(), 2);
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx1.signatures[0]
            );
            assert_eq!(
                first_batch
                    .sanitized_transactions
                    .get(1)
                    .unwrap()
                    .signature(),
                &tx3.signatures[0]
            );
            let _ = tx_handle
                .send_batch_execution_update(first_batch.sanitized_transactions, vec![])
                .unwrap();

            let second_batch = tx_handle.request_batch(BATCH_SIZE, &bank).unwrap();
            assert_eq!(second_batch.sanitized_transactions.len(), 2);
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(0)
                    .unwrap()
                    .signature(),
                &tx2.signatures[0]
            );
            assert_eq!(
                second_batch
                    .sanitized_transactions
                    .get(1)
                    .unwrap()
                    .signature(),
                &tx4.signatures[0]
            );
            let _ = tx_handle
                .send_batch_execution_update(second_batch.sanitized_transactions, vec![])
                .unwrap();
        }

        drop(tx_sender);
        drop(tpu_vote_sender);
        drop(gossip_vote_sender);
        assert_matches!(scheduler.join(), Ok(()));
    }

    // TODO: need to think of a clear and concise way to test this scheduler!

    // TODO some other tests:
    // 300: A(R), B, C
    // 250: A(R), B, C
    // 200: A(R), D, E
    // request schedule: (300, 200)
    // return (300, 200)
    // request schedule: (250)
    // return 250
    // -----------------------
    // 300: A(R), B, C,
    // 250: A,    B, C,
    // 200: A(R),       D, E
    // request schedule: 300
    // return 300
    // request schedule: 250
    // return 250
    // request schedule: 200
    // return 200
    // -----------------------
    // 300: A(R), B, C,
    // 250: A,    B, C,
    // 200: A(R),       D, E
    // request schedule: 300
    // insert:
    // 275: A(R),       D(R)
    // request schedule: 275
    // request schedule: []
    // return 300
    // request schedule: 250
    // return 275
    // return 250
    // request schedule: 200
    // return 200

    /// Converts transactions to packets and sends them to scheduler over channel
    fn send_transactions(txs: &[&Transaction], tx_sender: &Sender<Vec<PacketBatch>>) {
        let packets = txs
            .into_iter()
            .map(|tx| Packet::from_data(None, *tx).unwrap());
        tx_sender
            .send(vec![PacketBatch::new(packets.collect())])
            .unwrap();
    }

    /// Builds some arbitrary transaction with given AccountMetas and prioritization fee
    fn get_tx(
        account_metas: &[AccountMeta],
        micro_lamports_fee_per_cu: u64,
        blockhash: &Hash,
    ) -> Transaction {
        let kp = Keypair::new();
        Transaction::new_signed_with_payer(
            &[
                ComputeBudgetInstruction::set_compute_unit_price(micro_lamports_fee_per_cu),
                Instruction::new_with_bytes(system_program::id(), &[0], account_metas.to_vec()),
            ],
            Some(&kp.pubkey()),
            &[&kp],
            blockhash.clone(),
        )
    }

    /// Gets random accounts w/ alphabetical access for easy testing.
    fn get_random_accounts() -> HashMap<&'static str, Pubkey> {
        HashMap::from([
            ("A", Pubkey::new_unique()),
            ("B", Pubkey::new_unique()),
            ("C", Pubkey::new_unique()),
            ("D", Pubkey::new_unique()),
            ("E", Pubkey::new_unique()),
            ("F", Pubkey::new_unique()),
            ("G", Pubkey::new_unique()),
            ("H", Pubkey::new_unique()),
            ("I", Pubkey::new_unique()),
            ("J", Pubkey::new_unique()),
            ("K", Pubkey::new_unique()),
            ("L", Pubkey::new_unique()),
            ("M", Pubkey::new_unique()),
            ("N", Pubkey::new_unique()),
            ("O", Pubkey::new_unique()),
            ("P", Pubkey::new_unique()),
            ("Q", Pubkey::new_unique()),
            ("R", Pubkey::new_unique()),
            ("S", Pubkey::new_unique()),
            ("T", Pubkey::new_unique()),
            ("U", Pubkey::new_unique()),
            ("V", Pubkey::new_unique()),
            ("W", Pubkey::new_unique()),
            ("X", Pubkey::new_unique()),
            ("Y", Pubkey::new_unique()),
            ("Z", Pubkey::new_unique()),
        ])
    }

    /// Returns pubkey from map created above
    fn get_pubkey(map: &HashMap<&str, Pubkey>, char: &str) -> Pubkey {
        return map.get(char).unwrap().clone();
    }

    /// Returns AccountMeta with pubkey from account above and writeable flag set
    fn get_account_meta(map: &HashMap<&str, Pubkey>, char: &str, is_writable: bool) -> AccountMeta {
        if is_writable {
            AccountMeta::new(get_pubkey(map, char), false)
        } else {
            AccountMeta::new_readonly(get_pubkey(map, char), false)
        }
    }
}
