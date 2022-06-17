use {
    crate::bench_tps_client::*,
    log::*,
    solana_measure::measure::Measure,
    solana_sdk::{
        clock::{DEFAULT_MS_PER_SLOT, DEFAULT_S_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::Message,
        native_token::Sol,
        nonce::State,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction, system_transaction,
        timing::{duration_as_ms, duration_as_s, duration_as_us, timestamp},
        transaction::Transaction,
    },
    std::{
        sync::{
            atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering},
            Arc, Mutex, RwLock,
        },
        thread,
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
        collections::{HashSet, VecDeque},
    },
    rayon::prelude::*,
};

pub fn get_latest_blockhash<T: BenchTpsClient>(client: &T) -> Hash {
    loop {
        match client.get_latest_blockhash_with_commitment(CommitmentConfig::processed()) {
            Ok((blockhash, _)) => return blockhash,
            Err(err) => {
                info!("Couldn't get last blockhash: {:?}", err);
                sleep(Duration::from_secs(1));
            }
        };
    }
}

pub fn verify_funding_transfer<T: BenchTpsClient>(
    client: &Arc<T>,
    tx: &Transaction,
    amount: u64,
) -> bool {
    for a in &tx.message().account_keys[1..] {
        match client.get_balance_with_commitment(a, CommitmentConfig::processed()) {
            Ok(balance) => return balance >= amount,
            Err(err) => error!("failed to get balance {:?}", err),
        }
    }
    false
}

// TODO(klykov): try to use signatures
type Chunk<'a> = [(&'a Keypair, &'a Keypair)];
type Signers<'a> = (&'a Keypair, &'a Keypair);
pub type Base<'a> = Vec<(Signers<'a>, Transaction)>;

pub trait CreateNonceTransactions<'a> {
    fn create_accounts<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_fund: &'a Chunk<'a>,
        nonce_rent: u64,
    );
    fn make(&mut self, nonce_rent: u64, to_fund: &'a Chunk<'a>);
    fn sign(&mut self, blockhash: Hash);
    fn send<T: BenchTpsClient>(&self, client: &Arc<T>);
    fn verify<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_lamports: u64,
    );
}

impl<'a> CreateNonceTransactions<'a> for Base<'a> {
    fn create_accounts<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_fund: &'a Chunk<'a>,
        nonce_rent: u64,
    ) {
        self.make(nonce_rent, to_fund);

        let mut tries = 0;
        while !self.is_empty() {
            info!(
                "@ {} {} accounts",
                if tries == 0 {
                    "creating"
                } else {
                    " retrying"
                },
                self.len(),
            );

            let blockhash = get_latest_blockhash(client.as_ref());

            // re-sign retained to_fund_txes with updated blockhash
            self.sign(blockhash);
            self.send(client);

            // Sleep a few slots to allow transactions to process
            sleep(Duration::from_secs(1));

            self.verify(client, 1);

            // retry anything that seems to have dropped through cracks
            //  again since these txs are all or nothing, they're fine to
            //  retry
            tries += 1;
        }
        info!("transferred");
    }

    fn make(&mut self, nonce_rent: u64, to_fund: &'a Chunk<'a>) {
        let mut make_txs = Measure::start("make_txs");
        let to_fund_txs: Base = to_fund
            .par_iter()
            .map(|(authority, nonce)| {
                let instructions = system_instruction::create_nonce_account(
                    &authority.pubkey(),
                    &nonce.pubkey(),
                    &authority.pubkey(),
                    nonce_rent,
                );
                (
                    (*authority, *nonce),
                    Transaction::new_with_payer(&instructions, Some(&authority.pubkey())),
                )
            })
            .collect();
        make_txs.stop();
        debug!(
            "make {} unsigned txs: {}us",
            to_fund_txs.len(),
            make_txs.as_us()
        );
        self.extend(to_fund_txs);
    }

    fn sign(&mut self, blockhash: Hash) {
        let mut sign_txs = Measure::start("sign_txs");
        self.par_iter_mut().for_each(|(k, tx)| {
            tx.sign(&[k.0, k.1], blockhash);
        });
        sign_txs.stop();
        debug!("sign {} txs: {}us", self.len(), sign_txs.as_us());
    }

    fn send<T: BenchTpsClient>(&self, client: &Arc<T>) {
        let mut send_txs = Measure::start("send_and_clone_txs");
        let batch: Vec<_> = self.iter().map(|(_keypair, tx)| tx.clone()).collect();
        client.send_batch(batch).expect("transfer");
        send_txs.stop();
        debug!("send {} {}", self.len(), send_txs);
    }

    fn verify<T: 'static + BenchTpsClient + Send + Sync>(
        &mut self,
        client: &Arc<T>,
        to_lamports: u64,
    ) {
        let starting_txs = self.len();
        let verified_txs = Arc::new(AtomicUsize::new(0));
        let too_many_failures = Arc::new(AtomicBool::new(false));
        let loops = if starting_txs < 1000 { 3 } else { 1 };
        // Only loop multiple times for small (quick) transaction batches
        let time = Arc::new(Mutex::new(Instant::now()));
        for _ in 0..loops {
            let time = time.clone();
            let failed_verify = Arc::new(AtomicUsize::new(0));
            let client = client.clone();
            let verified_txs = &verified_txs;
            let failed_verify = &failed_verify;
            let too_many_failures = &too_many_failures;
            let verified_set: HashSet<Pubkey> = self
                .par_iter()
                .filter_map(move |(k, tx)| {
                    let pubkey = k.0.pubkey(); // Pubkey used as a key for the HashSet
                    // this pubkey is for authority so we have 1-1 mapping
                    if too_many_failures.load(Ordering::Relaxed) {
                        return None;
                    }

                    let verified = if verify_funding_transfer(&client, tx, to_lamports) {
                        verified_txs.fetch_add(1, Ordering::Relaxed);
                        Some(pubkey)
                    } else {
                        failed_verify.fetch_add(1, Ordering::Relaxed);
                        None
                    };

                    let verified_txs = verified_txs.load(Ordering::Relaxed);
                    let failed_verify = failed_verify.load(Ordering::Relaxed);
                    let remaining_count = starting_txs.saturating_sub(verified_txs + failed_verify);
                    if failed_verify > 100 && failed_verify > verified_txs {
                        too_many_failures.store(true, Ordering::Relaxed);
                        warn!(
                            "Too many failed transfers... {} remaining, {} verified, {} failures",
                            remaining_count, verified_txs, failed_verify
                        );
                    }
                    if remaining_count > 0 {
                        let mut time_l = time.lock().unwrap();
                        if time_l.elapsed().as_secs() > 2 {
                            info!(
                                "Verifying transfers... {} remaining, {} verified, {} failures",
                                remaining_count, verified_txs, failed_verify
                            );
                            *time_l = Instant::now();
                        }
                    }

                    verified
                })
                .collect();

                // here again assumption is that there is 1-1 mapping
            self.retain(|(k, _)| !verified_set.contains(&k.0.pubkey()));
            if self.is_empty() {
                break;
            }
            info!("Looping verifications");

            let verified_txs = verified_txs.load(Ordering::Relaxed);
            let failed_verify = failed_verify.load(Ordering::Relaxed);
            let remaining_count = starting_txs.saturating_sub(verified_txs + failed_verify);
            info!(
                "Verifying transfers... {} remaining, {} verified, {} failures",
                remaining_count, verified_txs, failed_verify
            );
            sleep(Duration::from_millis(100));
        }
    }
}
