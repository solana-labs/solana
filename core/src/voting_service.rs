use {
    crate::tower_storage::{SavedTower, TowerStorage},
    crossbeam_channel::{unbounded, Receiver, Sender},
    lru::LruCache,
    solana_gossip::cluster_info::ClusterInfo,
    solana_measure::measure::Measure,
    solana_poh::poh_recorder::PohRecorder,
    solana_runtime::{bank_forks::BankForks, vote_parser},
    solana_sdk::{clock::Slot, hash::Hash, transaction::Transaction},
    std::{
        sync::{Arc, Mutex, RwLock},
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const OPTIMISTICALLY_CONFIRMED_SLOTS_LRU_CACHE_CAPACITY: usize = 512;
const GOSSIP_VOTE_DELAY: Duration = Duration::from_secs(2);

pub enum VoteOp {
    PushVote {
        tx: Transaction,
        tower_slots: Vec<Slot>,
        saved_tower: SavedTower,
    },
    RefreshVote {
        tx: Transaction,
        last_voted_slot: Slot,
    },
}

impl VoteOp {
    fn tx(&self) -> &Transaction {
        match self {
            VoteOp::PushVote { tx, .. } => tx,
            VoteOp::RefreshVote { tx, .. } => tx,
        }
    }
}

pub struct VotingService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl VotingService {
    pub fn new(
        vote_receiver: Receiver<VoteOp>,
        cluster_info: Arc<ClusterInfo>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
        tower_storage: Arc<dyn TowerStorage>,
        bank_forks: Arc<RwLock<BankForks>>,
        optimistically_confirmed_slots_receiver: Receiver<Vec<(Slot, Hash)>>,
    ) -> Self {
        let optimistically_confirmed_slots = Arc::new(Mutex::new(LruCache::new(
            OPTIMISTICALLY_CONFIRMED_SLOTS_LRU_CACHE_CAPACITY,
        )));
        let confirmed_slots_thread = {
            let cache = optimistically_confirmed_slots.clone();
            Builder::new()
                .name("confirmed-slots".to_string())
                .spawn(move || {
                    for confirmed_slots in optimistically_confirmed_slots_receiver.iter() {
                        let mut cache = cache.lock().unwrap();
                        for (slot, hash) in confirmed_slots {
                            cache.put(slot, hash);
                        }
                    }
                })
                .unwrap()
        };
        let (gossip_sender, gossip_receiver) = unbounded();
        let send_thread = {
            let cluster_info = cluster_info.clone();
            Builder::new()
                .name("sol-vote-service".to_string())
                .spawn(move || {
                    for vote_op in vote_receiver.iter() {
                        let rooted_bank = bank_forks.read().unwrap().root_bank();
                        let send_to_tpu_vote_port = rooted_bank.send_to_tpu_vote_port_enabled();
                        Self::handle_vote(
                            &cluster_info,
                            &poh_recorder,
                            tower_storage.as_ref(),
                            vote_op,
                            send_to_tpu_vote_port,
                            &gossip_sender,
                        );
                    }
                })
                .unwrap()
        };
        let gossip_thread = Builder::new()
            .name("gossip-votes".to_string())
            .spawn(move || {
                let mut num_votes = 0;
                let mut num_votes_gossiped = 0;
                let mut since = Instant::now();
                for (asof, vote) in gossip_receiver.iter() {
                    if since.elapsed() > Duration::from_secs(2) {
                        datapoint_info!(
                            "voting_service",
                            ("num_votes", num_votes, i64),
                            ("num_votes_gossiped", num_votes_gossiped, i64),
                        );
                        num_votes = 0;
                        num_votes_gossiped = 0;
                        since = Instant::now();
                    }
                    num_votes += 1;
                    let now = Instant::now();
                    let at = asof + GOSSIP_VOTE_DELAY;
                    if let Some(delay) = at.checked_duration_since(now) {
                        thread::sleep(delay);
                    }
                    let (slot, hash) = match vote_parser::parse_vote_transaction(&vote.tx()) {
                        None => continue,
                        Some((_, vote, _)) => match vote.last_voted_slot() {
                            None => continue,
                            Some(slot) => (slot, vote.hash()),
                        },
                    };
                    let optimistically_confirmed_slots =
                        optimistically_confirmed_slots.lock().unwrap();
                    if optimistically_confirmed_slots.peek(&slot) != Some(&hash) {
                        Self::gossip_vote(vote, &cluster_info);
                        num_votes_gossiped += 1;
                    }
                }
            })
            .unwrap();
        Self {
            thread_hdls: vec![confirmed_slots_thread, send_thread, gossip_thread],
        }
    }

    pub fn handle_vote(
        cluster_info: &ClusterInfo,
        poh_recorder: &Mutex<PohRecorder>,
        tower_storage: &dyn TowerStorage,
        vote_op: VoteOp,
        send_to_tpu_vote_port: bool,
        gossip_sender: &Sender<(Instant, VoteOp)>,
    ) {
        if let VoteOp::PushVote { saved_tower, .. } = &vote_op {
            let mut measure = Measure::start("tower_save-ms");
            if let Err(err) = tower_storage.store(saved_tower) {
                error!("Unable to save tower to storage: {:?}", err);
                std::process::exit(1);
            }
            measure.stop();
            inc_new_counter_info!("tower_save-ms", measure.as_ms() as usize);
        }

        let target_address = if send_to_tpu_vote_port {
            crate::banking_stage::next_leader_tpu_vote(cluster_info, poh_recorder)
        } else {
            crate::banking_stage::next_leader_tpu(cluster_info, poh_recorder)
        };
        let _ = cluster_info.send_transaction(vote_op.tx(), target_address);

        // TODO: consider pushing votes to gossip eagerly if:
        //   vote.slots().len() > 3.
        let _ = gossip_sender.send((Instant::now(), vote_op));
    }

    fn gossip_vote(vote_op: VoteOp, cluster_info: &ClusterInfo) {
        match vote_op {
            VoteOp::PushVote {
                tx, tower_slots, ..
            } => {
                cluster_info.push_vote(&tower_slots, tx);
            }
            VoteOp::RefreshVote {
                tx,
                last_voted_slot,
            } => {
                cluster_info.refresh_vote(tx, last_voted_slot);
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdls.into_iter().try_for_each(JoinHandle::join)
    }
}
