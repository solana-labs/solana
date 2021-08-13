use {
    crossbeam_channel::{Receiver, RecvTimeoutError},
    itertools::izip,
    solana_ledger::{
        blockstore::Blockstore,
        blockstore_processor::{TransactionStatusBatch, TransactionStatusMessage},
    },
    solana_runtime::bank::{
        Bank, InnerInstructionsList, NonceRollbackInfo, TransactionLogMessages,
    },
    solana_transaction_status::{InnerInstructions, Reward, TransactionStatusMeta},
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct TransactionStatusService {
    thread_hdl: JoinHandle<()>,
}

impl TransactionStatusService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        write_transaction_status_receiver: Receiver<TransactionStatusMessage>,
        max_complete_transaction_status_slot: Arc<AtomicU64>,
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
                    &max_complete_transaction_status_slot,
                    &blockstore,
                ) {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn write_transaction_status_batch(
        write_transaction_status_receiver: &Receiver<TransactionStatusMessage>,
        max_complete_transaction_status_slot: &Arc<AtomicU64>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<(), RecvTimeoutError> {
        match write_transaction_status_receiver.recv_timeout(Duration::from_secs(1))? {
            TransactionStatusMessage::Batch(TransactionStatusBatch {
                bank,
                transactions,
                statuses,
                balances,
                token_balances,
                inner_instructions,
                transaction_logs,
                rent_debits,
            }) => {
                let slot = bank.slot();
                let inner_instructions_iter: Box<
                    dyn Iterator<Item = Option<InnerInstructionsList>>,
                > = if let Some(inner_instructions) = inner_instructions {
                    Box::new(inner_instructions.into_iter())
                } else {
                    Box::new(std::iter::repeat_with(|| None))
                };
                let transaction_logs_iter: Box<
                    dyn Iterator<Item = Option<TransactionLogMessages>>,
                > = if let Some(transaction_logs) = transaction_logs {
                    Box::new(transaction_logs.into_iter())
                } else {
                    Box::new(std::iter::repeat_with(|| None))
                };
                for (
                    transaction,
                    (status, nonce_rollback),
                    pre_balances,
                    post_balances,
                    pre_token_balances,
                    post_token_balances,
                    inner_instructions,
                    log_messages,
                    rent_debits,
                ) in izip!(
                    transactions,
                    statuses,
                    balances.pre_balances,
                    balances.post_balances,
                    token_balances.pre_token_balances,
                    token_balances.post_token_balances,
                    inner_instructions_iter,
                    transaction_logs_iter,
                    rent_debits,
                ) {
                    if Bank::can_commit(&status) {
                        let fee_calculator = nonce_rollback
                            .map(|nonce_rollback| nonce_rollback.fee_calculator())
                            .unwrap_or_else(|| {
                                #[allow(deprecated)]
                                bank.get_fee_calculator(transaction.message().recent_blockhash())
                            })
                            .expect("FeeCalculator must exist");
                        let fee = transaction.message().calculate_fee(&fee_calculator);
                        let tx_account_locks = transaction.get_account_locks();

                        let inner_instructions = inner_instructions.map(|inner_instructions| {
                            inner_instructions
                                .into_iter()
                                .enumerate()
                                .map(|(index, instructions)| InnerInstructions {
                                    index: index as u8,
                                    instructions,
                                })
                                .filter(|i| !i.instructions.is_empty())
                                .collect()
                        });

                        let pre_token_balances = Some(pre_token_balances);
                        let post_token_balances = Some(post_token_balances);
                        let rewards = Some(
                            rent_debits
                                .0
                                .into_iter()
                                .map(|(pubkey, reward_info)| Reward {
                                    pubkey: pubkey.to_string(),
                                    lamports: reward_info.lamports,
                                    post_balance: reward_info.post_balance,
                                    reward_type: Some(reward_info.reward_type),
                                    commission: reward_info.commission,
                                })
                                .collect(),
                        );

                        blockstore
                            .write_transaction_status(
                                slot,
                                *transaction.signature(),
                                tx_account_locks.writable,
                                tx_account_locks.readonly,
                                TransactionStatusMeta {
                                    status,
                                    fee,
                                    pre_balances,
                                    post_balances,
                                    inner_instructions,
                                    log_messages,
                                    pre_token_balances,
                                    post_token_balances,
                                    rewards,
                                },
                            )
                            .expect("Expect database write to succeed");
                    }
                }
            }
            TransactionStatusMessage::Freeze(slot) => {
                max_complete_transaction_status_slot.fetch_max(slot, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
