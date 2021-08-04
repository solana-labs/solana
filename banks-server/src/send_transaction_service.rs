// TODO: Merge this implementation with the one at `core/src/send_transaction_service.rs`
use log::*;
use solana_metrics::{datapoint_warn, inc_new_counter_info};
use solana_runtime::{bank::Bank, bank_forks::BankForks};
use solana_sdk::{clock::Slot, signature::Signature};
use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Arc, RwLock,
    },
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

/// Maximum size of the transaction queue
const MAX_TRANSACTION_QUEUE_SIZE: usize = 10_000; // This seems like a lot but maybe it needs to be bigger one day

pub struct SendTransactionService {
    thread: JoinHandle<()>,
}

pub struct TransactionInfo {
    pub signature: Signature,
    pub wire_transaction: Vec<u8>,
    pub last_valid_slot: Slot,
}

impl TransactionInfo {
    pub fn new(signature: Signature, wire_transaction: Vec<u8>, last_valid_slot: Slot) -> Self {
        Self {
            signature,
            wire_transaction,
            last_valid_slot,
        }
    }
}

#[derive(Default, Debug, PartialEq)]
struct ProcessTransactionsResult {
    rooted: u64,
    expired: u64,
    retried: u64,
    failed: u64,
    retained: u64,
}

impl SendTransactionService {
    pub fn new(
        tpu_address: SocketAddr,
        bank_forks: &Arc<RwLock<BankForks>>,
        receiver: Receiver<TransactionInfo>,
    ) -> Self {
        let thread = Self::retry_thread(receiver, bank_forks.clone(), tpu_address);
        Self { thread }
    }

    fn retry_thread(
        receiver: Receiver<TransactionInfo>,
        bank_forks: Arc<RwLock<BankForks>>,
        tpu_address: SocketAddr,
    ) -> JoinHandle<()> {
        let mut last_status_check = Instant::now();
        let mut transactions = HashMap::new();
        let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        Builder::new()
            .name("send-tx-svc".to_string())
            .spawn(move || loop {
                match receiver.recv_timeout(Duration::from_secs(1)) {
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(transaction_info) => {
                        Self::send_transaction(
                            &send_socket,
                            &tpu_address,
                            &transaction_info.wire_transaction,
                        );
                        if transactions.len() < MAX_TRANSACTION_QUEUE_SIZE {
                            transactions.insert(transaction_info.signature, transaction_info);
                        } else {
                            datapoint_warn!("send_transaction_service-queue-overflow");
                        }
                    }
                }

                if Instant::now().duration_since(last_status_check).as_secs() >= 5 {
                    if !transactions.is_empty() {
                        datapoint_info!(
                            "send_transaction_service-queue-size",
                            ("len", transactions.len(), i64)
                        );
                        let bank_forks = bank_forks.read().unwrap();
                        let root_bank = bank_forks.root_bank();
                        let working_bank = bank_forks.working_bank();

                        let _result = Self::process_transactions(
                            &working_bank,
                            &root_bank,
                            &send_socket,
                            &tpu_address,
                            &mut transactions,
                        );
                    }
                    last_status_check = Instant::now();
                }
            })
            .unwrap()
    }

    fn process_transactions(
        working_bank: &Arc<Bank>,
        root_bank: &Arc<Bank>,
        send_socket: &UdpSocket,
        tpu_address: &SocketAddr,
        transactions: &mut HashMap<Signature, TransactionInfo>,
    ) -> ProcessTransactionsResult {
        let mut result = ProcessTransactionsResult::default();

        transactions.retain(|signature, transaction_info| {
            if root_bank.has_signature(signature) {
                info!("Transaction is rooted: {}", signature);
                result.rooted += 1;
                inc_new_counter_info!("send_transaction_service-rooted", 1);
                false
            } else if transaction_info.last_valid_slot < root_bank.slot() {
                info!("Dropping expired transaction: {}", signature);
                result.expired += 1;
                inc_new_counter_info!("send_transaction_service-expired", 1);
                false
            } else {
                match working_bank.get_signature_status_slot(signature) {
                    None => {
                        // Transaction is unknown to the working bank, it might have been
                        // dropped or landed in another fork.  Re-send it
                        info!("Retrying transaction: {}", signature);
                        result.retried += 1;
                        inc_new_counter_info!("send_transaction_service-retry", 1);
                        Self::send_transaction(
                            send_socket,
                            tpu_address,
                            &transaction_info.wire_transaction,
                        );
                        true
                    }
                    Some((_slot, status)) => {
                        if status.is_err() {
                            info!("Dropping failed transaction: {}", signature);
                            result.failed += 1;
                            inc_new_counter_info!("send_transaction_service-failed", 1);
                            false
                        } else {
                            result.retained += 1;
                            true
                        }
                    }
                }
            }
        });

        result
    }

    fn send_transaction(
        send_socket: &UdpSocket,
        tpu_address: &SocketAddr,
        wire_transaction: &[u8],
    ) {
        if let Err(err) = send_socket.send_to(wire_transaction, tpu_address) {
            warn!("Failed to send transaction to {}: {:?}", tpu_address, err);
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{
        genesis_config::create_genesis_config, pubkey::Pubkey, signature::Signer,
        system_transaction,
    };
    use std::sync::mpsc::channel;

    #[test]
    fn service_exit() {
        let tpu_address = "127.0.0.1:0".parse().unwrap();
        let bank = Bank::default();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let (sender, receiver) = channel();

        let send_tranaction_service =
            SendTransactionService::new(tpu_address, &bank_forks, receiver);

        drop(sender);
        send_tranaction_service.join().unwrap();
    }

    #[test]
    fn process_transactions() {
        let (genesis_config, mint_keypair) = create_genesis_config(4);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let tpu_address = "127.0.0.1:0".parse().unwrap();

        let root_bank = Arc::new(Bank::new_from_parent(
            &bank_forks.read().unwrap().working_bank(),
            &Pubkey::default(),
            1,
        ));
        let rooted_signature = root_bank
            .transfer(1, &mint_keypair, &mint_keypair.pubkey())
            .unwrap();

        let working_bank = Arc::new(Bank::new_from_parent(&root_bank, &Pubkey::default(), 2));

        let non_rooted_signature = working_bank
            .transfer(2, &mint_keypair, &mint_keypair.pubkey())
            .unwrap();

        let failed_signature = {
            let blockhash = working_bank.last_blockhash();
            let transaction =
                system_transaction::transfer(&mint_keypair, &Pubkey::default(), 1, blockhash);
            let signature = transaction.signatures[0];
            working_bank.process_transaction(&transaction).unwrap_err();
            signature
        };

        let mut transactions = HashMap::new();

        info!("Expired transactions are dropped..");
        transactions.insert(
            Signature::default(),
            TransactionInfo::new(Signature::default(), vec![], root_bank.slot() - 1),
        );
        let result = SendTransactionService::process_transactions(
            &working_bank,
            &root_bank,
            &send_socket,
            &tpu_address,
            &mut transactions,
        );
        assert!(transactions.is_empty());
        assert_eq!(
            result,
            ProcessTransactionsResult {
                expired: 1,
                ..ProcessTransactionsResult::default()
            }
        );

        info!("Rooted transactions are dropped...");
        transactions.insert(
            rooted_signature,
            TransactionInfo::new(rooted_signature, vec![], working_bank.slot()),
        );
        let result = SendTransactionService::process_transactions(
            &working_bank,
            &root_bank,
            &send_socket,
            &tpu_address,
            &mut transactions,
        );
        assert!(transactions.is_empty());
        assert_eq!(
            result,
            ProcessTransactionsResult {
                rooted: 1,
                ..ProcessTransactionsResult::default()
            }
        );

        info!("Failed transactions are dropped...");
        transactions.insert(
            failed_signature,
            TransactionInfo::new(failed_signature, vec![], working_bank.slot()),
        );
        let result = SendTransactionService::process_transactions(
            &working_bank,
            &root_bank,
            &send_socket,
            &tpu_address,
            &mut transactions,
        );
        assert!(transactions.is_empty());
        assert_eq!(
            result,
            ProcessTransactionsResult {
                failed: 1,
                ..ProcessTransactionsResult::default()
            }
        );

        info!("Non-rooted transactions are kept...");
        transactions.insert(
            non_rooted_signature,
            TransactionInfo::new(non_rooted_signature, vec![], working_bank.slot()),
        );
        let result = SendTransactionService::process_transactions(
            &working_bank,
            &root_bank,
            &send_socket,
            &tpu_address,
            &mut transactions,
        );
        assert_eq!(transactions.len(), 1);
        assert_eq!(
            result,
            ProcessTransactionsResult {
                retained: 1,
                ..ProcessTransactionsResult::default()
            }
        );
        transactions.clear();

        info!("Unknown transactions are retried...");
        transactions.insert(
            Signature::default(),
            TransactionInfo::new(Signature::default(), vec![], working_bank.slot()),
        );
        let result = SendTransactionService::process_transactions(
            &working_bank,
            &root_bank,
            &send_socket,
            &tpu_address,
            &mut transactions,
        );
        assert_eq!(transactions.len(), 1);
        assert_eq!(
            result,
            ProcessTransactionsResult {
                retried: 1,
                ..ProcessTransactionsResult::default()
            }
        );
    }
}
