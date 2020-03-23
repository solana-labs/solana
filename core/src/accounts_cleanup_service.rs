// Service to clean up dead slots in accounts_db
//
// This can be expensive since we have to walk the append vecs being cleaned up.

use solana_ledger::bank_forks::BankForks;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct AccountsCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl AccountsCleanupService {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>, exit: &Arc<AtomicBool>) -> Self {
        info!("AccountsCleanupService active");
        let exit = exit.clone();
        let t_cleanup = Builder::new()
            .name("solana-accounts-cleanup".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                let bank = bank_forks.read().unwrap().working_bank();
                bank.clean_dead_slots();
                sleep(Duration::from_millis(100));
            })
            .unwrap();
        Self { t_cleanup }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_cleanup.join()
    }
}
