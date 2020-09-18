use crate::send_transaction_service::{SendTransactionService, TransactionInfo};
use bincode::{deserialize, serialize};
use futures::{
    future,
    prelude::stream::{self, StreamExt},
};
use solana_banks_interface::{Banks, BanksRequest, BanksResponse, TransactionStatus};
use solana_runtime::{
    bank::Bank,
    bank_forks::BankForks,
    commitment::{BlockCommitmentCache, CommitmentSlots},
};
use solana_sdk::{
    account::Account,
    clock::Slot,
    commitment_config::CommitmentLevel,
    fee_calculator::FeeCalculator,
    hash::Hash,
    pubkey::Pubkey,
    signature::Signature,
    transaction::{self, Transaction},
};
use std::{
    collections::HashMap,
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, RwLock,
    },
    thread::Builder,
    time::Duration,
};
use tarpc::{
    context::Context,
    rpc::{transport::channel::UnboundedChannel, ClientMessage, Response},
    serde_transport::tcp,
    server::{self, Channel, Handler},
    transport,
};
use tokio::time::delay_for;
use tokio_serde::formats::Bincode;

#[derive(Clone)]
struct BanksServer {
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    transaction_sender: Sender<TransactionInfo>,
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
    ) -> Self {
        Self {
            bank_forks,
            block_commitment_cache,
            transaction_sender,
        }
    }

    fn run(bank: &Bank, transaction_receiver: Receiver<TransactionInfo>) {
        while let Ok(info) = transaction_receiver.recv() {
            let mut transaction_infos = vec![info];
            while let Ok(info) = transaction_receiver.try_recv() {
                transaction_infos.push(info);
            }
            let transactions: Vec<_> = transaction_infos
                .into_iter()
                .map(|info| deserialize(&info.wire_transaction).unwrap())
                .collect();
            let _ = bank.process_transactions(&transactions);
        }
    }

    /// Useful for unit-testing
    fn new_loopback(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let (transaction_sender, transaction_receiver) = channel();
        let bank = bank_forks.read().unwrap().working_bank();
        let slot = bank.slot();
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new(
            HashMap::default(),
            0,
            CommitmentSlots::new_from_slot(slot),
        )));
        Builder::new()
            .name("solana-bank-forks-client".to_string())
            .spawn(move || Self::run(&bank, transaction_receiver))
            .unwrap();
        Self::new(bank_forks, block_commitment_cache, transaction_sender)
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
        signature: Signature,
        last_valid_slot: Slot,
        commitment: CommitmentLevel,
    ) -> Option<transaction::Result<()>> {
        let mut status = self.bank(commitment).get_signature_status(&signature);
        while status.is_none() {
            delay_for(Duration::from_millis(200)).await;
            let bank = self.bank(commitment);
            if bank.slot() > last_valid_slot {
                break;
            }
            status = bank.get_signature_status(&signature);
        }
        status
    }
}

#[tarpc::server]
impl Banks for BanksServer {
    async fn send_transaction_with_context(self, _: Context, transaction: Transaction) {
        let blockhash = &transaction.message.recent_blockhash;
        let last_valid_slot = self
            .bank_forks
            .read()
            .unwrap()
            .root_bank()
            .get_blockhash_last_valid_slot(&blockhash)
            .unwrap();
        let signature = transaction.signatures.get(0).cloned().unwrap_or_default();
        let info =
            TransactionInfo::new(signature, serialize(&transaction).unwrap(), last_valid_slot);
        self.transaction_sender.send(info).unwrap();
    }

    async fn get_fees_with_commitment_and_context(
        self,
        _: Context,
        commitment: CommitmentLevel,
    ) -> (FeeCalculator, Hash, Slot) {
        let bank = self.bank(commitment);
        let (blockhash, fee_calculator) = bank.last_blockhash_with_fee_calculator();
        let last_valid_slot = bank.get_blockhash_last_valid_slot(&blockhash).unwrap();
        (fee_calculator, blockhash, last_valid_slot)
    }

    async fn get_transaction_status_with_context(
        self,
        _: Context,
        signature: Signature,
    ) -> Option<TransactionStatus> {
        let bank = self.bank(CommitmentLevel::Recent);
        let (slot, status) = bank.get_signature_status_slot(&signature)?;
        let r_block_commitment_cache = self.block_commitment_cache.read().unwrap();

        let confirmations = if r_block_commitment_cache.root() >= slot {
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
        })
    }

    async fn get_slot_with_context(self, _: Context, commitment: CommitmentLevel) -> Slot {
        self.slot(commitment)
    }

    async fn process_transaction_with_commitment_and_context(
        self,
        _: Context,
        transaction: Transaction,
        commitment: CommitmentLevel,
    ) -> Option<transaction::Result<()>> {
        let blockhash = &transaction.message.recent_blockhash;
        let last_valid_slot = self
            .bank_forks
            .read()
            .unwrap()
            .root_bank()
            .get_blockhash_last_valid_slot(&blockhash)
            .unwrap();
        let signature = transaction.signatures.get(0).cloned().unwrap_or_default();
        let info =
            TransactionInfo::new(signature, serialize(&transaction).unwrap(), last_valid_slot);
        self.transaction_sender.send(info).unwrap();
        self.poll_signature_status(signature, last_valid_slot, commitment)
            .await
    }

    async fn get_account_with_commitment_and_context(
        self,
        _: Context,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> Option<Account> {
        let bank = self.bank(commitment);
        bank.get_account(&address)
    }
}

pub async fn start_local_server(
    bank_forks: &Arc<RwLock<BankForks>>,
) -> UnboundedChannel<Response<BanksResponse>, ClientMessage<BanksRequest>> {
    let banks_server = BanksServer::new_loopback(bank_forks.clone());
    let (client_transport, server_transport) = transport::channel::unbounded();
    let server = server::new(server::Config::default())
        .incoming(stream::once(future::ready(server_transport)))
        .respond_with(banks_server.serve());
    tokio::spawn(server);
    client_transport
}

pub async fn start_tcp_server(
    listen_addr: SocketAddr,
    tpu_addr: SocketAddr,
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
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
            let (sender, receiver) = channel();

            SendTransactionService::new(tpu_addr, &bank_forks, receiver);

            let server =
                BanksServer::new(bank_forks.clone(), block_commitment_cache.clone(), sender);
            chan.respond_with(server.serve()).execute()
        })
        // Max 10 channels.
        .buffer_unordered(10)
        .for_each(|_| async {});

    server.await;
    Ok(())
}
