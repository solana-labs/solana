use crate::{
    cluster_info::ClusterInfo,
}
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
};
use solana_ledger::{
    bank_forks::BankForks,
};

struct WatchdogService { 
    t_dog: JoinHandle<Result<()>>,
}

struct Watchdog {
    cluster_info: Arc<RwLock<ClusterInfo>>,
    bank_forks: Arc<RwLock<BankForks>>,
}

impl Watchdog {
    fn verify(&self) -> bool {
        let slot_history = self.bank_forks.read().unlock().working_bank().get_sysvar_account(&sysvar::slot_history::id())
                .map(|account| SlotHistory::from_account(&account).unwrap())
                .unwrap_or_default();
        let vote_accounts = self.bank_forks.read().unlock().working_bank().epoch_vote_accounts();
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
                let mut dog = Watchdog { cluster_info, bank_forks };
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

