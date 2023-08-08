use {
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    solana_accounts_db::stake_rewards::RewardInfo,
    solana_ledger::blockstore::Blockstore,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    solana_transaction_status::Reward,
    std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub type RewardsBatch = (Slot, Vec<(Pubkey, RewardInfo)>);
pub type RewardsRecorderReceiver = Receiver<RewardsMessage>;
pub type RewardsRecorderSender = Sender<RewardsMessage>;

pub enum RewardsMessage {
    Batch(RewardsBatch),
    Complete(Slot),
}

pub struct RewardsRecorderService {
    thread_hdl: JoinHandle<()>,
}

impl RewardsRecorderService {
    pub fn new(
        rewards_receiver: RewardsRecorderReceiver,
        max_complete_rewards_slot: Arc<AtomicU64>,
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solRewardsWritr".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(RecvTimeoutError::Disconnected) =
                    Self::write_rewards(&rewards_receiver, &max_complete_rewards_slot, &blockstore)
                {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn write_rewards(
        rewards_receiver: &RewardsRecorderReceiver,
        max_complete_rewards_slot: &Arc<AtomicU64>,
        blockstore: &Blockstore,
    ) -> Result<(), RecvTimeoutError> {
        match rewards_receiver.recv_timeout(Duration::from_secs(1))? {
            RewardsMessage::Batch((slot, rewards)) => {
                let rpc_rewards = rewards
                    .into_iter()
                    .map(|(pubkey, reward_info)| Reward {
                        pubkey: pubkey.to_string(),
                        lamports: reward_info.lamports,
                        post_balance: reward_info.post_balance,
                        reward_type: Some(reward_info.reward_type),
                        commission: reward_info.commission,
                    })
                    .collect();

                blockstore
                    .write_rewards(slot, rpc_rewards)
                    .expect("Expect database write to succeed");
            }
            RewardsMessage::Complete(slot) => {
                max_complete_rewards_slot.fetch_max(slot, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
