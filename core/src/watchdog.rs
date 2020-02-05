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

struct WatchdogService {
    t_dog: JoinHandle<Result<()>>,
}

struct Watchdog {
    cluster_info: Arc<RwLock<ClusterInfo>>,
    bank_forks: Arc<RwLock<BankForks>>,
    since: u64,
}

const MIN_CLUSTER_AGREEMENT: f64 = 0.75;

impl Watchdog {
    fn verify(&mut self) -> bool {
        let root: Slot = self.bank_forks.read().unwrap().root();

        let anscestors: Vec<Slot> = self.bank_forks.read().unwrap().working_bank().ancestors();

        let acceptable_roots = self.acceptable_roots(root, ancestors);

        let slot_history = self
            .bank_forks
            .read()
            .unwrap()
            .working_bank()
            .get_sysvar_account(&sysvar::slot_history::id())
            .map(|account| SlotHistory::from_account(&account).unwrap())
            .unwrap_or_default();

        let slot_hashes = self
            .bank_forks
            .read()
            .unwrap()
            .working_bank()
            .get_sysvar_account(&sysvar::slot_hashes::id())
            .map(|account| SlotHashes::from_account(&account).unwrap())
            .unwrap_or_default();

        let vote_accounts: HashMap<Pubkey, (u64, Account)> = self
            .bank_forks
            .read()
            .unwrap()
            .working_bank()
            .epoch_vote_accounts();

        let (votes, ns1) = self.cluster_info.read().unwrap().get_votes(&self.since);
        let (epoch_slots, ns2) = self
            .cluster_info
            .read()
            .unwrap()
            .get_all_epoch_states(&self.since);
        self.since = std::min(ns1, ns2);
    }

    fn find_acceptable_roots(root: Slot, ancestors: Vec<Slot>) -> HashSet<Slot> {
        ancestors
            .into_iter()
            .filter(|x| x <= VOTE_THRESHOLD_DEPTH + 2)
            .collect()
    }

    fn find_root_votes(
        votes: Vec<Transaction>,
        acceptable_roots: HashSet<Slot>,
    ) -> HashMap<(Pubkey, Vec<Vote>)> {
        let mut root_votes = HashMap::new();
        votes.into_iter().for_each(|tx| {
            let decoded = Self::decode_votes(tx);
            decoded
                .into_iter()
                .filter(|(_, vote)| vote.slots.iter().any(|s| acceptable_roots.contains(s)))
                .for_each(|(key, vote)| root_votes.entry(key).or_insert(vec![]).push(vote));
        });
        root_votes
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
