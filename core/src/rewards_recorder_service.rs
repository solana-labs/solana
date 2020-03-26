use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use solana_ledger::blockstore::Blockstore;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use solana_transaction_status::Reward;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub type RewardsRecorderReceiver = Receiver<(Slot, Vec<(Pubkey, i64)>)>;
pub type RewardsRecorderSender = Sender<(Slot, Vec<(Pubkey, i64)>)>;

pub struct RewardsRecorderService {
    thread_hdl: JoinHandle<()>,
}

impl RewardsRecorderService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        rewards_receiver: RewardsRecorderReceiver,
        blockstore: Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-rewards-writer".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(RecvTimeoutError::Disconnected) =
                    Self::write_rewards(&rewards_receiver, &blockstore)
                {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn write_rewards(
        rewards_receiver: &RewardsRecorderReceiver,
        blockstore: &Arc<Blockstore>,
    ) -> Result<(), RecvTimeoutError> {
        let (slot, rewards) = rewards_receiver.recv_timeout(Duration::from_secs(1))?;
        let rpc_rewards = rewards
            .into_iter()
            .map(|(pubkey, lamports)| Reward {
                pubkey: pubkey.to_string(),
                lamports,
            })
            .collect();

        blockstore
            .write_rewards(slot, rpc_rewards)
            .expect("Expect database write to succeed");
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
