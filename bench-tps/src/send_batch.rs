use {
    crate::bench_tps_client::*,
    log::*,
    rayon::prelude::*,
    solana_core::gen_keys::GenKeys,
    solana_measure::measure::Measure,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        message::Message,
        nonce::State,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signer::signers::Signers,
        system_instruction,
        transaction::Transaction,
    },
    std::{
        collections::HashSet,
        marker::Send,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
};

pub fn get_latest_blockhash<T: BenchTpsClient + ?Sized>(client: &T) -> Hash {
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

pub fn generate_keypairs(seed_keypair: &Keypair, count: u64) -> (Vec<Keypair>, u64) {
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_keypair.to_bytes()[..32]);
    let mut rnd = GenKeys::new(seed);

    let mut total_keys = 0;
    let mut extra = 0; // This variable tracks the number of keypairs needing extra transaction fees funded
    let mut delta = 1;
    while total_keys < count {
        extra += delta;
        delta *= MAX_SPENDS_PER_TX;
        total_keys += delta;
    }
    (rnd.gen_n_keypairs(total_keys), extra)
}

/// fund the dests keys by spending all of the source keys into MAX_SPENDS_PER_TX
/// on every iteration.  This allows us to replay the transfers because the source is either empty,
/// or full
pub fn fund_keys<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    source: &Keypair,
    dests: &[Keypair],
    total: u64,
    max_fee: u64,
    lamports_per_account: u64,
    data_size_limit: u32,
) {
    let mut funded: Vec<&Keypair> = vec![source];
    let mut funded_funds = total;
    let mut not_funded: Vec<&Keypair> = dests.iter().collect();
    while !not_funded.is_empty() {
        // Build to fund list and prepare funding sources for next iteration
        let mut new_funded: Vec<&Keypair> = vec![];
        let mut to_fund: Vec<(&Keypair, Vec<(Pubkey, u64)>)> = vec![];
        let to_lamports = (funded_funds - lamports_per_account - max_fee) / MAX_SPENDS_PER_TX;
        for f in funded {
            let start = not_funded.len() - MAX_SPENDS_PER_TX as usize;
            let dests: Vec<_> = not_funded.drain(start..).collect();
            let spends: Vec<_> = dests.iter().map(|k| (k.pubkey(), to_lamports)).collect();
            to_fund.push((f, spends));
            new_funded.extend(dests.into_iter());
        }

        to_fund.chunks(FUND_CHUNK_LEN).for_each(|chunk| {
            Vec::<(&Keypair, Transaction)>::with_capacity(chunk.len()).fund(
                &client,
                chunk,
                to_lamports,
                data_size_limit,
            );
        });

        info!("funded: {} left: {}", new_funded.len(), not_funded.len());
        funded = new_funded;
        funded_funds = to_lamports;
    }
}

pub fn generate_durable_nonce_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    authority_keypairs: &[Keypair],
) -> Vec<Keypair> {
    let nonce_rent = client
        .get_minimum_balance_for_rent_exemption(State::size())
        .unwrap();

    let seed_keypair = &authority_keypairs[0];
    let count = authority_keypairs.len();
    let (mut nonce_keypairs, _extra) = generate_keypairs(seed_keypair, count as u64);
    nonce_keypairs.truncate(count);

    info!("Creating {} nonce accounts...", count);
    let to_fund: Vec<NonceCreateSigners> = authority_keypairs
        .iter()
        .zip(nonce_keypairs.iter())
        .map(|x| NonceCreateSigners(x.0, x.1))
        .collect();

    to_fund.chunks(FUND_CHUNK_LEN).for_each(|chunk| {
        NonceCreateContainer::with_capacity(chunk.len())
            .create_accounts(&client, chunk, nonce_rent);
    });
    nonce_keypairs
}

pub fn withdraw_durable_nonce_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    authority_keypairs: &[Keypair],
    nonce_keypairs: &[Keypair],
) {
    let to_withdraw: Vec<NonceWithdrawSigners> = authority_keypairs
        .iter()
        .zip(nonce_keypairs.iter())
        .map(|x| NonceWithdrawSigners(x.0, x.1.pubkey()))
        .collect();

    to_withdraw.chunks(FUND_CHUNK_LEN).for_each(|chunk| {
        NonceWithdrawContainer::with_capacity(chunk.len()).withdraw_accounts(&client, chunk);
    });
}

const MAX_SPENDS_PER_TX: u64 = 4;

// Size of the chunk of transactions
// try to transfer a "few" at a time with recent blockhash
// assume 4MB network buffers, and 512 byte packets
const FUND_CHUNK_LEN: usize = 4 * 1024 * 1024 / 512;

fn verify_funding_transfer<T: BenchTpsClient + ?Sized>(
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

/// Helper trait to encapsulate common logic for sending transactions batch
///
trait SendBatchTransactions<'a, T: Sliceable + Send + Sync> {
    fn make<V: Send + Sync, F: Fn(&V) -> (T, Transaction) + Send + Sync>(
        &mut self,
        chunk: &[V],
        create_transaction: F,
    );
    fn send_transactions<C, F>(&mut self, client: &Arc<C>, to_lamports: u64, log_progress: F)
    where
        C: 'static + BenchTpsClient + Send + Sync + ?Sized,
        F: Fn(usize, usize);
    fn sign(&mut self, blockhash: Hash);
    fn send<C: BenchTpsClient + ?Sized>(&self, client: &Arc<C>);
    fn verify<C: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<C>,
        to_lamports: u64,
    );
}

/// This trait allows reuse SendBatchTransactions to send
/// transactions which require more than one signature
trait Sliceable {
    type Slice;
    fn as_slice(&self) -> Self::Slice;
    // Pubkey used as unique id to identify verified transactions
    fn get_pubkey(&self) -> Pubkey;
}

impl<'a, T: Sliceable + Send + Sync> SendBatchTransactions<'a, T> for Vec<(T, Transaction)>
where
    <T as Sliceable>::Slice: Signers,
{
    fn make<V: Send + Sync, F: Fn(&V) -> (T, Transaction) + Send + Sync>(
        &mut self,
        chunk: &[V],
        create_transaction: F,
    ) {
        let mut make_txs = Measure::start("make_txs");
        let txs: Vec<(T, Transaction)> = chunk.par_iter().map(create_transaction).collect();
        make_txs.stop();
        debug!("make {} unsigned txs: {}us", txs.len(), make_txs.as_us());
        self.extend(txs);
    }

    fn send_transactions<C, F>(&mut self, client: &Arc<C>, to_lamports: u64, log_progress: F)
    where
        C: 'static + BenchTpsClient + Send + Sync + ?Sized,
        F: Fn(usize, usize),
    {
        let mut tries: usize = 0;
        while !self.is_empty() {
            log_progress(tries, self.len());
            let blockhash = get_latest_blockhash(client.as_ref());

            // re-sign retained to_fund_txes with updated blockhash
            self.sign(blockhash);
            self.send(client);

            // Sleep a few slots to allow transactions to process
            sleep(Duration::from_secs(1));

            self.verify(client, to_lamports);

            // retry anything that seems to have dropped through cracks
            //  again since these txs are all or nothing, they're fine to
            //  retry
            tries += 1;
        }
        info!("transactions sent in {} tries", tries);
    }

    fn sign(&mut self, blockhash: Hash) {
        let mut sign_txs = Measure::start("sign_txs");
        self.par_iter_mut().for_each(|(k, tx)| {
            tx.sign(&k.as_slice(), blockhash);
        });
        sign_txs.stop();
        debug!("sign {} txs: {}us", self.len(), sign_txs.as_us());
    }

    fn send<C: BenchTpsClient + ?Sized>(&self, client: &Arc<C>) {
        let mut send_txs = Measure::start("send_and_clone_txs");
        let batch: Vec<_> = self.iter().map(|(_keypair, tx)| tx.clone()).collect();
        let result = client.send_batch(batch);
        send_txs.stop();
        if result.is_err() {
            debug!("Failed to send batch {result:?}");
        } else {
            debug!("send {} {}", self.len(), send_txs);
        }
    }

    fn verify<C: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<C>,
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
                    let pubkey = k.get_pubkey();
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

            self.retain(|(k, _)| !verified_set.contains(&k.get_pubkey()));
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

type FundingSigners<'a> = &'a Keypair;
type FundingChunk<'a> = [(FundingSigners<'a>, Vec<(Pubkey, u64)>)];
type FundingContainer<'a> = Vec<(FundingSigners<'a>, Transaction)>;

impl<'a> Sliceable for FundingSigners<'a> {
    type Slice = [FundingSigners<'a>; 1];
    fn as_slice(&self) -> Self::Slice {
        [self]
    }
    fn get_pubkey(&self) -> Pubkey {
        self.pubkey()
    }
}

trait FundingTransactions<'a>: SendBatchTransactions<'a, FundingSigners<'a>> {
    fn fund<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<T>,
        to_fund: &FundingChunk<'a>,
        to_lamports: u64,
        data_size_limit: u32,
    );
}

impl<'a> FundingTransactions<'a> for FundingContainer<'a> {
    fn fund<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<T>,
        to_fund: &FundingChunk<'a>,
        to_lamports: u64,
        data_size_limit: u32,
    ) {
        self.make(to_fund, |(k, t)| -> (FundingSigners<'a>, Transaction) {
            let mut instructions = system_instruction::transfer_many(&k.pubkey(), t);
            instructions.push(
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(data_size_limit),
            );
            let message = Message::new(&instructions, Some(&k.pubkey()));
            (*k, Transaction::new_unsigned(message))
        });

        let log_progress = |tries: usize, batch_len: usize| {
            info!(
                "{} {} each to {} accounts in {} txs",
                if tries == 0 {
                    "transferring"
                } else {
                    " retrying"
                },
                to_lamports,
                batch_len * MAX_SPENDS_PER_TX as usize,
                batch_len,
            );
        };
        self.send_transactions(client, to_lamports, log_progress);
    }
}

// Introduce a new structure to specify Sliceable implementations
// which uses both Keypairs to sign the transaction
struct NonceCreateSigners<'a>(&'a Keypair, &'a Keypair);
type NonceCreateChunk<'a> = [NonceCreateSigners<'a>];
type NonceCreateContainer<'a> = Vec<(NonceCreateSigners<'a>, Transaction)>;

impl<'a> Sliceable for NonceCreateSigners<'a> {
    type Slice = [&'a Keypair; 2];
    fn as_slice(&self) -> Self::Slice {
        [self.0, self.1]
    }
    fn get_pubkey(&self) -> Pubkey {
        self.0.pubkey()
    }
}

trait NonceTransactions<'a>: SendBatchTransactions<'a, NonceCreateSigners<'a>> {
    fn create_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<T>,
        to_fund: &'a NonceCreateChunk<'a>,
        nonce_rent: u64,
    );
}

impl<'a> NonceTransactions<'a> for NonceCreateContainer<'a> {
    fn create_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<T>,
        to_fund: &'a NonceCreateChunk<'a>,
        nonce_rent: u64,
    ) {
        self.make(to_fund, |kp| -> (NonceCreateSigners<'a>, Transaction) {
            let authority = kp.0;
            let nonce: &Keypair = kp.1;
            let instructions = system_instruction::create_nonce_account(
                &authority.pubkey(),
                &nonce.pubkey(),
                &authority.pubkey(),
                nonce_rent,
            );
            (
                NonceCreateSigners(authority, nonce),
                Transaction::new_with_payer(&instructions, Some(&authority.pubkey())),
            )
        });

        let log_progress = |tries: usize, batch_len: usize| {
            info!(
                "@ {} {} accounts",
                if tries == 0 { "creating" } else { " retrying" },
                batch_len,
            );
        };
        self.send_transactions(client, nonce_rent, log_progress);
    }
}

// Only Pubkey is required for nonce because it doesn't sign withdraw account transaction
struct NonceWithdrawSigners<'a>(&'a Keypair, Pubkey);
type NonceWithdrawChunk<'a> = [NonceWithdrawSigners<'a>];
type NonceWithdrawContainer<'a> = Vec<(NonceWithdrawSigners<'a>, Transaction)>;

impl<'a> Sliceable for NonceWithdrawSigners<'a> {
    type Slice = [&'a Keypair; 1];
    fn as_slice(&self) -> Self::Slice {
        [self.0]
    }
    fn get_pubkey(&self) -> Pubkey {
        self.0.pubkey()
    }
}

trait NonceWithdrawTransactions<'a>: SendBatchTransactions<'a, NonceWithdrawSigners<'a>> {
    fn withdraw_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<T>,
        to_withdraw: &'a NonceWithdrawChunk<'a>,
    );
}
impl<'a> NonceWithdrawTransactions<'a> for NonceWithdrawContainer<'a> {
    fn withdraw_accounts<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
        &mut self,
        client: &Arc<T>,
        to_withdraw: &'a NonceWithdrawChunk<'a>,
    ) {
        self.make(
            to_withdraw,
            |kp| -> (NonceWithdrawSigners<'a>, Transaction) {
                let authority = kp.0;
                let nonce_pubkey: Pubkey = kp.1;
                let nonce_balance = client.get_balance(&nonce_pubkey).unwrap();
                let instructions = vec![
                    system_instruction::withdraw_nonce_account(
                        &nonce_pubkey,
                        &authority.pubkey(),
                        &authority.pubkey(),
                        nonce_balance,
                    );
                    1
                ];
                (
                    NonceWithdrawSigners(authority, nonce_pubkey),
                    Transaction::new_with_payer(&instructions, Some(&authority.pubkey())),
                )
            },
        );

        let log_progress = |tries: usize, batch_len: usize| {
            info!(
                "@ {} {} accounts",
                if tries == 0 {
                    "withdrawing"
                } else {
                    " retrying"
                },
                batch_len,
            );
        };
        self.send_transactions(client, 0, log_progress);
    }
}
