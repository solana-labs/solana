use {
    bincode::{deserialize, serialize},
    crossbeam_channel::{unbounded, Receiver, Sender},
    futures::{future, prelude::stream::StreamExt},
    solana_banks_interface::{
        Banks, BanksRequest, BanksResponse, BanksTransactionResultWithMetadata,
        BanksTransactionResultWithSimulation, TransactionConfirmationStatus, TransactionMetadata,
        TransactionSimulationDetails, TransactionStatus,
    },
    solana_client::connection_cache::ConnectionCache,
    solana_runtime::{
        bank::{Bank, TransactionExecutionResult, TransactionSimulationResult},
        bank_forks::BankForks,
        commitment::BlockCommitmentCache,
    },
    solana_sdk::{
        account::Account,
        clock::Slot,
        commitment_config::CommitmentLevel,
        feature_set::FeatureSet,
        fee_calculator::FeeCalculator,
        hash::Hash,
        message::{Message, SanitizedMessage},
        pubkey::Pubkey,
        signature::Signature,
        transaction::{self, MessageHash, SanitizedTransaction, VersionedTransaction},
    },
    solana_send_transaction_service::{
        send_transaction_service::{SendTransactionService, TransactionInfo},
        tpu_info::NullTpuInfo,
    },
    std::{
        convert::TryFrom,
        io,
        net::{Ipv4Addr, SocketAddr},
        sync::{Arc, RwLock},
        thread::Builder,
        time::Duration,
    },
    tarpc::{
        context::Context,
        serde_transport::tcp,
        server::{self, incoming::Incoming, Channel},
        transport::{self, channel::UnboundedChannel},
        ClientMessage, Response,
    },
    tokio::time::sleep,
    tokio_serde::formats::Bincode,
};

#[derive(Clone)]
struct BanksServer {
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    transaction_sender: Sender<TransactionInfo>,
    poll_signature_status_sleep_duration: Duration,
}

impl BanksServer {
    /// Return a BanksServer that forwards transactions to the
    /// given sender. If unit-testing, those transactions can go to
    /// a bank in the given BankForks. Otherwise, the receiver should
    /// forward them to a validator in the leader schedule.
    fn new(
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        transaction_sender: Sender<TransactionInfo>,
        poll_signature_status_sleep_duration: Duration,
    ) -> Self {
        Self {
            bank_forks,
            block_commitment_cache,
            transaction_sender,
            poll_signature_status_sleep_duration,
        }
    }

    fn run(bank_forks: Arc<RwLock<BankForks>>, transaction_receiver: Receiver<TransactionInfo>) {
        while let Ok(info) = transaction_receiver.recv() {
            let mut transaction_infos = vec![info];
            while let Ok(info) = transaction_receiver.try_recv() {
                transaction_infos.push(info);
            }
            let transactions: Vec<_> = transaction_infos
                .into_iter()
                .map(|info| deserialize(&info.wire_transaction).unwrap())
                .collect();
            let bank = bank_forks.read().unwrap().working_bank();
            let _ = bank.try_process_transactions(transactions.iter());
        }
    }

    /// Useful for unit-testing
    fn new_loopback(
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        poll_signature_status_sleep_duration: Duration,
    ) -> Self {
        let (transaction_sender, transaction_receiver) = unbounded();
        let bank = bank_forks.read().unwrap().working_bank();
        let slot = bank.slot();
        {
            // ensure that the commitment cache and bank are synced
            let mut w_block_commitment_cache = block_commitment_cache.write().unwrap();
            w_block_commitment_cache.set_all_slots(slot, slot);
        }
        let server_bank_forks = bank_forks.clone();
        Builder::new()
            .name("solBankForksCli".to_string())
            .spawn(move || Self::run(server_bank_forks, transaction_receiver))
            .unwrap();
        Self::new(
            bank_forks,
            block_commitment_cache,
            transaction_sender,
            poll_signature_status_sleep_duration,
        )
    }

    fn slot(&self, commitment: CommitmentLevel) -> Slot {
        self.block_commitment_cache
            .read()
            .unwrap()
            .slot_with_commitment(commitment)
    }

    fn bank(&self, commitment: CommitmentLevel) -> Arc<Bank> {
        self.bank_forks.read().unwrap()[self.slot(commitment)].clone()
    }

    async fn poll_signature_status(
        self,
        signature: &Signature,
        blockhash: &Hash,
        last_valid_block_height: u64,
        commitment: CommitmentLevel,
    ) -> Option<transaction::Result<()>> {
        let mut status = self
            .bank(commitment)
            .get_signature_status_with_blockhash(signature, blockhash);
        while status.is_none() {
            sleep(self.poll_signature_status_sleep_duration).await;
            let bank = self.bank(commitment);
            if bank.block_height() > last_valid_block_height {
                break;
            }
            status = bank.get_signature_status_with_blockhash(signature, blockhash);
        }
        status
    }
}

fn verify_transaction(
    transaction: &SanitizedTransaction,
    feature_set: &Arc<FeatureSet>,
) -> transaction::Result<()> {
    transaction.verify()?;
    transaction.verify_precompiles(feature_set)?;
    Ok(())
}

fn simulate_transaction(
    bank: &Bank,
    transaction: VersionedTransaction,
) -> BanksTransactionResultWithSimulation {
    let sanitized_transaction = match SanitizedTransaction::try_create(
        transaction,
        MessageHash::Compute,
        Some(false), // is_simple_vote_tx
        bank,
        true, // require_static_program_ids
    ) {
        Err(err) => {
            return BanksTransactionResultWithSimulation {
                result: Some(Err(err)),
                simulation_details: None,
            };
        }
        Ok(tx) => tx,
    };
    let TransactionSimulationResult {
        result,
        logs,
        post_simulation_accounts: _,
        units_consumed,
        return_data,
    } = bank.simulate_transaction_unchecked(sanitized_transaction);
    let simulation_details = TransactionSimulationDetails {
        logs,
        units_consumed,
        return_data,
    };
    BanksTransactionResultWithSimulation {
        result: Some(result),
        simulation_details: Some(simulation_details),
    }
}

#[tarpc::server]
impl Banks for BanksServer {
    async fn send_transaction_with_context(self, _: Context, transaction: VersionedTransaction) {
        let blockhash = transaction.message.recent_blockhash();
        let last_valid_block_height = self
            .bank_forks
            .read()
            .unwrap()
            .root_bank()
            .get_blockhash_last_valid_block_height(blockhash)
            .unwrap();
        let signature = transaction.signatures.get(0).cloned().unwrap_or_default();
        let info = TransactionInfo::new(
            signature,
            serialize(&transaction).unwrap(),
            last_valid_block_height,
            None,
            None,
            None,
        );
        self.transaction_sender.send(info).unwrap();
    }

    async fn get_fees_with_commitment_and_context(
        self,
        _: Context,
        commitment: CommitmentLevel,
    ) -> (FeeCalculator, Hash, u64) {
        let bank = self.bank(commitment);
        let blockhash = bank.last_blockhash();
        let lamports_per_signature = bank.get_lamports_per_signature();
        let last_valid_block_height = bank
            .get_blockhash_last_valid_block_height(&blockhash)
            .unwrap();
        (
            FeeCalculator::new(lamports_per_signature),
            blockhash,
            last_valid_block_height,
        )
    }

    async fn get_transaction_status_with_context(
        self,
        _: Context,
        signature: Signature,
    ) -> Option<TransactionStatus> {
        let bank = self.bank(CommitmentLevel::Processed);
        let (slot, status) = bank.get_signature_status_slot(&signature)?;
        let r_block_commitment_cache = self.block_commitment_cache.read().unwrap();

        let optimistically_confirmed_bank = self.bank(CommitmentLevel::Confirmed);
        let optimistically_confirmed =
            optimistically_confirmed_bank.get_signature_status_slot(&signature);

        let confirmations = if r_block_commitment_cache.root() >= slot
            && r_block_commitment_cache.highest_confirmed_root() >= slot
        {
            None
        } else {
            r_block_commitment_cache
                .get_confirmation_count(slot)
                .or(Some(0))
        };
        Some(TransactionStatus {
            slot,
            confirmations,
            err: status.err(),
            confirmation_status: if confirmations.is_none() {
                Some(TransactionConfirmationStatus::Finalized)
            } else if optimistically_confirmed.is_some() {
                Some(TransactionConfirmationStatus::Confirmed)
            } else {
                Some(TransactionConfirmationStatus::Processed)
            },
        })
    }

    async fn get_slot_with_context(self, _: Context, commitment: CommitmentLevel) -> Slot {
        self.slot(commitment)
    }

    async fn get_block_height_with_context(self, _: Context, commitment: CommitmentLevel) -> u64 {
        self.bank(commitment).block_height()
    }

    async fn process_transaction_with_preflight_and_commitment_and_context(
        self,
        ctx: Context,
        transaction: VersionedTransaction,
        commitment: CommitmentLevel,
    ) -> BanksTransactionResultWithSimulation {
        let mut simulation_result =
            simulate_transaction(&self.bank(commitment), transaction.clone());
        // Simulation was ok, so process the real transaction and replace the
        // simulation's result with the real transaction result
        if let Some(Ok(_)) = simulation_result.result {
            simulation_result.result = self
                .process_transaction_with_commitment_and_context(ctx, transaction, commitment)
                .await;
        }
        simulation_result
    }

    async fn simulate_transaction_with_commitment_and_context(
        self,
        _: Context,
        transaction: VersionedTransaction,
        commitment: CommitmentLevel,
    ) -> BanksTransactionResultWithSimulation {
        simulate_transaction(&self.bank(commitment), transaction)
    }

    async fn process_transaction_with_commitment_and_context(
        self,
        _: Context,
        transaction: VersionedTransaction,
        commitment: CommitmentLevel,
    ) -> Option<transaction::Result<()>> {
        let bank = self.bank(commitment);
        let sanitized_transaction = match SanitizedTransaction::try_create(
            transaction.clone(),
            MessageHash::Compute,
            Some(false), // is_simple_vote_tx
            bank.as_ref(),
            true, // require_static_program_ids
        ) {
            Ok(tx) => tx,
            Err(err) => return Some(Err(err)),
        };

        if let Err(err) = verify_transaction(&sanitized_transaction, &bank.feature_set) {
            return Some(Err(err));
        }

        let blockhash = transaction.message.recent_blockhash();
        let last_valid_block_height = self
            .bank(commitment)
            .get_blockhash_last_valid_block_height(blockhash)
            .unwrap();
        let signature = sanitized_transaction.signature();
        let info = TransactionInfo::new(
            *signature,
            serialize(&transaction).unwrap(),
            last_valid_block_height,
            None,
            None,
            None,
        );
        self.transaction_sender.send(info).unwrap();
        self.poll_signature_status(signature, blockhash, last_valid_block_height, commitment)
            .await
    }

    async fn process_transaction_with_metadata_and_context(
        self,
        _: Context,
        transaction: VersionedTransaction,
    ) -> BanksTransactionResultWithMetadata {
        let bank = self.bank_forks.read().unwrap().working_bank();
        match bank.process_transaction_with_metadata(transaction) {
            TransactionExecutionResult::NotExecuted(error) => BanksTransactionResultWithMetadata {
                result: Err(error),
                metadata: None,
            },
            TransactionExecutionResult::Executed { details, .. } => {
                BanksTransactionResultWithMetadata {
                    result: details.status,
                    metadata: Some(TransactionMetadata {
                        compute_units_consumed: details.executed_units,
                        log_messages: details.log_messages.unwrap_or_default(),
                        return_data: details.return_data,
                    }),
                }
            }
        }
    }

    async fn get_account_with_commitment_and_context(
        self,
        _: Context,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> Option<Account> {
        let bank = self.bank(commitment);
        bank.get_account(&address).map(Account::from)
    }

    async fn get_latest_blockhash_with_context(self, _: Context) -> Hash {
        let bank = self.bank(CommitmentLevel::default());
        bank.last_blockhash()
    }

    async fn get_latest_blockhash_with_commitment_and_context(
        self,
        _: Context,
        commitment: CommitmentLevel,
    ) -> Option<(Hash, u64)> {
        let bank = self.bank(commitment);
        let blockhash = bank.last_blockhash();
        let last_valid_block_height = bank.get_blockhash_last_valid_block_height(&blockhash)?;
        Some((blockhash, last_valid_block_height))
    }

    async fn get_fee_for_message_with_commitment_and_context(
        self,
        _: Context,
        commitment: CommitmentLevel,
        message: Message,
    ) -> Option<u64> {
        let bank = self.bank(commitment);
        let sanitized_message = SanitizedMessage::try_from(message).ok()?;
        bank.get_fee_for_message(&sanitized_message)
    }
}

pub async fn start_local_server(
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    poll_signature_status_sleep_duration: Duration,
) -> UnboundedChannel<Response<BanksResponse>, ClientMessage<BanksRequest>> {
    let banks_server = BanksServer::new_loopback(
        bank_forks,
        block_commitment_cache,
        poll_signature_status_sleep_duration,
    );
    let (client_transport, server_transport) = transport::channel::unbounded();
    let server = server::BaseChannel::with_defaults(server_transport).execute(banks_server.serve());
    tokio::spawn(server);
    client_transport
}

pub async fn start_tcp_server(
    listen_addr: SocketAddr,
    tpu_addr: SocketAddr,
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    connection_cache: Arc<ConnectionCache>,
) -> io::Result<()> {
    // Note: These settings are copied straight from the tarpc example.
    let server = tcp::listen(listen_addr, Bincode::default)
        .await?
        // Ignore accept errors.
        .filter_map(|r| future::ready(r.ok()))
        .map(server::BaseChannel::with_defaults)
        // Limit channels to 1 per IP.
        .max_channels_per_key(1, |t| {
            t.as_ref()
                .peer_addr()
                .map(|x| x.ip())
                .unwrap_or_else(|_| Ipv4Addr::new(0, 0, 0, 0).into())
        })
        // serve is generated by the service attribute. It takes as input any type implementing
        // the generated Banks trait.
        .map(move |chan| {
            let (sender, receiver) = unbounded();

            SendTransactionService::new::<NullTpuInfo>(
                tpu_addr,
                &bank_forks,
                None,
                receiver,
                &connection_cache,
                5_000,
                0,
            );

            let server = BanksServer::new(
                bank_forks.clone(),
                block_commitment_cache.clone(),
                sender,
                Duration::from_millis(200),
            );
            chan.execute(server.serve())
        })
        // Max 10 channels.
        .buffer_unordered(10)
        .for_each(|_| async {});

    server.await;
    Ok(())
}
