use crate::rpc_subscriptions::RpcSubscriptions;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use solana_ledger::blockstore::{Blockstore, CompletedDataSetInfo};
use solana_sdk::signature::Signature;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub type CompletedDataSetsReceiver = Receiver<Vec<CompletedDataSetInfo>>;
pub type CompletedDataSetsSender = Sender<Vec<CompletedDataSetInfo>>;

pub struct CompletedDataSetsService {
    thread_hdl: JoinHandle<()>,
}

impl CompletedDataSetsService {
    pub fn new(
        completed_sets_receiver: CompletedDataSetsReceiver,
        blockstore: Arc<Blockstore>,
        rpc_subscriptions: Arc<RpcSubscriptions>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let thread_hdl = Builder::new()
            .name("completed-data-set-service".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(RecvTimeoutError::Disconnected) = Self::recv_completed_data_sets(
                    &completed_sets_receiver,
                    &blockstore,
                    &rpc_subscriptions,
                ) {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn recv_completed_data_sets(
        completed_sets_receiver: &CompletedDataSetsReceiver,
        blockstore: &Blockstore,
        rpc_subscriptions: &RpcSubscriptions,
    ) -> Result<(), RecvTimeoutError> {
        let completed_data_sets = completed_sets_receiver.recv_timeout(Duration::from_secs(1))?;
        for completed_set_info in std::iter::once(completed_data_sets)
            .chain(completed_sets_receiver.try_iter())
            .flatten()
        {
            let CompletedDataSetInfo {
                slot,
                start_index,
                end_index,
            } = completed_set_info;
            match blockstore.get_entries_in_data_block(slot, start_index, end_index, None) {
                Ok(entries) => {
                    let transactions = entries
                        .into_iter()
                        .flat_map(|e| e.transactions.into_iter().map(|t| t.signatures[0]))
                        .collect::<Vec<Signature>>();
                    if !transactions.is_empty() {
                        rpc_subscriptions.notify_signatures_received((slot, transactions));
                    }
                }
                Err(e) => warn!("completed-data-set-service deserialize error: {:?}", e),
            }
        }

        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
