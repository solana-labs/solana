use {
    crate::{
        blockstore_processor::{BlockCostCapacityMeter, ProcessCallback, TransactionStatusSender},
        token_balances::collect_token_balances,
    },
    crossbeam_channel::{unbounded, Receiver, Sender},
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        bank::{Bank, TransactionExecutionResult, TransactionResults},
        bank_utils,
        block_cost_limits::MAX_ACCOUNT_DATA_BLOCK_LEN,
        transaction_batch::TransactionBatch,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        clock::MAX_PROCESSING_AGE,
        feature_set,
        instruction::InstructionError,
        pubkey::Pubkey,
        signature::Signature,
        transaction::{self, SanitizedTransaction, TransactionError},
    },
    solana_transaction_status::token_balances::TransactionTokenBalancesSet,
    std::{
        borrow::Cow,
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
    },
};

#[derive(Debug)]
pub enum ReplayerHandleError {
    Disconnected,
}

pub type ReplayerHandleResult<T> = Result<T, ReplayerHandleError>;
type RequestSender = Sender<(Sender<ReplayResponse>, ReplayRequest)>;
type RequestReceiver = Receiver<(Sender<ReplayResponse>, ReplayRequest)>;

pub struct ReplayResponse {
    pub result: transaction::Result<()>,
    pub timings: ExecuteTimings,
}

/// Request for replay, sends responses back on this channel
pub struct ReplayRequest {
    pub bank: Arc<Bank>,
    pub tx: SanitizedTransaction,
    pub transaction_status_sender: Option<TransactionStatusSender>,
    pub replay_vote_sender: Option<ReplayVoteSender>,
    pub cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    pub entry_callback: Option<ProcessCallback>,
}

pub struct ReplayerHandle {
    request_sender: RequestSender,
}

impl ReplayerHandle {
    pub fn new(request_sender: RequestSender) -> ReplayerHandle {
        ReplayerHandle { request_sender }
    }

    pub fn send(&self, request: ReplayRequest) -> ReplayerHandleResult<Receiver<ReplayResponse>> {
        let (request_sender, request_receiver) = unbounded();
        self.request_sender
            .send((request_sender, request))
            .map_err(|_| ReplayerHandleError::Disconnected)?;
        Ok(request_receiver)
    }
}

pub struct Replayer {
    request_sender: RequestSender,
    threads: Vec<JoinHandle<()>>,
}

impl Replayer {
    pub fn new(num_threads: usize, exit: &Arc<AtomicBool>) -> Replayer {
        let (request_sender, request_receiver) = unbounded();
        let threads = Self::start_replay_threads(num_threads, request_receiver, exit);
        Replayer {
            request_sender,
            threads,
        }
    }

    pub fn handle(&self) -> ReplayerHandle {
        ReplayerHandle {
            request_sender: self.request_sender.clone(),
        }
    }

    pub fn start_replay_threads(
        num_threads: usize,
        request_receiver: RequestReceiver,
        exit: &Arc<AtomicBool>,
    ) -> Vec<JoinHandle<()>> {
        (0..num_threads)
            .map(|i| {
                let request_receiver = request_receiver.clone();
                let exit = exit.clone();
                Builder::new()
                    .name(format!("solReplayer-{}", i))
                    .spawn(move || {
                        info!("started replayer");
                        while !exit.load(Ordering::Relaxed) {
                            match request_receiver.recv() {
                                Ok((
                                    response_sender,
                                    ReplayRequest {
                                        bank,
                                        tx,
                                        transaction_status_sender,
                                        replay_vote_sender,
                                        cost_capacity_meter,
                                        entry_callback,
                                    },
                                )) => {
                                    // info!("got replay request");
                                    let mut timings = ExecuteTimings::default();

                                    let batch = TransactionBatch::new(
                                        vec![Ok(())],
                                        &bank,
                                        Cow::Owned(vec![tx]),
                                    );
                                    let result = execute_batch(
                                        &batch,
                                        &bank,
                                        transaction_status_sender.as_ref(),
                                        replay_vote_sender.as_ref(),
                                        &mut timings,
                                        cost_capacity_meter.clone(),
                                    );

                                    if let Some(entry_callback) = entry_callback {
                                        entry_callback(&bank);
                                    }

                                    // info!("sending response");
                                    if response_sender
                                        .send(ReplayResponse { result, timings })
                                        .is_err()
                                    {
                                        warn!("response_sender disconnected");
                                    }
                                }
                                Err(_) => {
                                    return;
                                }
                            }
                        }
                        info!("stopped replayer");
                    })
                    .unwrap()
            })
            .collect()
    }

    pub fn join(mut self) -> thread::Result<()> {
        info!("joining relayer");
        drop(self.request_sender);
        info!("dropped sender");
        for t in self.threads {
            t.join()?;
        }
        info!("joined threads");
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

fn execute_batch(
    batch: &TransactionBatch,
    bank: &Arc<Bank>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timings: &mut ExecuteTimings,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
) -> transaction::Result<()> {
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
        timings,
    );

    if bank
        .feature_set
        .is_active(&feature_set::gate_large_block::id())
    {
        let execution_cost_units = aggregate_total_execution_units(timings) - pre_process_units;
        let remaining_block_cost_cap = cost_capacity_meter
            .write()
            .unwrap()
            .accumulate(execution_cost_units);

        debug!(
            "bank {} executed a batch, number of transactions {}, total execute cu {}, remaining block cost cap {}",
            bank.slot(),
            batch.sanitized_transactions().len(),
            execution_cost_units,
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

    check_accounts_data_size(bank, &execution_results)?;

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
        );
    }

    let first_err = get_first_error(batch, fee_collection_results);
    first_err.map(|(result, _)| result).unwrap_or(Ok(()))
}

// Includes transaction signature for unit-testing
fn get_first_error(
    batch: &TransactionBatch,
    fee_collection_results: Vec<transaction::Result<()>>,
) -> Option<(transaction::Result<()>, Signature)> {
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

/// Check to see if the transactions exceeded the accounts data size limits
fn check_accounts_data_size<'a>(
    bank: &Bank,
    execution_results: impl IntoIterator<Item = &'a TransactionExecutionResult>,
) -> transaction::Result<()> {
    check_accounts_data_block_size(bank)?;
    check_accounts_data_total_size(bank, execution_results)
}

/// Check to see if transactions exceeded the accounts data size limit per block
fn check_accounts_data_block_size(bank: &Bank) -> transaction::Result<()> {
    if !bank
        .feature_set
        .is_active(&feature_set::cap_accounts_data_size_per_block::id())
    {
        return Ok(());
    }

    debug_assert!(MAX_ACCOUNT_DATA_BLOCK_LEN <= i64::MAX as u64);
    if bank.load_accounts_data_size_delta_on_chain() > MAX_ACCOUNT_DATA_BLOCK_LEN as i64 {
        Err(TransactionError::WouldExceedAccountDataBlockLimit)
    } else {
        Ok(())
    }
}

/// Check the transaction execution results to see if any instruction errored by exceeding the max
/// accounts data size limit for all slots.  If yes, the whole block needs to be failed.
fn check_accounts_data_total_size<'a>(
    bank: &Bank,
    execution_results: impl IntoIterator<Item = &'a TransactionExecutionResult>,
) -> transaction::Result<()> {
    if !bank
        .feature_set
        .is_active(&feature_set::cap_accounts_data_len::id())
    {
        return Ok(());
    }

    if let Some(result) = execution_results
        .into_iter()
        .map(|execution_result| execution_result.flattened_result())
        .find(|result| {
            matches!(
                result,
                Err(TransactionError::InstructionError(
                    _,
                    InstructionError::MaxAccountsDataSizeExceeded
                )),
            )
        })
    {
        return result;
    }

    Ok(())
}
