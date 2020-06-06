use crate::cluster_info::ClusterInfo;
use solana_ledger::bank_forks::BankForks;
use solana_metrics::{datapoint_warn, inc_new_counter_info};
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, signature::Signature};
use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, RwLock,
    },
    thread::{self, Builder, JoinHandle},
    time::{Duration, Instant},
};

/// Maximum size of the transaction queue
const MAX_TRANSACTION_QUEUE_SIZE: usize = 10_000; // This seems like a lot but maybe it needs to be bigger one day

pub struct SendTransactionService {
    thread: JoinHandle<()>,
    sender: Mutex<Sender<TransactionInfo>>,
    send_socket: UdpSocket,
    tpu_address: SocketAddr,
}

struct TransactionInfo {
    signature: Signature,
    wire_transaction: Vec<u8>,
    last_valid_slot: Slot,
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
        cluster_info: &Arc<ClusterInfo>,
        bank_forks: &Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let (sender, receiver) = channel::<TransactionInfo>();
        let tpu_address = cluster_info.my_contact_info().tpu;

        let thread = Self::retry_thread(receiver, bank_forks.clone(), tpu_address, exit.clone());
        Self {
            thread,
            sender: Mutex::new(sender),
            send_socket: UdpSocket::bind("0.0.0.0:0").unwrap(),
            tpu_address,
        }
    }

    fn retry_thread(
        receiver: Receiver<TransactionInfo>,
        bank_forks: Arc<RwLock<BankForks>>,
        tpu_address: SocketAddr,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let mut last_status_check = Instant::now();
        let mut transactions = HashMap::new();
        let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        Builder::new()
            .name("send-tx-svc".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }

                if let Ok(transaction_info) = receiver.recv_timeout(Duration::from_secs(1)) {
                    if transactions.len() < MAX_TRANSACTION_QUEUE_SIZE {
                        transactions.insert(transaction_info.signature, transaction_info);
                    } else {
                        datapoint_warn!("send_transaction_service-queue-overflow");
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
                            &send_socket,
                            &tpu_address,
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

    pub fn send(&self, signature: Signature, wire_transaction: Vec<u8>, last_valid_slot: Slot) {
        inc_new_counter_info!("send_transaction_service-enqueue", 1, 1);
        Self::send_transaction(&self.send_socket, &self.tpu_address, &wire_transaction);

        self.sender
            .lock()
            .unwrap()
            .send(TransactionInfo {
                signature,
                wire_transaction,
                last_valid_slot,
            })
            .unwrap_or_else(|err| warn!("Failed to enqueue transaction: {}", err));
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread.join()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rpc::tests::new_bank_forks;
    use solana_sdk::{pubkey::Pubkey, signature::Signer};

    #[test]
    fn service_exit() {
        let cluster_info = Arc::new(ClusterInfo::default());
        let bank_forks = new_bank_forks().0;
        let exit = Arc::new(AtomicBool::new(false));

        let send_tranaction_service =
            SendTransactionService::new(&cluster_info, &bank_forks, &exit);

        exit.store(true, Ordering::Relaxed);
        send_tranaction_service.join().unwrap();
    }

    #[test]
    fn process_transactions() {
        solana_logger::setup();

        let (bank_forks, mint_keypair, _voting_keypair) = new_bank_forks();
        let cluster_info = ClusterInfo::default();
        let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let tpu_address = cluster_info.my_contact_info().tpu;

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
            let transaction = solana_sdk::system_transaction::transfer(
                &mint_keypair,
                &Pubkey::default(),
                1,
                blockhash,
            );
            let signature = transaction.signatures[0];
            working_bank.process_transaction(&transaction).unwrap_err();
            signature
        };

        let mut transactions = HashMap::new();

        info!("Expired transactions are dropped..");
        transactions.insert(
            Signature::default(),
            TransactionInfo {
                signature: Signature::default(),
                wire_transaction: vec![],
                last_valid_slot: root_bank.slot() - 1,
            },
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
            TransactionInfo {
                signature: rooted_signature,
                wire_transaction: vec![],
                last_valid_slot: working_bank.slot(),
            },
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
            TransactionInfo {
                signature: failed_signature,
                wire_transaction: vec![],
                last_valid_slot: working_bank.slot(),
            },
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
            TransactionInfo {
                signature: non_rooted_signature,
                wire_transaction: vec![],
                last_valid_slot: working_bank.slot(),
            },
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
            TransactionInfo {
                signature: Signature::default(),
                wire_transaction: vec![],
                last_valid_slot: working_bank.slot(),
            },
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
