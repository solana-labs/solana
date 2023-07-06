use {
    crate::{
        connection_cache::ConnectionCache,
        nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
        rpc_client::RpcClient as BlockingRpcClient,
    },
    bincode::serialize,
    dashmap::DashMap,
    solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool},
    solana_rpc_client::spinner,
    solana_rpc_client_api::request::MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS,
    solana_sdk::{
        hash::Hash,
        message::Message,
        signature::Signature,
        signers::Signers,
        transaction::{Transaction, TransactionError},
    },
    solana_tpu_client::{
        nonblocking::tpu_client::set_message_for_confirmed_transactions,
        tpu_client::{Result, TpuClientConfig, TpuSenderError},
    },
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
            Arc,
        },
        time::Duration,
    },
    tokio::{sync::RwLock, task::JoinHandle, time::Instant},
};

const BLOCKHASH_REFRESH_RATE: Duration = Duration::from_secs(5);
const CONFIRMATION_REFRESH_RATE: Duration = Duration::from_secs(5);
const TPU_RESEND_REFRESH_RATE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
struct TransactionData {
    last_valid_blockheight: u64,
    transaction: Transaction,
    index: usize,
}

#[derive(Clone, Debug, Copy)]
struct BlockHashData {
    pub blockhash: Hash,
    pub last_valid_blockheight: u64,
}

pub fn send_and_confirm_transactions_in_parallel_blocking<T: Signers + ?Sized>(
    rpc_client: Arc<BlockingRpcClient>,
    websocket_url: String,
    messages: &[Message],
    signers: &T,
    resign_txs_count: Option<usize>,
    with_spinner: bool,
) -> Result<Vec<Option<TransactionError>>> {
    let fut = send_and_confirm_transactions_in_parallel(
        rpc_client.get_inner_client().clone(),
        websocket_url,
        messages,
        signers,
        resign_txs_count,
        with_spinner,
    );
    tokio::task::block_in_place(|| rpc_client.runtime().block_on(fut))
}

fn create_blockhash_data_updating_task(
    rpc_client: Arc<RpcClient>,
    blockhash_data_rw: Arc<RwLock<BlockHashData>>,
    current_blockheight: Arc<AtomicU64>,
    exit_signal: Arc<AtomicBool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while !exit_signal.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(blockhash_and_valid_height) = rpc_client
                .get_latest_blockhash_with_commitment(rpc_client.commitment())
                .await
            {
                *blockhash_data_rw.write().await = BlockHashData {
                    blockhash: blockhash_and_valid_height.0,
                    last_valid_blockheight: blockhash_and_valid_height.1,
                };
            }

            if let Ok(blockheight) = rpc_client.get_block_height().await {
                current_blockheight.store(blockheight, Ordering::Relaxed);
            }
            tokio::time::sleep(BLOCKHASH_REFRESH_RATE).await;
        }
    })
}

fn create_transaction_confirmation_task(
    nb_transasction_count: usize,
    rpc_client: Arc<RpcClient>,
    current_blockheight: Arc<AtomicU64>,
    transaction_map: Arc<DashMap<Signature, TransactionData>>,
    confirmed_transactions: Arc<AtomicU32>,
    exit_signal: Arc<AtomicBool>,
) -> JoinHandle<Vec<Option<TransactionError>>> {
    tokio::spawn(async move {
        let mut transaction_errors = vec![None; nb_transasction_count];
        while !exit_signal.load(Ordering::Relaxed) {
            let instant = Instant::now();
            if !transaction_map.is_empty() {
                let current_blockheight = current_blockheight.load(Ordering::Relaxed);
                let transactions_to_verify: Vec<Signature> = transaction_map
                    .iter()
                    .filter(|x| current_blockheight < x.last_valid_blockheight)
                    .map(|x| *x.key())
                    .collect();
                for signatures in
                    transactions_to_verify.chunks(MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS)
                {
                    if let Ok(result) = rpc_client.get_signature_statuses(signatures).await {
                        let statuses = result.value;
                        for (signature, status) in signatures.iter().zip(statuses.into_iter()) {
                            if let Some(status) = status {
                                if status.satisfies_commitment(rpc_client.commitment()) {
                                    if let Some((_, data)) = transaction_map.remove(signature) {
                                        confirmed_transactions.fetch_add(1, Ordering::Relaxed);
                                        transaction_errors[data.index] = status.err;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let elapsed = instant.elapsed();
            if elapsed < CONFIRMATION_REFRESH_RATE {
                tokio::time::sleep(CONFIRMATION_REFRESH_RATE - elapsed).await;
            }
        }
        transaction_errors
    })
}

async fn sign_all_messages_and_send<T: Signers + ?Sized>(
    progress_bar: &Option<indicatif::ProgressBar>,
    tpu_client: &TpuClient<QuicPool, QuicConnectionManager, QuicConfig>,
    transaction_map: Arc<DashMap<Signature, TransactionData>>,
    messages_with_index: Vec<(usize, Message)>,
    blockhash_data_rw: Arc<RwLock<BlockHashData>>,
    signers: &T,
    confirmed_transactions: Arc<AtomicU32>,
    total_transactions: usize,
) -> Result<()> {
    // send all the transaction meesages
    for (index, message) in messages_with_index.iter() {
        let mut transaction = Transaction::new_unsigned(message.clone());
        let blockhashdata = *blockhash_data_rw.read().await;
        transaction.try_sign(signers, blockhashdata.blockhash)?;
        let signature = transaction.signatures[0];
        if !tpu_client.send_transaction(&transaction).await {
            let _ = tpu_client
                .rpc_client()
                .send_transaction(&transaction)
                .await
                .is_ok();
        }
        // send to confirm the transaction
        transaction_map.insert(
            signature,
            TransactionData {
                index: *index,
                transaction,
                last_valid_blockheight: blockhashdata.last_valid_blockheight,
            },
        );

        if let Some(progress_bar) = progress_bar {
            set_message_for_confirmed_transactions(
                progress_bar,
                confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
                total_transactions,
                None,
                blockhashdata.last_valid_blockheight,
                &format!("Sending {}/{} transactions", index + 1, total_transactions,),
            );
        }
    }
    Ok(())
}

async fn confirm_transactions_till_block_height_and_resend_unexpired_transaction_over_tpu(
    progress_bar: &Option<indicatif::ProgressBar>,
    tpu_client: &TpuClient<QuicPool, QuicConnectionManager, QuicConfig>,
    transaction_map: Arc<DashMap<Signature, TransactionData>>,
    current_blockheight: Arc<AtomicU64>,
    confirmed_transactions: Arc<AtomicU32>,
    wait_till_blockheight: u64,
    total_transactions: usize,
) {
    let transactions_to_confirm = transaction_map.len();

    if let Some(progress_bar) = progress_bar {
        set_message_for_confirmed_transactions(
            progress_bar,
            confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
            total_transactions,
            Some(current_blockheight.load(Ordering::Relaxed)),
            wait_till_blockheight,
            &format!("Waiting for next block, {transactions_to_confirm} transactions pending..."),
        );
    }

    // wait till all transactions are confirmed or we have surpassed max processing age for the last sent transaction
    while !transaction_map.is_empty()
        && current_blockheight.load(Ordering::Relaxed) < wait_till_blockheight
    {
        let blockheight = current_blockheight.load(Ordering::Relaxed);

        if let Some(progress_bar) = progress_bar {
            set_message_for_confirmed_transactions(
                progress_bar,
                confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
                total_transactions,
                Some(blockheight),
                wait_till_blockheight,
                "Checking transaction status...",
            );
        }

        let instant = Instant::now();
        // retry sending transaction only over TPU port / any transactions sent over RPC will be automatically rebroadcast by the RPC server
        let txs_to_resend_over_tpu = transaction_map
            .iter()
            .filter(|x| blockheight < x.last_valid_blockheight)
            .map(|x| {
                serialize(&x.transaction)
                    .expect("Should serialize as it has been serialized before")
            })
            .collect();
        let _ = tpu_client
            .try_send_wire_transaction_batch(txs_to_resend_over_tpu)
            .await;

        let elapsed = instant.elapsed();
        if elapsed < TPU_RESEND_REFRESH_RATE {
            tokio::time::sleep(TPU_RESEND_REFRESH_RATE - elapsed).await;
        }
    }

    if let Some(progress_bar) = progress_bar {
        set_message_for_confirmed_transactions(
            progress_bar,
            confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
            total_transactions,
            Some(current_blockheight.load(Ordering::Relaxed)),
            wait_till_blockheight,
            "Checking transaction status...",
        );
    }
}

// This is a new method which will be able to send and confirm a large amount of transactions
// usually the send_and_confirm_transactions_with_spinner for QUIC clients are optimized for small number of transactions
// If we use these methods
pub async fn send_and_confirm_transactions_in_parallel<T: Signers + ?Sized>(
    rpc_client: Arc<RpcClient>,
    websocket_url: String,
    messages: &[Message],
    signers: &T,
    resign_txs_count: Option<usize>,
    with_spinner: bool,
) -> Result<Vec<Option<TransactionError>>> {
    let connection_cache = ConnectionCache::new_quic("connection_cache_cli_program_quic", 1);
    let tpu_client = if let ConnectionCache::Quic(cache) = connection_cache {
        TpuClient::new_with_connection_cache(
            rpc_client.clone(),
            &websocket_url,
            TpuClientConfig::default(),
            cache,
        )
        .await?
    } else {
        // should not ever happen
        panic!("Expected a quic connection cache");
    };
    // get current blockhash and corresponding last valid block height
    let blockhash_and_valid_height = rpc_client
        .get_latest_blockhash_with_commitment(rpc_client.commitment())
        .await?;
    let blockhash_data_rw = Arc::new(RwLock::new(BlockHashData {
        blockhash: blockhash_and_valid_height.0,
        last_valid_blockheight: blockhash_and_valid_height.1,
    }));

    // get current blockheight
    let block_height = rpc_client.get_block_height().await?;
    let current_blockheight = Arc::new(AtomicU64::new(block_height));

    let progress_bar = if with_spinner {
        let progress_bar = spinner::new_progress_bar();
        progress_bar.set_message("Setting up...");
        Some(progress_bar)
    } else {
        None
    };

    let exit_signal = Arc::new(AtomicBool::new(false));

    // blockhash anc blockheight update task
    let block_data_task = create_blockhash_data_updating_task(
        rpc_client.clone(),
        blockhash_data_rw.clone(),
        current_blockheight.clone(),
        exit_signal.clone(),
    );

    let transaction_map = Arc::new(DashMap::<Signature, TransactionData>::new());
    let confirmed_transactions = Arc::new(AtomicU32::new(0));
    // tasks which confirms the transactions that were sent
    let transaction_confirming_task = create_transaction_confirmation_task(
        messages.len(),
        rpc_client.clone(),
        current_blockheight.clone(),
        transaction_map.clone(),
        confirmed_transactions.clone(),
        exit_signal.clone(),
    );

    // transaction sender task
    let total_transactions = messages.len();
    let mut initial = true;
    let signing_count = resign_txs_count.unwrap_or(1);
    for expired_blockhash_retries in (0..signing_count).rev() {
        // only send messages which have not been confirmed
        let messages_with_index: Vec<(usize, Message)> = if initial {
            initial = false;
            messages.iter().cloned().enumerate().collect()
        } else {
            // remove all the confirmed transactions
            transaction_map
                .iter()
                .map(|x| (x.index, x.transaction.message.clone()))
                .collect()
        };

        if messages_with_index.is_empty() {
            break;
        }

        // clear the map so that we can start resending
        transaction_map.clear();

        sign_all_messages_and_send(
            &progress_bar,
            &tpu_client,
            transaction_map.clone(),
            messages_with_index,
            blockhash_data_rw.clone(),
            signers,
            confirmed_transactions.clone(),
            total_transactions,
        )
        .await?;

        // wait till block height till all the transactions are confirmed
        let block_data = *blockhash_data_rw.read().await;
        let wait_till_blockheight = block_data.last_valid_blockheight;
        confirm_transactions_till_block_height_and_resend_unexpired_transaction_over_tpu(
            &progress_bar,
            &tpu_client,
            transaction_map.clone(),
            current_blockheight.clone(),
            confirmed_transactions.clone(),
            wait_till_blockheight,
            total_transactions,
        )
        .await;

        if transaction_map.is_empty() {
            break;
        }

        if let Some(progress_bar) = &progress_bar {
            progress_bar.println(format!(
                "Blockhash expired. {expired_blockhash_retries} retries remaining"
            ));
        }
    }

    exit_signal.store(true, Ordering::Relaxed);

    block_data_task.abort();
    if transaction_map.is_empty() {
        let transaction_errors = transaction_confirming_task
            .await
            .expect("Confirmation task should join correctly");
        Ok(transaction_errors)
    } else {
        transaction_confirming_task.abort();
        Err(TpuSenderError::Custom("Max retries exceeded".into()))
    }
}
