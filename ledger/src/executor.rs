use {
    crate::{
        blockstore_processor::{BlockCostCapacityMeter, TransactionStatusSender},
        token_balances::collect_token_balances,
    },
    crossbeam_channel::{unbounded, Receiver, RecvError, SendError, Sender},
    rayon::{
        iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator},
        ThreadPool,
    },
    solana_program_runtime::timings::ExecuteTimings,
    solana_rayon_threadlimit::get_max_thread_count,
    solana_runtime::{
        bank::{Bank, TransactionResults},
        bank_utils,
        transaction_batch::TransactionBatch,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        clock::MAX_PROCESSING_AGE,
        feature_set,
        pubkey::Pubkey,
        signature::Signature,
        transaction::{Result, SanitizedTransaction, TransactionAccountLocks, TransactionError},
    },
    solana_transaction_status::token_balances::TransactionTokenBalancesSet,
    std::{
        borrow::Cow,
        collections::{HashMap, HashSet},
        result,
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
};

pub(crate) struct TransactionBatchWithIndexes<'a, 'b> {
    pub batch: TransactionBatch<'a, 'b>,
    pub transaction_indexes: Vec<usize>,
}

/// Callback for accessing bank state while processing the blockstore
pub type ProcessCallback = Arc<dyn Fn(&Bank) + Sync + Send>;

// get_max_thread_count to match number of threads in the old code.
// see: https://github.com/solana-labs/solana/pull/24853
lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_max_thread_count())
        .thread_name(|ix| format!("solBstoreProc{:02}", ix))
        .build()
        .unwrap();
}

const DEFAULT_CONFLICT_SET_SIZE: usize = 30;

pub struct ReplayResponse {
    pub result: Result<()>,
    pub timing: ExecuteTimings,
    pub batch_idx: Option<usize>,
}

/// Request for replay, sends responses back on this channel
pub struct ReplayRequest {
    pub tx_and_idx: (SanitizedTransaction, usize),
    pub bank: Arc<Bank>,
    pub transaction_status_sender: Option<TransactionStatusSender>,
    pub replay_vote_sender: Option<ReplayVoteSender>,
    pub cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    pub tx_cost: u64,
    pub log_messages_bytes_limit: Option<usize>,
    pub entry_callback: Option<ProcessCallback>,
    pub batch_idx: Option<usize>,
}

pub struct Replayer {
    threads: Vec<JoinHandle<()>>,
    request_sender: Sender<(Sender<ReplayResponse>, ReplayRequest)>,
}

pub struct ReplayerHandle {
    request_sender: Sender<(Sender<ReplayResponse>, ReplayRequest)>,
    response_sender: Sender<ReplayResponse>,
    response_receiver: Receiver<ReplayResponse>,
}

/// A handle to the replayer. Each replayer handle has a separate channel that responses are sent on.
/// This means multiple threads can have a handle to the replayer and results are sent back to the
/// correct replayer handle each time.
impl ReplayerHandle {
    pub fn new(
        request_sender: Sender<(Sender<ReplayResponse>, ReplayRequest)>,
        response_sender: Sender<ReplayResponse>,
        response_receiver: Receiver<ReplayResponse>,
    ) -> ReplayerHandle {
        ReplayerHandle {
            request_sender,
            response_sender,
            response_receiver,
        }
    }

    pub fn send(
        &self,
        request: ReplayRequest,
    ) -> result::Result<(), SendError<(Sender<ReplayResponse>, ReplayRequest)>> {
        self.request_sender
            .send((self.response_sender.clone(), request))
    }

    pub fn recv_and_drain(&self) -> result::Result<Vec<ReplayResponse>, RecvError> {
        let mut results = vec![self.response_receiver.recv()?];
        results.extend(self.response_receiver.try_iter());
        Ok(results)
    }
}

impl Replayer {
    pub fn new(num_threads: usize) -> Replayer {
        let (request_sender, request_receiver) = unbounded();
        let threads = Self::start_replay_threads(num_threads, request_receiver);
        Replayer {
            threads,
            request_sender,
        }
    }

    pub fn handle(&self) -> ReplayerHandle {
        let (response_sender, response_receiver) = unbounded();
        ReplayerHandle::new(
            self.request_sender.clone(),
            response_sender,
            response_receiver,
        )
    }

    pub fn start_replay_threads(
        num_threads: usize,
        request_receiver: Receiver<(Sender<ReplayResponse>, ReplayRequest)>,
    ) -> Vec<JoinHandle<()>> {
        (0..num_threads)
            .map(|i| {
                let request_receiver = request_receiver.clone();
                Builder::new()
                    .name(format!("solReplayer-{}", i))
                    .spawn(move || loop {
                        match request_receiver.recv() {
                            Ok((
                                response_sender,
                                ReplayRequest {
                                    tx_and_idx,
                                    bank,
                                    transaction_status_sender,
                                    replay_vote_sender,
                                    cost_capacity_meter,
                                    tx_cost,
                                    log_messages_bytes_limit,
                                    entry_callback,
                                    batch_idx,
                                },
                            )) => {
                                let mut timings = ExecuteTimings::default();

                                let txs = vec![tx_and_idx.0];
                                let mut batch =
                                    TransactionBatch::new(vec![Ok(())], &bank, Cow::Borrowed(&txs));
                                batch.set_needs_unlock(false);
                                let batch_with_idx = TransactionBatchWithIndexes {
                                    batch,
                                    transaction_indexes: vec![tx_and_idx.1],
                                };

                                let result = execute_batch(
                                    &batch_with_idx,
                                    &bank,
                                    transaction_status_sender.as_ref(),
                                    replay_vote_sender.as_ref(),
                                    &mut timings,
                                    cost_capacity_meter,
                                    tx_cost,
                                    log_messages_bytes_limit,
                                );

                                if let Some(entry_callback) = entry_callback {
                                    entry_callback(&bank);
                                }

                                if response_sender
                                    .send(ReplayResponse {
                                        result,
                                        timing: timings,
                                        batch_idx,
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(_) => {
                                return;
                            }
                        }
                    })
                    .unwrap()
            })
            .collect()
    }

    pub fn join(self) -> thread::Result<()> {
        for t in self.threads {
            t.join()?;
        }
        Ok(())
    }
}

fn aggregate_total_execution_units(execute_timings: &ExecuteTimings) -> u64 {
    let mut execute_cost_units: u64 = 0;
    for (program_id, timing) in &execute_timings.details.per_program_timings {
        if timing.count < 1 {
            continue;
        }
        execute_cost_units =
            execute_cost_units.saturating_add(timing.accumulated_units / timing.count as u64);
        trace!("aggregated execution cost of {:?} {:?}", program_id, timing);
    }
    execute_cost_units
}

// Includes transaction signature for unit-testing
fn get_first_error(
    batch: &TransactionBatch,
    fee_collection_results: Vec<Result<()>>,
) -> Option<(Result<()>, Signature)> {
    let mut first_err = None;
    for (result, transaction) in fee_collection_results
        .iter()
        .zip(batch.sanitized_transactions())
    {
        if let Err(ref err) = result {
            if first_err.is_none() {
                first_err = Some((result.clone(), *transaction.signature()));
            }
            warn!(
                "Unexpected validator error: {:?}, transaction: {:?}",
                err, transaction
            );
            datapoint_error!(
                "validator_process_entry_error",
                (
                    "error",
                    format!("error: {:?}, transaction: {:?}", err, transaction),
                    String
                )
            );
        }
    }
    first_err
}

fn execute_batch(
    batch: &TransactionBatchWithIndexes,
    bank: &Arc<Bank>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timings: &mut ExecuteTimings,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    tx_cost: u64,
    log_messages_bytes_limit: Option<usize>,
) -> Result<()> {
    let TransactionBatchWithIndexes {
        batch,
        transaction_indexes,
    } = batch;
    let record_token_balances = transaction_status_sender.is_some();

    let mut mint_decimals: HashMap<Pubkey, u8> = HashMap::new();

    let pre_token_balances = if record_token_balances {
        collect_token_balances(bank, batch, &mut mint_decimals)
    } else {
        vec![]
    };

    let pre_process_units: u64 = aggregate_total_execution_units(timings);

    let (tx_results, balances) = batch.bank().load_execute_and_commit_transactions(
        batch,
        MAX_PROCESSING_AGE,
        transaction_status_sender.is_some(),
        transaction_status_sender.is_some(),
        transaction_status_sender.is_some(),
        transaction_status_sender.is_some(),
        timings,
        log_messages_bytes_limit,
    );

    if bank
        .feature_set
        .is_active(&feature_set::gate_large_block::id())
    {
        let execution_cost_units = aggregate_total_execution_units(timings) - pre_process_units;
        let remaining_block_cost_cap = cost_capacity_meter
            .write()
            .unwrap()
            .accumulate(execution_cost_units + tx_cost);

        debug!(
            "bank {} executed a batch, number of transactions {}, total execute cu {}, total additional cu {}, remaining block cost cap {}",
            bank.slot(),
            batch.sanitized_transactions().len(),
            execution_cost_units,
            tx_cost,
            remaining_block_cost_cap,
        );

        if remaining_block_cost_cap == 0_u64 {
            return Err(TransactionError::WouldExceedMaxBlockCostLimit);
        }
    }

    bank_utils::find_and_send_votes(
        batch.sanitized_transactions(),
        &tx_results,
        replay_vote_sender,
    );

    let TransactionResults {
        fee_collection_results,
        execution_results,
        rent_debits,
        ..
    } = tx_results;

    if let Some(transaction_status_sender) = transaction_status_sender {
        let transactions = batch.sanitized_transactions().to_vec();
        let post_token_balances = if record_token_balances {
            collect_token_balances(bank, batch, &mut mint_decimals)
        } else {
            vec![]
        };

        let token_balances =
            TransactionTokenBalancesSet::new(pre_token_balances, post_token_balances);

        transaction_status_sender.send_transaction_status_batch(
            bank.clone(),
            transactions,
            execution_results,
            balances,
            token_balances,
            rent_debits,
            transaction_indexes.to_vec(),
        );
    }
    let first_err = get_first_error(batch, fee_collection_results);
    first_err.map(|(result, _)| result).unwrap_or(Ok(()))
}

// The dependency graph contains a set of indices < N that must be executed before index N.
pub fn build_dependency_graph(
    tx_account_locks_results: &Vec<Result<TransactionAccountLocks>>,
) -> Result<Vec<HashSet<usize>>> {
    if let Some(err) = tx_account_locks_results.iter().find(|r| r.is_err()) {
        err.clone()?;
    }
    let transaction_locks: Vec<_> = tx_account_locks_results
        .iter()
        .map(|r| r.as_ref().unwrap())
        .collect();

    // build a map whose key is a pubkey + value is a sorted vector of all indices that
    // lock that account
    let mut indices_read_locking_account = HashMap::new();
    let mut indicies_write_locking_account = HashMap::new();
    transaction_locks
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

    Ok(PAR_THREAD_POOL.install(|| {
        transaction_locks
            .par_iter()
            .enumerate()
            .map(|(idx, account_locks)| {
                // user measured value from mainnet; rarely see more than 30 conflicts or so
                let mut dep_graph = HashSet::with_capacity(DEFAULT_CONFLICT_SET_SIZE);
                let readlock_accs = account_locks.writable.iter();
                let writelock_accs = account_locks
                    .readonly
                    .iter()
                    .chain(account_locks.writable.iter());

                for acc in readlock_accs {
                    if let Some(indices) = indices_read_locking_account.get(acc) {
                        dep_graph.extend(indices.iter().take_while(|l_idx| **l_idx < idx));
                    }
                }

                for read_acc in writelock_accs {
                    if let Some(indices) = indicies_write_locking_account.get(read_acc) {
                        dep_graph.extend(indices.iter().take_while(|l_idx| **l_idx < idx));
                    }
                }
                dep_graph
            })
            .collect()
    }))
}
