// A stage that handles generating the keys used to encrypt the ledger and sample it
// for storage mining. Replicators submit storage proofs, validator then bundles them
// to submit its proof for mining to be rewarded.

#[cfg(all(feature = "chacha", feature = "cuda"))]
use chacha_cuda::chacha_cbc_encrypt_file_many_keys;
use entry::EntryReceiver;
use rand::{ChaChaRng, Rng, SeedableRng};
use result::{Error, Result};
use service::Service;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signature::Signature;
use solana_sdk::vote_program;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

// Block of hash answers to validate against
// Vec of [ledger blocks] x [keys]
type StorageResults = Vec<Hash>;
type StorageKeys = Vec<u8>;

#[derive(Default)]
pub struct StorageState {
    storage_results: Arc<RwLock<StorageResults>>,
    storage_keys: Arc<RwLock<StorageKeys>>,
}

pub struct StorageStage {
    t_storage_mining_verifier: JoinHandle<()>,
}

macro_rules! cross_boundary {
    ($start:expr, $len:expr, $boundary:expr) => {
        (($start + $len) & !($boundary - 1)) > $start & !($boundary - 1)
    };
}

const NUM_HASHES_FOR_STORAGE_ROTATE: u64 = 1024;
// TODO: some way to dynamically size NUM_IDENTITIES
const NUM_IDENTITIES: usize = 1024;
const NUM_SAMPLES: usize = 4;
pub const ENTRIES_PER_SLICE: u64 = 16;
const KEY_SIZE: usize = 64;

fn get_identity_index_from_pubkey(key: &Pubkey) -> usize {
    let rkey = key.as_ref();
    let mut res: usize = (rkey[0] as usize)
        | ((rkey[1] as usize) << 8)
        | ((rkey[2] as usize) << 16)
        | ((rkey[3] as usize) << 24);
    res &= NUM_IDENTITIES - 1;
    res
}

impl StorageState {
    pub fn new() -> Self {
        let storage_keys = Arc::new(RwLock::new(vec![0u8; KEY_SIZE * NUM_IDENTITIES]));
        let storage_results = Arc::new(RwLock::new(vec![Hash::default(); NUM_IDENTITIES]));

        StorageState {
            storage_keys,
            storage_results,
        }
    }

    pub fn get_mining_key(&self, key: &Pubkey) -> Vec<u8> {
        let idx = get_identity_index_from_pubkey(key);
        self.storage_keys.read().unwrap()[idx..idx + KEY_SIZE].to_vec()
    }

    pub fn get_mining_result(&self, key: &Pubkey) -> Hash {
        let idx = get_identity_index_from_pubkey(key);
        self.storage_results.read().unwrap()[idx]
    }
}

impl StorageStage {
    pub fn new(
        storage_state: &StorageState,
        storage_entry_receiver: EntryReceiver,
        ledger_path: Option<&str>,
        keypair: Arc<Keypair>,
        exit: Arc<AtomicBool>,
        entry_height: u64,
    ) -> Self {
        let storage_keys_ = storage_state.storage_keys.clone();
        let storage_results_ = storage_state.storage_results.clone();
        let ledger_path = ledger_path.map(String::from);
        let t_storage_mining_verifier = Builder::new()
            .name("solana-storage-mining-verify-stage".to_string())
            .spawn(move || {
                let exit = exit.clone();
                let mut poh_height = 0;
                let mut current_key = 0;
                let mut entry_height = entry_height;
                loop {
                    if let Some(ref ledger_path_str) = ledger_path {
                        if let Err(e) = Self::process_entries(
                            &keypair,
                            &storage_keys_,
                            &storage_results_,
                            &storage_entry_receiver,
                            ledger_path_str,
                            &mut poh_height,
                            &mut entry_height,
                            &mut current_key,
                        ) {
                            match e {
                                Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                                Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                                _ => info!("Error from process_entries: {:?}", e),
                            }
                        }
                    }
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }).unwrap();

        StorageStage {
            t_storage_mining_verifier,
        }
    }

    pub fn process_entry_crossing(
        _storage_results: &Arc<RwLock<StorageResults>>,
        _storage_keys: &Arc<RwLock<StorageKeys>>,
        keypair: &Arc<Keypair>,
        _ledger_path: &str,
        entry_id: Hash,
        entry_height: u64,
    ) -> Result<()> {
        let mut seed = [0u8; 32];
        let signature = keypair.sign(&entry_id.as_ref());

        seed.copy_from_slice(&signature.as_ref()[..32]);

        let mut rng = ChaChaRng::from_seed(seed);

        // Regenerate the answers
        let num_slices = (entry_height / ENTRIES_PER_SLICE) as usize;
        if num_slices == 0 {
            info!("Ledger has 0 slices!");
            return Ok(());
        }
        // TODO: what if the validator does not have this slice
        let slice = signature.as_ref()[0] as usize % num_slices;

        debug!(
            "storage verifying: slice: {} identities: {}",
            slice, NUM_IDENTITIES,
        );

        let mut samples = vec![];
        for _ in 0..NUM_SAMPLES {
            samples.push(rng.gen_range(0, 10));
        }
        debug!("generated samples: {:?}", samples);
        // TODO: cuda required to generate the reference values
        // but if it is missing, then we need to take care not to
        // process storage mining results.
        #[cfg(all(feature = "chacha", feature = "cuda"))]
        {
            let mut storage_results = _storage_results.write().unwrap();

            // Lock the keys, since this is the IV memory,
            // it will be updated in-place by the encryption.
            // Should be overwritten by the vote signatures which replace the
            // key values by the time it runs again.
            let mut storage_keys = _storage_keys.write().unwrap();

            match chacha_cbc_encrypt_file_many_keys(
                _ledger_path,
                slice as u64,
                &mut storage_keys,
                &samples,
            ) {
                Ok(hashes) => {
                    debug!("Success! encrypted ledger slice: {}", slice);
                    storage_results.copy_from_slice(&hashes);
                }
                Err(e) => {
                    info!("error encrypting file: {:?}", e);
                    Err(e)?;
                }
            }
        }
        // TODO: bundle up mining submissions from replicators
        // and submit them in a tx to the leader to get reward.
        Ok(())
    }

    pub fn process_entries(
        keypair: &Arc<Keypair>,
        storage_keys: &Arc<RwLock<StorageKeys>>,
        storage_results: &Arc<RwLock<StorageResults>>,
        entry_receiver: &EntryReceiver,
        ledger_path: &str,
        poh_height: &mut u64,
        entry_height: &mut u64,
        current_key_idx: &mut usize,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries = entry_receiver.recv_timeout(timeout)?;

        for entry in entries {
            // Go through the transactions, find votes, and use them to update
            // the storage_keys with their signatures.
            for tx in entry.transactions {
                for program_id in tx.program_ids {
                    if vote_program::check_id(&program_id) {
                        debug!(
                            "generating storage_keys from votes current_key_idx: {}",
                            *current_key_idx
                        );
                        let mut storage_keys = storage_keys.write().unwrap();
                        storage_keys[*current_key_idx..*current_key_idx + size_of::<Signature>()]
                            .copy_from_slice(tx.signatures[0].as_ref());
                        *current_key_idx += size_of::<Signature>();
                        *current_key_idx %= storage_keys.len();
                    }
                }
            }
            if cross_boundary!(*poh_height, entry.num_hashes, NUM_HASHES_FOR_STORAGE_ROTATE) {
                info!(
                    "crosses sending at poh_height: {} entry_height: {}! hashes: {}",
                    *poh_height, entry_height, entry.num_hashes
                );
                Self::process_entry_crossing(
                    &storage_results,
                    &storage_keys,
                    &keypair,
                    &ledger_path,
                    entry.id,
                    *entry_height,
                )?;
            }
            *entry_height += 1;
            *poh_height += entry.num_hashes;
        }
        Ok(())
    }
}

impl Service for StorageStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_storage_mining_verifier.join()
    }
}

#[cfg(test)]
mod tests {
    use entry::Entry;
    use ledger::make_tiny_test_entries;
    use ledger::{create_tmp_sample_ledger, LedgerWriter};
    use logger;
    use rayon::prelude::*;
    use service::Service;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;
    use solana_sdk::vote_program::Vote;
    use solana_sdk::vote_transaction::VoteTransaction;
    use std::cmp::{max, min};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use storage_stage::StorageState;
    use storage_stage::NUM_IDENTITIES;
    use storage_stage::{get_identity_index_from_pubkey, StorageStage};

    #[test]
    fn test_storage_stage_none_ledger() {
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (_storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            None,
            keypair,
            exit.clone(),
            0,
        );
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();
    }

    #[test]
    fn test_storage_stage_process_entries() {
        logger::setup();
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (_mint, ledger_path, _genesis) = create_tmp_sample_ledger(
            "storage_stage_process_entries",
            1000,
            1,
            Keypair::new().pubkey(),
            1,
        );

        let entries = make_tiny_test_entries(128);
        {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(&entries.clone()).unwrap();
            // drops writer, flushes buffers
        }

        let (storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            Some(&ledger_path),
            keypair,
            exit.clone(),
            0,
        );
        storage_entry_sender.send(entries.clone()).unwrap();

        let keypair = Keypair::new();
        let mut result = storage_state.get_mining_result(&keypair.pubkey());
        assert_eq!(result, Hash::default());

        for _ in 0..9 {
            storage_entry_sender.send(entries.clone()).unwrap();
        }
        for _ in 0..5 {
            result = storage_state.get_mining_result(&keypair.pubkey());
            if result != Hash::default() {
                info!("found result = {:?} sleeping..", result);
                break;
            }
            info!("result = {:?} sleeping..", result);
            sleep(Duration::new(1, 0));
        }

        info!("joining..?");
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();

        #[cfg(not(all(feature = "cuda", feature = "chacha")))]
        assert_eq!(result, Hash::default());

        #[cfg(all(feature = "cuda", feature = "chacha"))]
        assert_ne!(result, Hash::default());

        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_storage_stage_process_vote_entries() {
        logger::setup();
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (_mint, ledger_path, _genesis) = create_tmp_sample_ledger(
            "storage_stage_process_entries",
            1000,
            1,
            Keypair::new().pubkey(),
            1,
        );

        let entries = make_tiny_test_entries(128);
        {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(&entries.clone()).unwrap();
            // drops writer, flushes buffers
        }

        let (storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            Some(&ledger_path),
            keypair,
            exit.clone(),
            0,
        );
        storage_entry_sender.send(entries.clone()).unwrap();

        let mut reference_keys;
        {
            let keys = storage_state.storage_keys.read().unwrap();
            reference_keys = vec![0; keys.len()];
            reference_keys.copy_from_slice(&keys);
        }
        let mut vote_txs: Vec<Transaction> = Vec::new();
        let vote = Vote {
            tick_height: 123456,
        };
        let keypair = Keypair::new();
        let vote_tx = VoteTransaction::vote_new(&keypair, vote, Hash::default(), 1);
        vote_txs.push(vote_tx);
        let vote_entries = vec![Entry::new(&Hash::default(), 1, vote_txs)];
        storage_entry_sender.send(vote_entries).unwrap();

        for _ in 0..5 {
            {
                let keys = storage_state.storage_keys.read().unwrap();
                if keys[..] != *reference_keys.as_slice() {
                    break;
                }
            }

            sleep(Duration::new(1, 0));
        }

        debug!("joining..?");
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();

        {
            let keys = storage_state.storage_keys.read().unwrap();
            assert_ne!(keys[..], *reference_keys);
        }

        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_pubkey_distribution() {
        // See that pub keys have an even-ish distribution..
        let mut hist = Arc::new(vec![]);
        for _ in 0..NUM_IDENTITIES {
            Arc::get_mut(&mut hist).unwrap().push(AtomicUsize::new(0));
        }
        {
            let hist = hist.clone();
            (0..(32 * NUM_IDENTITIES))
                .into_par_iter()
                .for_each(move |_| {
                    let keypair = Keypair::new();
                    let ix = get_identity_index_from_pubkey(&keypair.pubkey());
                    hist[ix].fetch_add(1, Ordering::Relaxed);
                });
        }

        let mut hist_max = 0;
        let mut hist_min = NUM_IDENTITIES;
        for x in hist.iter() {
            let val = x.load(Ordering::Relaxed);
            hist_max = max(val, hist_max);
            hist_min = min(val, hist_min);
        }
        info!("min: {} max: {}", hist_min, hist_max);
        assert_ne!(hist_min, 0);
    }
}
