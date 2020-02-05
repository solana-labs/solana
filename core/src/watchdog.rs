use crate::cluster_info::ClusterInfo;
use solana_ledger::bank_forks::BankForks;
use solana_sdk::{
    instruction_processor_utils::limited_deserialize, slot_hashes::SlotHashes,
    slot_history::SlotHistory, timing::timestamp,
};
use solana_vote_program::vote_state::VoteState;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};

pub const MAX_ROOT_DELTA: usize = VOTE_THRESHOLD_DEPTH;

struct WatchdogService {
    t_dog: JoinHandle<Result<()>>,
}

struct Watchdog {
    cluster_info: Arc<RwLock<ClusterInfo>>,
    bank_forks: Arc<RwLock<BankForks>>,
    since_votes: u64,
    since_epochs: u64,
}

const MIN_CLUSTER_AGREEMENT: f64 = 0.75;

impl Watchdog {
    fn verify(&mut self) -> bool {
        let slot_hashes = self
            .bank_forks
            .read()
            .unwrap()
            .working_bank()
            .get_sysvar_account(&sysvar::slot_hashes::id())
            .map(|account| SlotHashes::from_account(&account).unwrap())
            .unwrap_or_default();
        let slots_hashes: HashMap<Slot,Hash> = slot_hashes.iter().collect();
        let hashes_slots: HashMap<Hash,Slot> = slot_hashes.iter().map(|(s,h)| (h,s)).collect();

        let vote_accounts: HashMap<Pubkey, (u64, Account)> = self
            .bank_forks
            .read()
            .unwrap()
            .working_bank()
            .epoch_vote_accounts();

        let (votes, ns1) = self
            .cluster_info
            .read()
            .unwrap()
            .get_votes(&self.since_votes);
        self.since_votes = ns1;
    }

    fn slot_hash_heat_map(
        vote_accounts: &HashMap<Pubkey, (u64, Account)>,
        hash_slots: &HashMap<Hash, Slot>,
        slot_hashes: &HashMap<Slot, Hash>,
        votes: &HashMap<Pubkey, Vec<Vote>>,
    ) -> HashMap<Hash, u64> {
        let mut heat_map = HashMap::new();
        for (key, val) in votes {
            let hashes: HashSet<Hash> = val
                .votes
                .iter()
                .flat_map(|v| {
                    let mut hss = vec![v.hash];
                    if hash_slots[v.hash] == v.slots[0] {
                        hss.extend(v.slots.iter().filter_map(|s| slot_hashes.get(s)))
                    }
                    hss
                })
                .collect();
            for hash in hashes {
                *heap_map.entry(hash).or_insert(0) +=
                    vote_accounts.get(key).map(|v| v.0).unwrap_or_default();
            }
        }
        heat_map
    }

    fn vote_heat_map(
        vote_accounts: &HashMap<Pubkey, (u64, Account)>,
        votes: &HashMap<Pubkey, Vec<Vote>>,
    ) -> HashMap<Slot, u64> {
        let mut heat_map = HashMap::new();
        for (key, val) in votes {
            let slots: HashSet<Slot> = val.votes.iter().flat_map(|v| v.slots).collect();
            for slot in slots {
                *heap_map.entry(slot).or_insert(0) +=
                    vote_accounts.get(key).map(|v| v.0).unwrap_or(0);
            }
        }
        heat_map
    }

    fn collect_votes(votes: Vec<Transaction>) -> HashMap<Pubkey, Vec<Vote>> {
        let mut votes = HashMap::new();
        votes.into_iter().for_each(|tx| {
            let decoded = Self::decode_votes(tx);
            decoded
                .into_iter()
                .for_each(|(key, vote)| votes.entry(key).or_insert(vec![]).push(vote));
        });
        votes
    }
    fn decode_votes(tx: Transaction) -> Vec<(Pubkey, Vote)> {
        tx.message
            .instructions
            .enumerate()
            .filter(|(i, ix)| {
                tx.message.account_keys.get(ixx.program_id_index) == solana_vote_program::id()
            })
            .filter_map(|(i, _)| {
                let VoteInstruction::Vote(vote) = limited_deserialize(tx.data(ix)).ok()?;
                Some((tx.key(i, 0)?, vote))
            })
            .collect()
    }
}

impl WatchdogService {
    fn new(
        cluster_info: Arc<RwLock<ClusterInfo>>,
        bank_forks: Arc<RwLock<BankForks>>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let t_dog = Builder::new()
            .name("solana-watchdog".to_string())
            .spawn(move || {
                let mut dog = Watchdog {
                    cluster_info,
                    bank_forks,
                };
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    if !dog.verify() {
                        panic!("CLUSTER CONSISTENCY WATCHDOG FAILURE");
                    }
                    thread::sleep(Duration::from_millis(1000));
                }
            });
        Self { t_dog }
    }
    pub fn join(self) -> thread::Result<()> {
        self.t_dog.join()
    }
}
