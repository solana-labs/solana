// Service to purge 0 lamport accounts from the accounts_db.

use crate::service::Service;
use solana_ledger::bank_forks::BankForks;
use solana_measure::measure::Measure;
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

pub struct BankCleanupService {
    t_cleanup: JoinHandle<()>,
}

impl BankCleanupService {
    pub fn new(bank_forks: &Arc<RwLock<BankForks>>, exit: &Arc<AtomicBool>) -> Self {
        info!("BankCleanupService active.");
        let exit = exit.clone();
        let bank_forks = bank_forks.clone();
        let t_cleanup = Builder::new()
            .name("solana-bank-cleanup".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                let (bank, root) = {
                    let bank_forks = bank_forks.read().unwrap();
                    let banks = bank_forks.frozen_banks();
                    let root = bank_forks.root();
                    (banks[&root].clone(), root)
                };
                let mut time = Measure::start("purge_zero_lamport_accounts");
                bank.purge_zero_lamport_accounts();
                time.stop();
                info!("bank clean up {} took {}ms", root, time.as_ms());
                thread::sleep(Duration::from_secs(10));
            })
            .unwrap();
        Self { t_cleanup }
    }
}

impl Service for BankCleanupService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_cleanup.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use solana_runtime::bank::Bank;
    use std::thread::sleep;

    #[test]
    fn test_cleanup() {
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(1000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(
            &[bank.clone()],
            vec![0],
        )));

        let service = BankCleanupService::new(&bank_forks, &exit);
        for _ in 0..10 {
            if !bank_forks.read().unwrap()[0].has_accounts_with_zero_lamports() {
                break;
            }
            sleep(Duration::from_millis(10));
        }
        assert!(!bank_forks.read().unwrap()[0].has_accounts_with_zero_lamports());
        exit.store(true, Ordering::Relaxed);
        let _ = service.join();
    }
}
