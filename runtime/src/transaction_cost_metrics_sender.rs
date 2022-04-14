use {
    crate::{bank::Bank, cost_model::CostModel},
    crossbeam_channel::{Receiver, Sender},
    log::*,
    solana_sdk::{clock::Slot, signature::Signature, transaction::SanitizedTransaction},
    std::{
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
};

pub enum TransactionCostMetrics {
    TransactionCostDetail {
        slot: Slot,
        tx_signature: Signature,
        signature_cost: u64,
        write_lock_cost: u64,
        data_bytes_cost: u64,
        builtins_execution_cost: u64,
        bpf_execution_cost: u64,
    },
}

pub struct TransactionCostMetricsSender {
    cost_model: Arc<RwLock<CostModel>>,
    metrics_sender: Sender<TransactionCostMetrics>,
}

impl TransactionCostMetricsSender {
    pub fn new(
        cost_model: Arc<RwLock<CostModel>>,
        metrics_sender: Sender<TransactionCostMetrics>,
    ) -> Self {
        Self {
            cost_model,
            metrics_sender,
        }
    }

    pub fn send_cost_details<'a>(
        &self,
        bank: Arc<Bank>,
        txs: impl Iterator<Item = &'a SanitizedTransaction>,
    ) {
        let cost_model = self.cost_model.read().unwrap();
        txs.for_each(|tx| {
            let cost = cost_model.calculate_cost(tx);
            self.metrics_sender
                .send(TransactionCostMetrics::TransactionCostDetail {
                    slot: bank.slot(),
                    tx_signature: *tx.signature(),
                    signature_cost: cost.signature_cost,
                    write_lock_cost: cost.write_lock_cost,
                    data_bytes_cost: cost.data_bytes_cost,
                    builtins_execution_cost: cost.builtins_execution_cost,
                    bpf_execution_cost: cost.bpf_execution_cost,
                })
                .unwrap_or_else(|err| {
                    warn!(
                        "transaction cost metrics service report cost detail failed: {:?}",
                        err
                    )
                });
        });
    }
}

pub struct TransactionCostMetricsService {
    thread_hdl: JoinHandle<()>,
}

impl TransactionCostMetricsService {
    pub fn new(transaction_cost_metrics_receiver: Receiver<TransactionCostMetrics>) -> Self {
        let thread_hdl = Builder::new()
            .name("transaction_cost_metrics_service".to_string())
            .spawn(move || {
                Self::service_loop(transaction_cost_metrics_receiver);
            })
            .unwrap();

        Self { thread_hdl }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }

    fn service_loop(transaction_cost_metrics_receiver: Receiver<TransactionCostMetrics>) {
        for tx_cost_metrics in transaction_cost_metrics_receiver.iter() {
            match tx_cost_metrics {
                TransactionCostMetrics::TransactionCostDetail {
                    slot,
                    tx_signature,
                    signature_cost,
                    write_lock_cost,
                    data_bytes_cost,
                    builtins_execution_cost,
                    bpf_execution_cost,
                } => {
                    // report transaction cost details per slot|signature
                    datapoint_trace!(
                        "transaction-cost-details",
                        ("slot", slot as i64, i64),
                        ("tx_signature", tx_signature.to_string(), String),
                        ("signature_cost", signature_cost as i64, i64),
                        ("write_lock_cost", write_lock_cost as i64, i64),
                        ("data_bytes_cost", data_bytes_cost as i64, i64),
                        (
                            "builtins_execution_cost",
                            builtins_execution_cost as i64,
                            i64
                        ),
                        ("bpf_execution_cost", bpf_execution_cost as i64, i64),
                    );
                }
            }
        }
    }
}
