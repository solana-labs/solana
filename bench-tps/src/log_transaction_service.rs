//! `LogTransactionService` requests confirmed blocks, analyses transactions submitted by bench-tps,
//! and saves log files in csv format.

use {
    crate::rpc_with_retry_utils::{get_blocks_with_retry, get_slot_with_retry},
    chrono::{DateTime, TimeZone, Utc},
    crossbeam_channel::{select, tick, unbounded, Receiver, Sender},
    log::*,
    serde::Serialize,
    solana_measure::measure::Measure,
    solana_rpc_client_api::config::RpcBlockConfig,
    solana_sdk::{
        clock::{DEFAULT_MS_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::{CommitmentConfig, CommitmentLevel},
        signature::Signature,
        slot_history::Slot,
    },
    solana_tps_client::TpsClient,
    solana_transaction_status::{
        option_serializer::OptionSerializer, EncodedTransactionWithStatusMeta, RewardType,
        TransactionDetails, UiConfirmedBlock, UiTransactionEncoding, UiTransactionStatusMeta,
    },
    std::{
        collections::HashMap,
        fs::File,
        sync::Arc,
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

// Data to establish communication between sender thread and
// LogTransactionService.
#[derive(Clone)]
pub(crate) struct TransactionInfoBatch {
    pub signatures: Vec<Signature>,
    pub sent_at: DateTime<Utc>,
    pub compute_unit_prices: Vec<Option<u64>>,
}

pub(crate) type SignatureBatchSender = Sender<TransactionInfoBatch>;

pub(crate) struct LogTransactionService {
    thread_handler: JoinHandle<()>,
}

pub(crate) fn create_log_transactions_service_and_sender<Client>(
    client: &Arc<Client>,
    block_data_file: Option<&str>,
    transaction_data_file: Option<&str>,
) -> (Option<LogTransactionService>, Option<SignatureBatchSender>)
where
    Client: 'static + TpsClient + Send + Sync + ?Sized,
{
    if data_file_provided(block_data_file, transaction_data_file) {
        let (sender, receiver) = unbounded();
        let log_tx_service =
            LogTransactionService::new(client, receiver, block_data_file, transaction_data_file);
        (Some(log_tx_service), Some(sender))
    } else {
        (None, None)
    }
}

// How many blocks to process during one iteration.
// The time to process blocks is dominated by get_block calls.
// Each call takes slightly less time than slot.
const NUM_SLOTS_PER_ITERATION: u64 = 16;
// How often process blocks.
const PROCESS_BLOCKS_EVERY_MS: u64 = NUM_SLOTS_PER_ITERATION * DEFAULT_MS_PER_SLOT;
// Max age for transaction in the transaction map, older transactions are cleaned up and marked as timeout.
const REMOVE_TIMEOUT_TX_EVERY_MS: i64 = MAX_PROCESSING_AGE as i64 * DEFAULT_MS_PER_SLOT as i64;

// Map used to filter submitted transactions.
#[derive(Clone)]
struct TransactionSendInfo {
    pub sent_at: DateTime<Utc>,
    pub compute_unit_price: Option<u64>,
}
type MapSignatureToTxInfo = HashMap<Signature, TransactionSendInfo>;

type SignatureBatchReceiver = Receiver<TransactionInfoBatch>;

impl LogTransactionService {
    fn new<Client>(
        client: &Arc<Client>,
        signature_receiver: SignatureBatchReceiver,
        block_data_file: Option<&str>,
        transaction_data_file: Option<&str>,
    ) -> Self
    where
        Client: 'static + TpsClient + Send + Sync + ?Sized,
    {
        if !data_file_provided(block_data_file, transaction_data_file) {
            panic!("Expect block-data-file or transaction-data-file is specified, must have been verified by callee.");
        }

        let client = client.clone();
        let tx_log_writer = TransactionLogWriter::new(transaction_data_file);
        let block_log_writer = BlockLogWriter::new(block_data_file);

        let thread_handler = Builder::new()
            .name("LogTransactionService".to_string())
            .spawn(move || {
                Self::run(client, signature_receiver, tx_log_writer, block_log_writer);
            })
            .expect("LogTransactionService should have started successfully.");
        Self { thread_handler }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_handler.join()
    }

    fn run<Client>(
        client: Arc<Client>,
        signature_receiver: SignatureBatchReceiver,
        mut tx_log_writer: TransactionLogWriter,
        mut block_log_writer: BlockLogWriter,
    ) where
        Client: 'static + TpsClient + Send + Sync + ?Sized,
    {
        // used to request blocks data and only confirmed makes sense in this context.
        let commitment: CommitmentConfig = CommitmentConfig {
            commitment: CommitmentLevel::Confirmed,
        };
        let block_processing_timer_receiver = tick(Duration::from_millis(PROCESS_BLOCKS_EVERY_MS));

        let mut start_slot = get_slot_with_retry(&client, commitment)
            .expect("get_slot_with_retry should have succeed, cannot proceed without having slot. Must be a problem with RPC.");

        let mut sender_stopped = false;
        let mut signature_to_tx_info = MapSignatureToTxInfo::new();
        loop {
            select! {
                recv(signature_receiver) -> msg => {
                    match msg {
                        Ok(TransactionInfoBatch {
                            signatures,
                            sent_at,
                            compute_unit_prices
                        }) => {
                            signatures.iter().zip(compute_unit_prices).for_each( |(sign, compute_unit_price)| {signature_to_tx_info.insert(*sign, TransactionSendInfo {
                                sent_at,
                                compute_unit_price
                            });});
                        }
                        Err(_) => {
                            sender_stopped = true;
                        }
                    }
                },
                recv(block_processing_timer_receiver) -> _ => {
                    info!("sign_receiver queue len: {}", signature_receiver.len());
                    if !signature_receiver.is_empty() {
                        continue;
                    }
                    let mut measure_get_blocks = Measure::start("measure_get_blocks");
                    let block_slots = get_blocks_with_retry(&client, start_slot, Some(start_slot + NUM_SLOTS_PER_ITERATION - 1), commitment);
                    measure_get_blocks.stop();
                    let time_get_blocks_us = measure_get_blocks.as_us();
                    info!("Time to get_blocks : {time_get_blocks_us}us.");
                    let Ok(block_slots) = block_slots else {
                        error!("Failed to get blocks, stop LogWriterService.");
                        break;
                    };
                    if block_slots.is_empty() {
                        continue;
                    }
                    let last_block_time = Self::process_blocks(
                        &client,
                        block_slots,
                        &mut signature_to_tx_info,
                        &mut tx_log_writer,
                        &mut block_log_writer,
                        commitment,
                    );
                    Self::clean_transaction_map(&mut tx_log_writer, &mut signature_to_tx_info, last_block_time);

                    start_slot = start_slot.saturating_add(NUM_SLOTS_PER_ITERATION);
                    tx_log_writer.flush();
                    block_log_writer.flush();
                    if sender_stopped && signature_to_tx_info.is_empty() {
                        info!("Stop LogTransactionService");
                        break;
                    }
                },
            }
        }
    }

    /// Download and process the blocks.
    /// Returns the time when the last processed block has been confirmed or now().
    fn process_blocks<Client>(
        client: &Arc<Client>,
        block_slots: Vec<Slot>,
        signature_to_tx_info: &mut MapSignatureToTxInfo,
        tx_log_writer: &mut TransactionLogWriter,
        block_log_writer: &mut BlockLogWriter,
        commitment: CommitmentConfig,
    ) -> DateTime<Utc>
    where
        Client: 'static + TpsClient + Send + Sync + ?Sized,
    {
        let rpc_block_config = RpcBlockConfig {
            encoding: Some(UiTransactionEncoding::Base64),
            transaction_details: Some(TransactionDetails::Full),
            rewards: Some(true),
            commitment: Some(commitment),
            max_supported_transaction_version: Some(0),
        };
        let mut measure_process_blocks = Measure::start("measure_process_blocks");
        let blocks = block_slots
            .iter()
            .map(|slot| client.get_block_with_config(*slot, rpc_block_config));
        let num_blocks = blocks.len();
        let mut last_block_time = None;
        for (block, slot) in blocks.zip(&block_slots) {
            let Ok(block) = block else {
                continue;
            };
            let block_time = Self::process_block(
                block,
                signature_to_tx_info,
                *slot,
                tx_log_writer,
                block_log_writer,
            );
            // if last_time is some, it means that there is at least one valid block
            if block_time.is_some() {
                last_block_time = block_time;
            }
        }
        measure_process_blocks.stop();
        let time_process_blocks_us = measure_process_blocks.as_us();
        info!("Time to process {num_blocks} blocks: {time_process_blocks_us}us.");
        last_block_time.unwrap_or_else(Utc::now)
    }

    fn process_block(
        block: UiConfirmedBlock,
        signature_to_tx_info: &mut MapSignatureToTxInfo,
        slot: u64,
        tx_log_writer: &mut TransactionLogWriter,
        block_log_writer: &mut BlockLogWriter,
    ) -> Option<DateTime<Utc>> {
        let rewards = block
            .rewards
            .as_ref()
            .expect("Rewards should be part of the block information.");
        let slot_leader = rewards
            .iter()
            .find(|r| r.reward_type == Some(RewardType::Fee))
            .map_or("".to_string(), |x| x.pubkey.clone());

        let Some(transactions) = &block.transactions else {
            warn!("Empty block: {slot}");
            return None;
        };

        let mut num_bench_tps_transactions: usize = 0;
        let mut total_cu_consumed: u64 = 0;
        let mut bench_tps_cu_consumed: u64 = 0;
        for EncodedTransactionWithStatusMeta {
            transaction, meta, ..
        } in transactions
        {
            let Some(transaction) = transaction.decode() else {
                continue;
            };
            let cu_consumed = meta
                .as_ref()
                .map_or(0, |meta| match meta.compute_units_consumed {
                    OptionSerializer::Some(cu_consumed) => cu_consumed,
                    _ => 0,
                });
            let signature = &transaction.signatures[0];

            total_cu_consumed = total_cu_consumed.saturating_add(cu_consumed);
            if let Some(TransactionSendInfo {
                sent_at,
                compute_unit_price,
            }) = signature_to_tx_info.remove(signature)
            {
                num_bench_tps_transactions = num_bench_tps_transactions.saturating_add(1);
                bench_tps_cu_consumed = bench_tps_cu_consumed.saturating_add(cu_consumed);

                tx_log_writer.write(
                    Some(block.blockhash.clone()),
                    Some(slot_leader.clone()),
                    signature,
                    sent_at,
                    Some(slot),
                    block.block_time,
                    meta.as_ref(),
                    false,
                    compute_unit_price,
                );
            }
        }
        block_log_writer.write(
            block.blockhash.clone(),
            slot_leader,
            slot,
            block.block_time,
            num_bench_tps_transactions,
            transactions.len(),
            bench_tps_cu_consumed,
            total_cu_consumed,
        );

        block.block_time.map(|time| {
            Utc.timestamp_opt(time, 0)
                .latest()
                .expect("valid timestamp")
        })
    }

    /// Remove from map all the signatures which we haven't processed before and they are
    /// older than the timestamp of the last processed block plus max blockhash age.
    fn clean_transaction_map(
        tx_log_writer: &mut TransactionLogWriter,
        signature_to_tx_info: &mut MapSignatureToTxInfo,
        last_block_time: DateTime<Utc>,
    ) {
        signature_to_tx_info.retain(|signature, tx_info| {
            let duration_since_sent = last_block_time.signed_duration_since(tx_info.sent_at);
            let is_timeout_tx = duration_since_sent.num_milliseconds() > REMOVE_TIMEOUT_TX_EVERY_MS;
            if is_timeout_tx {
                tx_log_writer.write(
                    None,
                    None,
                    signature,
                    tx_info.sent_at,
                    None,
                    None,
                    None,
                    true,
                    tx_info.compute_unit_price,
                );
            }
            !is_timeout_tx
        });
    }
}

fn data_file_provided(block_data_file: Option<&str>, transaction_data_file: Option<&str>) -> bool {
    block_data_file.is_some() || transaction_data_file.is_some()
}

type CsvFileWriter = csv::Writer<File>;

#[derive(Clone, Serialize)]
struct BlockData {
    pub blockhash: String,
    pub block_slot: Slot,
    pub slot_leader: String,
    pub block_time: Option<DateTime<Utc>>,
    pub total_num_transactions: usize,
    pub num_bench_tps_transactions: usize,
    pub total_cu_consumed: u64,
    pub bench_tps_cu_consumed: u64,
}

struct BlockLogWriter {
    log_writer: Option<CsvFileWriter>,
}

impl BlockLogWriter {
    fn new(block_data_file: Option<&str>) -> Self {
        let block_log_writer = block_data_file.map(|block_data_file| {
            CsvFileWriter::from_writer(
                File::create(block_data_file)
                    .expect("Application should be able to create a file."),
            )
        });
        Self {
            log_writer: block_log_writer,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        blockhash: String,
        slot_leader: String,
        slot: Slot,
        block_time: Option<i64>,
        num_bench_tps_transactions: usize,
        total_num_transactions: usize,
        bench_tps_cu_consumed: u64,
        total_cu_consumed: u64,
    ) {
        let Some(block_log_writer) = &mut self.log_writer else {
            return;
        };
        let block_data = BlockData {
            blockhash,
            slot_leader,
            block_slot: slot,
            block_time: block_time.map(|time| {
                Utc.timestamp_opt(time, 0)
                    .latest()
                    .expect("timestamp should be valid")
            }),
            num_bench_tps_transactions,
            total_num_transactions,
            bench_tps_cu_consumed,
            total_cu_consumed,
        };
        let _ = block_log_writer.serialize(block_data);
    }

    fn flush(&mut self) {
        if let Some(block_log_writer) = &mut self.log_writer {
            let _ = block_log_writer.flush();
        }
    }
}

#[derive(Clone, Serialize)]
struct TransactionData {
    pub blockhash: Option<String>,
    pub slot_leader: Option<String>,
    pub signature: String,
    pub sent_at: Option<DateTime<Utc>>,
    pub confirmed_slot: Option<Slot>,
    pub block_time: Option<DateTime<Utc>>,
    pub successful: bool,
    pub error: Option<String>,
    pub timed_out: bool,
    pub compute_unit_price: u64,
}

struct TransactionLogWriter {
    log_writer: Option<CsvFileWriter>,
}

impl TransactionLogWriter {
    fn new(transaction_data_file: Option<&str>) -> Self {
        let transaction_log_writer = transaction_data_file.map(|transaction_data_file| {
            CsvFileWriter::from_writer(
                File::create(transaction_data_file)
                    .expect("Application should be able to create a file."),
            )
        });
        Self {
            log_writer: transaction_log_writer,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        blockhash: Option<String>,
        slot_leader: Option<String>,
        signature: &Signature,
        sent_at: DateTime<Utc>,
        confirmed_slot: Option<Slot>,
        block_time: Option<i64>,
        meta: Option<&UiTransactionStatusMeta>,
        timed_out: bool,
        compute_unit_price: Option<u64>,
    ) {
        let Some(transaction_log_writer) = &mut self.log_writer else {
            return;
        };
        let tx_data = TransactionData {
            blockhash,
            slot_leader,
            signature: signature.to_string(),
            sent_at: Some(sent_at),
            confirmed_slot,
            block_time: block_time.map(|time| {
                Utc.timestamp_opt(time, 0)
                    .latest()
                    .expect("valid timestamp")
            }),
            successful: meta.as_ref().map_or(false, |m| m.status.is_ok()),
            error: meta
                .as_ref()
                .and_then(|m| m.err.as_ref().map(|x| x.to_string())),
            timed_out,
            compute_unit_price: compute_unit_price.unwrap_or(0),
        };
        let _ = transaction_log_writer.serialize(tx_data);
    }

    fn flush(&mut self) {
        if let Some(transaction_log_writer) = &mut self.log_writer {
            let _ = transaction_log_writer.flush();
        }
    }
}
