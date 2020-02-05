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
        let root: Slot = self.bank_forks.read().unwrap().root();

        let anscestors: Vec<Slot> = self.bank_forks.read().unwrap().working_bank().ancestors();

        let acceptable_roots = self.acceptable_roots(root, ancestors);

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

        let (votes, ns1) = self.cluster_info.read().unwrap().get_votes(&self.since_votes);
        self.since_votes = ns1;
        let (epoch_slots, ns2) = self
            .cluster_info
            .read()
            .unwrap()
            .get_all_epoch_states(&self.since_epochs);
        self.since_epochs = ns2;
    }

    fn compute_epoch_slot_agreement(
        vote_accounts: HashMap<Pubkey, (u64, Account)>,
        epoch_slots: HashMap<Pubkey, EpochSlot>)
    -> u64 {
        root_votes.iter().flat_map(|k,_| 
            vote_accounts.get(k).unwrap_or_default()
        ).sum()
    }

    fn find_root_epoch_slots(acceptable_roots: HashSet<Slot>, epoch_slots: Vec<EpochSlot>) -> HashMap<Pubkey, EpochSlot> {
        epoch_slots.into_iter().filter(|e| acceptable_roots.contains(e.slot)).map(|e| (e.from, e)).collect()
    }

    fn compute_root_vote_slot_agreement(
        vote_accounts: HashMap<Pubkey, (u64, Account)>,
        root_votes: HashMap<Pubkey, Vec<Vote>>)
    -> u64 {
        root_votes.iter().flat_map(|k,_| 
            vote_accounts.get(k).unwrap_or_default()
        ).sum()
    }

    //network should be within root +/- threshold blocks
    fn find_acceptable_roots(root: Slot, ancestors: Vec<Slot>) -> HashSet<Slot> {
        assert!(ancestors.is_empty() || ancestors.first() >= ancestors.last());
        let root_pos = ancestors
            .iter()
            .take_while(|x| x == root)
            .count();
        ancestors.into_iter().skip(std::MAX(0, root_pos - MAX_ROOT_DELTA)).take(2*MAX_ROOT_DELTA).collect()
    }

    fn find_root_votes(
        votes: Vec<Transaction>,
        acceptable_roots: HashSet<Slot>,
    ) -> HashMap<Pubkey, Vec<Vote>> {
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
