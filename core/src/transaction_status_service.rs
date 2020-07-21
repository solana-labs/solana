use crossbeam_channel::{Receiver, RecvTimeoutError};
use solana_ledger::{blockstore::Blockstore, blockstore_processor::TransactionStatusBatch};
use solana_runtime::{
    bank::{Bank, HashAgeKind},
    nonce_utils,
    transaction_utils::OrderedIterator,
};
use solana_transaction_status::TransactionStatusMeta;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub struct TransactionStatusService {
    thread_hdl: JoinHandle<()>,
}

impl TransactionStatusService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        write_transaction_status_receiver: Receiver<TransactionStatusBatch>,
        blockstore: Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-transaction-status-writer".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(RecvTimeoutError::Disconnected) = Self::write_transaction_status_batch(
                    &write_transaction_status_receiver,
                    &blockstore,
                ) {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn write_transaction_status_batch(
        write_transaction_status_receiver: &Receiver<TransactionStatusBatch>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<(), RecvTimeoutError> {
        let TransactionStatusBatch {
            bank,
            transactions,
            iteration_order,
            statuses,
            balances,
        } = write_transaction_status_receiver.recv_timeout(Duration::from_secs(1))?;

        let slot = bank.slot();
        for (((transaction, (status, hash_age_kind)), pre_balances), post_balances) in
            OrderedIterator::new(&transactions, iteration_order.as_deref())
                .zip(statuses)
                .zip(balances.pre_balances)
                .zip(balances.post_balances)
        {
            if Bank::can_commit(&status) && !transaction.signatures.is_empty() {
                let fee_calculator = match hash_age_kind {
                    Some(HashAgeKind::DurableNonce(_, account)) => {
                        nonce_utils::fee_calculator_of(&account)
                    }
                    _ => bank.get_fee_calculator(&transaction.message().recent_blockhash),
                }
                .expect("FeeCalculator must exist");
                let fee = fee_calculator.calculate_fee(transaction.message());
                let (writable_keys, readonly_keys) =
                    transaction.message.get_account_keys_by_lock_type();
                blockstore
                    .write_transaction_status(
                        slot,
                        transaction.signatures[0],
                        writable_keys,
                        readonly_keys,
                        &TransactionStatusMeta {
                            status,
                            fee,
                            pre_balances,
                            post_balances,
                        },
                    )
                    .expect("Expect database write to succeed");
            }
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
