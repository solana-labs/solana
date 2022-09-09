use {
    crate::{
        blockstore_processor::{BlockCostCapacityMeter, TransactionStatusSender},
        token_balances::collect_token_balances,
    },
    crossbeam_channel::{unbounded, Receiver, RecvError, SendError, Sender},
    solana_program_runtime::timings::ExecuteTimings,
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
        transaction::{self, SanitizedTransaction, TransactionError},
    },
    solana_transaction_status::token_balances::TransactionTokenBalancesSet,
    std::{
        collections::HashMap,
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

pub struct ReplayResponse {
    pub result: transaction::Result<()>,
    pub timing: ExecuteTimings,
    pub idx: Option<usize>,
}

/// Request for replay, sends responses back on this channel
pub struct ReplayRequest {
    pub bank: Arc<Bank>,
    pub tx: SanitizedTransaction,
    pub transaction_status_sender: Option<TransactionStatusSender>,
    pub replay_vote_sender: Option<ReplayVoteSender>,
    pub cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    pub entry_callback: Option<ProcessCallback>,
    pub idx: Option<usize>,
}

pub struct Replayer {
    threads: Vec<JoinHandle<()>>,
}

pub struct ReplayerHandle {
    request_sender: Sender<ReplayRequest>,
    response_receiver: Receiver<ReplayResponse>,
}

impl ReplayerHandle {
    pub fn new(
        request_sender: Sender<ReplayRequest>,
        response_receiver: Receiver<ReplayResponse>,
    ) -> ReplayerHandle {
        ReplayerHandle {
            request_sender,
            response_receiver,
        }
    }

    pub fn send(&self, request: ReplayRequest) -> Result<(), SendError<ReplayRequest>> {
        self.request_sender.send(request)
    }

    pub fn recv_and_drain(&self) -> Result<Vec<ReplayResponse>, RecvError> {
        let mut results = vec![self.response_receiver.recv()?];
        results.extend(self.response_receiver.try_iter());
        Ok(results)
    }
}

impl Replayer {
    pub fn new(num_threads: usize) -> (Replayer, ReplayerHandle) {
        let (request_sender, request_receiver) = unbounded();
        let (response_sender, response_receiver) = unbounded();
        let threads = Self::start_replay_threads(num_threads, request_receiver, response_sender);
        (
            Replayer { threads },
            ReplayerHandle {
                request_sender,
                response_receiver,
            },
        )
    }

    pub fn start_replay_threads(
        num_threads: usize,
        request_receiver: Receiver<ReplayRequest>,
        response_sender: Sender<ReplayResponse>,
    ) -> Vec<JoinHandle<()>> {
        (0..num_threads)
            .map(|i| {
                let request_receiver = request_receiver.clone();
                let response_sender = response_sender.clone();
                Builder::new()
                    .name(format!("solReplayer-{}", i))
                    .spawn(move || loop {
                        match request_receiver.recv() {
                            Ok(ReplayRequest {
                                bank,
                                tx,
                                transaction_status_sender,
                                replay_vote_sender,
                                cost_capacity_meter,
                                entry_callback,
                                idx,
                            }) => {
                                // let mut timing = ExecuteTimings::default();
                                //
                                // let txs = vec![tx];
                                // let batch =
                                //     TransactionBatch::new(vec![Ok(())], &bank, Cow::Borrowed(&txs));
                                // let result = execute_batch(
                                //     &batch,
                                //     &bank,
                                //     transaction_status_sender.as_ref(),
                                //     replay_vote_sender.as_ref(),
                                //     &mut timing,
                                //     cost_capacity_meter.clone(),
                                // );
                                //
                                // if let Some(entry_callback) = entry_callback {
                                //     entry_callback(&bank);
                                // }
                                //
                                // if response_sender
                                //     .send(ReplayResponse {
                                //         result,
                                //         timing,
                                //         idx,
                                //     })
                                //     .is_err()
                                // {
                                //     break;
                                // }
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

fn execute_batch(
    batch: &TransactionBatchWithIndexes,
    bank: &Arc<Bank>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timings: &mut ExecuteTimings,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    tx_cost: u64,
    log_messages_bytes_limit: Option<usize>,
) -> transaction::Result<()> {
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

    // let first_err = get_first_error(batch, fee_collection_results);
    // first_err.map(|(result, _)| result).unwrap_or(Ok(()))
    Ok(())
}
