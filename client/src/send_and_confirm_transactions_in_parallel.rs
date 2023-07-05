use {
    crate::{
        connection_cache::ConnectionCache,
        nonblocking::{rpc_client::RpcClient, tpu_client::TpuClient},
        rpc_client::RpcClient as BlockingRpcClient,
    },
    bincode::serialize,
    dashmap::DashMap,
    solana_rpc_client::spinner,
    solana_rpc_client_api::request::MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS,
    solana_sdk::{
        clock::{MAX_PROCESSING_AGE, MAX_RECENT_BLOCKHASHES},
        message::Message,
        signature::Signature,
        signers::Signers,
        transaction::{Transaction, TransactionError},
    },
    solana_tpu_client::{
        nonblocking::tpu_client::set_message_for_confirmed_transactions,
        tpu_client::{Result, TpuClientConfig},
    },
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
            Arc,
        },
        time::Duration,
    },
    tokio::sync::{Mutex, RwLock},
};

const BLOCKHASH_REFRESH_RATE: Duration = Duration::from_secs(5);
const CONFIRMATION_REFRESH_RATE: Duration = Duration::from_secs(5);
const TPU_RESEND_REFRESH_RATE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
struct TransactionData {
    blockheight: u64,
    last_valid_blockheight: u64,
    transaction: Transaction,
    index: usize,
}

pub fn send_and_confirm_transactions_in_parallel_with_spinner_blocking<T: Signers + ?Sized>(
    rpc_client: Arc<BlockingRpcClient>,
    websocket_url: String,
    messages: &[Message],
    signers: &T,
) -> Result<Vec<Option<TransactionError>>> {
    let fut = send_and_confirm_transactions_in_parallel_with_spinner(
        rpc_client.get_inner_client().clone(),
        websocket_url,
        messages,
        signers,
    );
    tokio::task::block_in_place(|| rpc_client.runtime().block_on(fut))
}

// This is a new method which will be able to send and confirm a large amount of transactions
// usually the send_and_confirm_transactions_with_spinner for QUIC clients are optimized for small number of transactions
// If we use these methods
pub async fn send_and_confirm_transactions_in_parallel_with_spinner<T: Signers + ?Sized>(
    rpc_client: Arc<RpcClient>,
    websocket_url: String,
    messages: &[Message],
    signers: &T,
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
        return Err(solana_tpu_client::tpu_client::TpuSenderError::Custom(
            "Exepeced quic connection cache".to_string(),
        ));
    };
    // get current blockhash
    let original_block_hash = rpc_client.get_latest_blockhash().await?;
    let block_hash_rw = Arc::new(RwLock::new(original_block_hash));
    let current_block_height = Arc::new(AtomicU64::new(0));

    let progress_bar = spinner::new_progress_bar();
    progress_bar.set_message("Setting up...");

    let exit_signal = Arc::new(AtomicBool::new(false));

    // blockhash anc blockheight update task
    {
        let rpc_client = rpc_client.clone();
        let block_hash_rw = block_hash_rw.clone();
        let exit_signal = exit_signal.clone();
        let current_block_height = current_block_height.clone();
        tokio::spawn(async move {
            while !exit_signal.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(blockhash) = rpc_client.get_latest_blockhash().await {
                    *block_hash_rw.write().await = blockhash;
                }

                if let Ok(block_height) = rpc_client.get_block_height().await {
                    current_block_height.store(block_height, std::sync::atomic::Ordering::Relaxed);
                }
                tokio::time::sleep(BLOCKHASH_REFRESH_RATE).await;
            }
        });
    }

    let transaction_map = Arc::new(DashMap::<Signature, TransactionData>::new());
    let transaction_errors = Arc::new(Mutex::new(vec![None; messages.len()]));
    let confirmed_transactions = Arc::new(AtomicU32::new(0));
    // tasks which confirms the transactions that were sent
    {
        let exit_signal = exit_signal.clone();
        let current_block_height = current_block_height.clone();
        let transaction_map = transaction_map.clone();
        let rpc_client = rpc_client.clone();
        let confirmed_transactions = confirmed_transactions.clone();
        let transaction_errors = transaction_errors.clone();
        tokio::spawn(async move {
            while !exit_signal.load(Ordering::Relaxed) {
                if !transaction_map.is_empty() {
                    // transaction is valid for MAX_PROCESSING_AGE
                    // We only verify the transactions till MAX_RECENT_BLOCKHASHES which is 2*MAX_PROCESSING_AGE
                    // If the transaction is not confirmed in MAX_RECENT_BLOCKHASHES we declare that it is expired.
                    // give our CONFIRMATION_REFRESH_RATE of 5 secs we can have maximum 12.5 blocks
                    // So there are 300 slots for the transactions to get confirmed
                    let current_block_height = current_block_height.load(Ordering::Relaxed);
                    let transactions_to_verify: Vec<Signature> = transaction_map
                        .iter()
                        .filter(|x| {
                            current_block_height < x.blockheight + MAX_RECENT_BLOCKHASHES as u64
                        })
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
                                            let mut transaction_errors =
                                                transaction_errors.lock().await;
                                            transaction_errors[data.index] = status.err;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(CONFIRMATION_REFRESH_RATE).await;
            }
        });
    }

    // transaction sender task
    let total_transactions = messages.len();
    let mut initial = true;
    for expired_blockhash_retries in (0..5).rev() {
        // only send messages which have not been confirmed
        let nb_transaction = messages.len();
        let messages: Vec<(usize, Message)> = if initial {
            initial = false;
            messages.iter().cloned().enumerate().collect()
        } else {
            // remove all the confirmed transactions
            transaction_map
                .iter()
                .map(|x| (x.index, x.transaction.message.clone()))
                .collect()
        };

        if messages.is_empty() {
            break;
        }

        // clear the map so that we can start resending
        transaction_map.clear();

        for (index, message) in messages.iter() {
            let mut transaction = Transaction::new_unsigned(message.clone());
            let blockhash = *block_hash_rw.read().await;
            transaction.try_sign(signers, blockhash)?;
            let signautre = transaction.signatures[0];
            if !tpu_client.send_transaction(&transaction).await {
                let _ = tpu_client
                    .rpc_client()
                    .send_transaction(&transaction)
                    .await
                    .is_ok();
            }
            let blockheight = current_block_height.load(std::sync::atomic::Ordering::Relaxed);
            let last_valid_blockheight = blockheight + MAX_PROCESSING_AGE as u64;
            // send to confirm the transaction
            transaction_map.insert(
                signautre,
                TransactionData {
                    index: *index,
                    blockheight,
                    transaction,
                    last_valid_blockheight,
                },
            );

            set_message_for_confirmed_transactions(
                &progress_bar,
                confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
                total_transactions,
                Some(blockheight), //block_height,
                last_valid_blockheight,
                &format!("Sending {}/{} transactions", index + 1, nb_transaction,),
            );
        }
        let blockheight = current_block_height.load(std::sync::atomic::Ordering::Relaxed);
        let wait_till_blockheight = blockheight + MAX_PROCESSING_AGE as u64;

        let transactions_to_confirm = transaction_map.len();
        set_message_for_confirmed_transactions(
            &progress_bar,
            confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
            total_transactions,
            Some(blockheight),
            wait_till_blockheight,
            &format!("Waiting for next block, {transactions_to_confirm} transactions pending..."),
        );

        // wait till all transactions are confirmed or we have surpassed max processing age for the last sent transaction
        while !transaction_map.is_empty()
            && current_block_height.load(Ordering::Relaxed) < wait_till_blockheight
        {
            let blockheight = current_block_height.load(Ordering::Relaxed);
            set_message_for_confirmed_transactions(
                &progress_bar,
                confirmed_transactions.load(std::sync::atomic::Ordering::Relaxed),
                total_transactions,
                Some(blockheight),
                wait_till_blockheight,
                "Checking transaction status...",
            );
            // retry sending transaction only over TPU port / usually we should send transaction once over RPC port but multiple times over tpu port
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
            tokio::time::sleep(TPU_RESEND_REFRESH_RATE).await;
        }

        if transaction_map.is_empty() {
            break;
        }

        progress_bar.println(format!(
            "Blockhash expired. {expired_blockhash_retries} retries remaining"
        ));
    }
    exit_signal.store(true, Ordering::Relaxed);

    let progress_bar = spinner::new_progress_bar();
    progress_bar.set_message("Setting up...");
    let return_data = transaction_errors.lock().await;
    Ok(return_data.clone())
}
