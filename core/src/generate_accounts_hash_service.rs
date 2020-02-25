// Service to generate the full accounts hash at some interval
// to combine into the bank vote hash.
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

pub struct GenerateAccountsHashService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl GenerateAccountsHashService {
    pub fn new(
        receiver: Receiver<(u64, Pubkey)>,
        bank_forks: &Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let bank_forks = bank_forks.clone();
        let thread_h = Builder::new()
            .name("solana-generate-accounts-hash".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                let timer = Duration::new(1, 0);
                if let Ok((slot, pubkey)) = receiver.recv_timeout(timer) {
                    info!("generate_accounts_hash processing: {:?} {}", slot, pubkey);
                    let bank = bank_forks.read().unwrap()[slot].clone();
                    Self::process_bank(&bank);
                }
            })
            .unwrap();

        GenerateAccountsHashService {
            thread_hdls: vec![thread_h],
        }
    }

    pub fn process_bank(bank: &Arc<Bank>) {
        if bank.is_snapshotable() {
            bank.update_accounts_hash();
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
