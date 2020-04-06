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

pub struct AccountsBackgroundService {
    t_background: JoinHandle<()>,
}

const INTERVAL_MS: u64 = 100;

impl AccountsBackgroundService {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>, exit: &Arc<AtomicBool>) -> Self {
        info!("AccountsBackgroundService active");
        let exit = exit.clone();
        let t_background = Builder::new()
            .name("solana-accounts-background".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                let bank = bank_forks.read().unwrap().working_bank();

                bank.process_dead_slots();

                // Currently, given INTERVAL_MS, we process 1 slot/100 ms
                bank.process_stale_slot();

                sleep(Duration::from_millis(INTERVAL_MS));
            })
            .unwrap();
        Self { t_background }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_background.join()
    }
}
