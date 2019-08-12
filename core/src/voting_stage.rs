use std::thread::{self, Builder, JoinHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use crate::result::{Error, Result};
use crate::replay_stage::ForkProgress;
use solana_runtime::bank::Bank;
use crate::bank_forks::BankForks;
use std::sync::{RwLock, Arc};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use crate::consensus::{Tower, StakeLockout};
use crate::blocktree::Blocktree;
use solana_sdk::signature::KeypairUtil;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_instruction;
use solana_sdk::transaction::Transaction;
use crate::cluster_info::ClusterInfo;
use crate::leader_schedule_cache::LeaderScheduleCache;
use crate::snapshot_package::SnapshotPackageSender;
use crate::replay_stage::LockoutAggregationData;
use crate::service::Service;

// Implement a destructor for the VotingStage thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}

// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct VotingStage {
    t_voting: JoinHandle<Result<()>>,
}

impl VotingStage {
    pub fn new(receiver: Receiver<(u64, Pubkey)>,
           exit: Arc<AtomicBool>,
           bank_forks: Arc<RwLock<BankForks>>,
           ) -> Self {
        let exit_ = exit.clone();

        let t_voting = Builder::new()
            .name("solana-voting-stage".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_.clone());
                loop {
                    if let Ok((slot, collector)) = receiver.recv() {
                        info!("slot: {}", slot);
                        if let Some(bank) = bank_forks.read().unwrap().get(slot) {
                            bank.calculate_hash();
                            Self::handle_votable_bank(bank);
                        }
                    }
                }
            });
        VotingStage {
            t_voting
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_votable_bank<T>(
        bank: &Arc<Bank>,
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &mut Tower,
        progress: &mut HashMap<u64, ForkProgress>,
        vote_account: &Pubkey,
        voting_keypair: &Option<Arc<T>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blocktree: &Arc<Blocktree>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        root_bank_sender: &Sender<Vec<Arc<Bank>>>,
        lockouts: HashMap<u64, StakeLockout>,
        lockouts_sender: &Sender<LockoutAggregationData>,
        snapshot_package_sender: &Option<SnapshotPackageSender>,
    ) -> Result<()>
    where
        T: 'static + KeypairUtil + Send + Sync,
    {
        trace!("handle votable bank {}", bank.slot());
        if let Some(new_root) = tower.record_vote(bank.slot(), bank.hash()) {
            // get the root bank before squash
            let root_bank = bank_forks
                .read()
                .unwrap()
                .get(new_root)
                .expect("Root bank doesn't exist")
                .clone();
            let mut rooted_banks = root_bank.parents();
            rooted_banks.push(root_bank);
            let rooted_slots: Vec<_> = rooted_banks.iter().map(|bank| bank.slot()).collect();
            blocktree
                .set_roots(&rooted_slots)
                .expect("Ledger set roots failed");
            // Set root first in leader schedule_cache before bank_forks because bank_forks.root
            // is consumed by repair_service to update gossip, so we don't want to get blobs for
            // repair on gossip before we update leader schedule, otherwise they may get dropped.
            leader_schedule_cache.set_root(rooted_banks.last().unwrap());
            bank_forks
                .write()
                .unwrap()
                .set_root(new_root, snapshot_package_sender);
            Self::handle_new_root(&bank_forks, progress);
            trace!("new root {}", new_root);
            if let Err(e) = root_bank_sender.send(rooted_banks) {
                trace!("root_bank_sender failed: {:?}", e);
                Err(e)?;
            }
        }
        Self::update_confidence_cache(bank_forks, tower, lockouts, lockouts_sender);
        tower.update_epoch(&bank);
        if let Some(ref voting_keypair) = voting_keypair {
            let node_keypair = cluster_info.read().unwrap().keypair.clone();

            // Send our last few votes along with the new one
            let vote_ix = vote_instruction::vote(
                &vote_account,
                &voting_keypair.pubkey(),
                tower.recent_votes(),
            );

            let mut vote_tx =
                Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));

            let blockhash = bank.last_blockhash();
            vote_tx.partial_sign(&[node_keypair.as_ref()], blockhash);
            vote_tx.partial_sign(&[voting_keypair.as_ref()], blockhash);
            cluster_info.write().unwrap().push_vote(vote_tx);
        }
        Ok(())
    }

    fn update_confidence_cache(
        bank_forks: &Arc<RwLock<BankForks>>,
        tower: &Tower,
        lockouts: HashMap<u64, StakeLockout>,
        lockouts_sender: &Sender<LockoutAggregationData>,
    ) {
        let total_epoch_stakes = tower.total_epoch_stakes();
        let mut w_bank_forks = bank_forks.write().unwrap();
        for (fork, stake_lockout) in lockouts.iter() {
            if tower.root().is_none() || *fork >= tower.root().unwrap() {
                w_bank_forks.cache_fork_confidence(
                    *fork,
                    stake_lockout.stake(),
                    total_epoch_stakes,
                    stake_lockout.lockout(),
                );
            }
        }
        drop(w_bank_forks);
        let bank_forks_clone = bank_forks.clone();
        let root = tower.root();

        if let Err(e) = lockouts_sender.send((lockouts, root, bank_forks_clone)) {
            trace!("lockouts_sender failed: {:?}", e);
        }
    }

    fn handle_new_root(
        bank_forks: &Arc<RwLock<BankForks>>,
        progress: &mut HashMap<u64, ForkProgress>,
    ) {
        let r_bank_forks = bank_forks.read().unwrap();
        progress.retain(|k, _| r_bank_forks.get(*k).is_some());
    }
}

impl Service for VotingStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_voting.join()?;
    }
}
