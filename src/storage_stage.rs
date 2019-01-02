// A stage that handles generating the keys used to encrypt the ledger and sample it
// for storage mining. Replicators submit storage proofs, validator then bundles them
// to submit its proof for mining to be rewarded.

#[cfg(all(feature = "chacha", feature = "cuda"))]
use crate::chacha_cuda::chacha_cbc_encrypt_file_many_keys;
use crate::client::mk_client;
use crate::cluster_info::ClusterInfo;
use crate::db_ledger::DbLedger;
use crate::entry::EntryReceiver;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::thin_client::ThinClient;
use bincode::deserialize;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::storage_program;
use solana_sdk::storage_program::{get_segment_from_entry, StorageProgram, StorageTransaction};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;

// Block of hash answers to validate against
// Vec of [ledger blocks] x [keys]
type StorageResults = Vec<Hash>;
type StorageKeys = Vec<u8>;
type ReplicatorMap = Vec<HashSet<Pubkey>>;

#[derive(Default)]
pub struct StorageStateInner {
    storage_results: StorageResults,
    pub storage_keys: StorageKeys,
    replicator_map: ReplicatorMap,
}

#[derive(Default)]
pub struct StorageState {
    state: Arc<RwLock<StorageStateInner>>,
}

pub struct StorageStage {
    t_storage_mining_verifier: JoinHandle<()>,
}

macro_rules! cross_boundary {
    ($start:expr, $len:expr, $boundary:expr) => {
        (($start + $len) & !($boundary - 1)) > $start & !($boundary - 1)
    };
}

const NUM_HASHES_FOR_STORAGE_ROTATE: u64 = 32;
// TODO: some way to dynamically size NUM_IDENTITIES
const NUM_IDENTITIES: usize = 1024;
pub const NUM_STORAGE_SAMPLES: usize = 4;
const KEY_SIZE: usize = 64;

fn get_identity_index_from_signature(key: &Signature) -> usize {
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
        let storage_keys = vec![0u8; KEY_SIZE * NUM_IDENTITIES];
        let storage_results = vec![Hash::default(); NUM_IDENTITIES];
        let replicator_map = vec![];

        let state = StorageStateInner {
            storage_keys,
            storage_results,
            replicator_map,
        };

        StorageState {
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub fn get_mining_result(&self, key: &Signature) -> Hash {
        let idx = get_identity_index_from_signature(key);
        self.state.read().unwrap().storage_results[idx]
    }

    pub fn get_pubkeys_for_entry_height(&self, entry_height: u64) -> Vec<Pubkey> {
        // TODO: keep track of age?
        const MAX_PUBKEYS_TO_RETURN: usize = 5;
        let index = get_segment_from_entry(entry_height);
        let replicator_map = &self.state.read().unwrap().replicator_map;
        info!("replicator_map: {:?}", replicator_map);
        if index < replicator_map.len() {
            replicator_map[index]
                .iter()
                .cloned()
                .take(MAX_PUBKEYS_TO_RETURN)
                .collect::<Vec<_>>()
        } else {
            vec![]
        }
    }
}

impl StorageStage {
    pub fn new(
        storage_state: &StorageState,
        storage_entry_receiver: EntryReceiver,
        db_ledger: Option<Arc<DbLedger>>,
        keypair: Arc<Keypair>,
        exit: Arc<AtomicBool>,
        entry_height: u64,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Self {
        debug!("storage_stage::new: entry_height: {}", entry_height);
        let storage_state_inner = storage_state.state.clone();
        let cluster_info = cluster_info.clone();
        let t_storage_mining_verifier = Builder::new()
            .name("solana-storage-mining-verify-stage".to_string())
            .spawn(move || {
                let exit = exit.clone();
                let mut poh_height = 0;
                let mut entry_height = entry_height;
                loop {
                    if let Some(ref some_db_ledger) = db_ledger {
                        if let Err(e) = Self::process_entries(
                            &keypair,
                            &storage_state_inner,
                            &storage_entry_receiver,
                            &some_db_ledger,
                            &mut poh_height,
                            &mut entry_height,
                            &cluster_info,
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
            })
            .unwrap();

        StorageStage {
            t_storage_mining_verifier,
        }
    }

    fn create_storage_accounts(keypair: &Keypair, leader_client: &mut ThinClient) {
        let last_id = leader_client.get_last_id();
        match leader_client.get_account_userdata(&storage_program::system_id()) {
            Ok(_) => {}
            Err(e) => {
                debug!("error storage userdata: {:?}", e);

                let mut tx = Transaction::system_create(
                    keypair,
                    storage_program::system_id(),
                    last_id,
                    1,
                    16 * 1024,
                    storage_program::id(),
                    1,
                );
                if let Err(e) = leader_client.retry_transfer(keypair, &mut tx, 5) {
                    info!("Couldn't create storage account error: {}", e);
                }
            }
        }
        if leader_client
            .get_account_userdata(&keypair.pubkey())
            .is_err()
        {
            let mut tx = Transaction::system_create(
                keypair,
                keypair.pubkey(),
                last_id,
                1,
                0,
                storage_program::id(),
                1,
            );
            if let Err(e) = leader_client.retry_transfer(keypair, &mut tx, 5) {
                info!("Couldn't create storage account error: {}", e);
            }
        }
    }

    pub fn process_entry_crossing(
        _state: &Arc<RwLock<StorageStateInner>>,
        keypair: &Arc<Keypair>,
        _db_ledger: &Arc<DbLedger>,
        entry_id: Hash,
        entry_height: u64,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Result<()> {
        let mut seed = [0u8; 32];
        let signature = keypair.sign(&entry_id.as_ref());

        seed.copy_from_slice(&signature.as_ref()[..32]);

        let mut rng = ChaChaRng::from_seed(seed);

        if let Some(leader_data) = cluster_info.read().unwrap().leader_data() {
            let mut leader_client = mk_client(leader_data);

            Self::create_storage_accounts(keypair, &mut leader_client);

            let last_id = leader_client.get_last_id();

            debug!(
                "advertising new storage last id entry_height: {}!",
                entry_height
            );
            let mut tx = Transaction::storage_new_advertise_last_id(
                keypair,
                entry_id,
                last_id,
                entry_height,
            );
            if let Err(e) = leader_client.retry_transfer(keypair, &mut tx, 5) {
                info!(
                    "Couldn't advertise my storage last id to the network! error: {}",
                    e
                );
            }
        }

        // Regenerate the answers
        let num_segments = get_segment_from_entry(entry_height);
        if num_segments == 0 {
            info!("Ledger has 0 segments!");
            return Ok(());
        }
        // TODO: what if the validator does not have this segment
        let segment = signature.as_ref()[0] as usize % num_segments;

        debug!(
            "storage verifying: segment: {} identities: {}",
            segment, NUM_IDENTITIES,
        );

        let mut samples = vec![];
        for _ in 0..NUM_STORAGE_SAMPLES {
            samples.push(rng.gen_range(0, 10));
        }
        debug!("generated samples: {:?}", samples);
        // TODO: cuda required to generate the reference values
        // but if it is missing, then we need to take care not to
        // process storage mining results.
        #[cfg(all(feature = "chacha", feature = "cuda"))]
        {
            // Lock the keys, since this is the IV memory,
            // it will be updated in-place by the encryption.
            // Should be overwritten by the vote signatures which replace the
            // key values by the time it runs again.

            let mut statew = _state.write().unwrap();

            match chacha_cbc_encrypt_file_many_keys(
                _db_ledger,
                segment as u64,
                &mut statew.storage_keys,
                &samples,
            ) {
                Ok(hashes) => {
                    debug!("Success! encrypted ledger segment: {}", segment);
                    statew.storage_results.copy_from_slice(&hashes);
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

    fn process_storage_program(
        instruction_idx: usize,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        entry_height: u64,
        tx: &Transaction,
    ) {
        match deserialize(&tx.instructions[instruction_idx].userdata) {
            Ok(StorageProgram::SubmitMiningProof {
                entry_height: proof_entry_height,
                ..
            }) => {
                if proof_entry_height < entry_height {
                    let mut statew = storage_state.write().unwrap();
                    let max_segment_index = get_segment_from_entry(entry_height);
                    if statew.replicator_map.len() < max_segment_index {
                        statew
                            .replicator_map
                            .resize(max_segment_index, HashSet::new());
                    }
                    let proof_segment_index = get_segment_from_entry(proof_entry_height);
                    if proof_segment_index < statew.replicator_map.len() {
                        statew.replicator_map[proof_segment_index].insert(tx.account_keys[0]);
                    }
                }
                debug!(
                    "storage proof: max entry_height: {} proof height: {}",
                    entry_height, proof_entry_height
                );
            }
            Ok(StorageProgram::AdvertiseStorageLastId { id, .. }) => {
                debug!("id: {}", id);
            }
            Ok(StorageProgram::ClaimStorageReward { .. }) => {}
            Ok(StorageProgram::ProofValidation { .. }) => {}
            Err(e) => {
                info!("error: {:?}", e);
            }
        }
    }

    pub fn process_entries(
        keypair: &Arc<Keypair>,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        entry_receiver: &EntryReceiver,
        db_ledger: &Arc<DbLedger>,
        poh_height: &mut u64,
        entry_height: &mut u64,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries = entry_receiver.recv_timeout(timeout)?;
        info!(
            "processing: {} entries height: {} poh_height: {}",
            entries.len(),
            entry_height,
            poh_height
        );

        for entry in entries {
            // Go through the transactions, find votes, and use them to update
            // the storage_keys with their signatures.
            for tx in entry.transactions {
                for (i, program_id) in tx.program_ids.iter().enumerate() {
                    if storage_program::check_id(&program_id) {
                        Self::process_storage_program(i, storage_state, *entry_height, &tx);
                    }
                }
            }
            if cross_boundary!(*poh_height, entry.num_hashes, NUM_HASHES_FOR_STORAGE_ROTATE) {
                info!(
                    "crosses sending at poh_height: {} entry_height: {}! hashes: {}",
                    *poh_height, entry_height, entry.num_hashes
                );
                Self::process_entry_crossing(
                    &storage_state,
                    &keypair,
                    &db_ledger,
                    entry.id,
                    *entry_height,
                    cluster_info,
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
    use crate::cluster_info::ClusterInfo;
    use crate::cluster_info::NodeInfo;
    use crate::db_ledger::create_tmp_sample_ledger;
    use crate::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
    use crate::entry::{make_tiny_test_entries, Entry};

    use crate::service::Service;
    use crate::storage_stage::StorageState;
    use crate::storage_stage::NUM_IDENTITIES;
    use crate::storage_stage::{get_identity_index_from_signature, StorageStage};
    use rayon::prelude::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::hash::Hasher;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::storage_program::{StorageTransaction, ENTRIES_PER_SEGMENT};
    use solana_sdk::transaction::Transaction;
    use std::cmp::{max, min};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    fn test_cluster_info(id: Pubkey) -> Arc<RwLock<ClusterInfo>> {
        let node_info = NodeInfo::new_localhost(id, 0);
        let cluster_info = ClusterInfo::new(node_info);
        Arc::new(RwLock::new(cluster_info))
    }

    #[test]
    fn test_storage_stage_none_ledger() {
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (_storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();

        let cluster_info = test_cluster_info(keypair.pubkey());

        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            None,
            keypair,
            exit.clone(),
            0,
            &cluster_info,
        );
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();
    }

    #[test]
    fn test_storage_stage_process_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (_mint, ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "storage_stage_process_entries",
            1000,
            1,
            Keypair::new().pubkey(),
            1,
        );

        let entries = make_tiny_test_entries(128);
        let db_ledger = DbLedger::open(&ledger_path).unwrap();
        db_ledger
            .write_entries(DEFAULT_SLOT_HEIGHT, genesis_entries.len() as u64, &entries)
            .unwrap();

        let (storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();

        let cluster_info = test_cluster_info(keypair.pubkey());

        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            Some(Arc::new(db_ledger)),
            keypair,
            exit.clone(),
            0,
            &cluster_info,
        );
        storage_entry_sender.send(entries.clone()).unwrap();

        let keypair = Keypair::new();
        let hash = Hash::default();
        let signature = Signature::new(keypair.sign(&hash.as_ref()).as_ref());
        let mut result = storage_state.get_mining_result(&signature);
        assert_eq!(result, Hash::default());

        for _ in 0..9 {
            storage_entry_sender.send(entries.clone()).unwrap();
        }
        for _ in 0..5 {
            result = storage_state.get_mining_result(&signature);
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
    fn test_storage_stage_process_mining_proof_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (_mint, ledger_path, genesis_entries) = create_tmp_sample_ledger(
            "storage_stage_process_entries",
            1000,
            1,
            Keypair::new().pubkey(),
            1,
        );

        let entries = make_tiny_test_entries(ENTRIES_PER_SEGMENT as usize + 2);
        let db_ledger = DbLedger::open(&ledger_path).unwrap();
        db_ledger
            .write_entries(DEFAULT_SLOT_HEIGHT, genesis_entries.len() as u64, &entries)
            .unwrap();

        let cluster_info = test_cluster_info(keypair.pubkey());

        let (storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            Some(Arc::new(db_ledger)),
            keypair,
            exit.clone(),
            0,
            &cluster_info,
        );
        storage_entry_sender.send(entries.clone()).unwrap();

        let keypair = Keypair::new();
        let last_id = Hash::default();
        let signature = Signature::new(keypair.sign(&last_id.as_ref()).as_ref());
        let storage_tx = Transaction::storage_new_mining_proof(
            &keypair,
            Hash::default(),
            Hash::default(),
            0,
            signature,
        );
        let txs = vec![storage_tx];
        let storage_entries = vec![Entry::new(&Hash::default(), 0, 1, txs)];
        storage_entry_sender.send(storage_entries).unwrap();

        for _ in 0..5 {
            if storage_state.get_pubkeys_for_entry_height(0).len() != 0 {
                break;
            }

            sleep(Duration::new(1, 0));
            info!("pubkeys are empty");
        }

        info!("joining..?");
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();

        assert_eq!(
            storage_state.get_pubkeys_for_entry_height(0)[0],
            keypair.pubkey()
        );

        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_signature_distribution() {
        // See that signatures have an even-ish distribution..
        let mut hist = Arc::new(vec![]);
        for _ in 0..NUM_IDENTITIES {
            Arc::get_mut(&mut hist).unwrap().push(AtomicUsize::new(0));
        }
        let hasher = Hasher::default();
        {
            let hist = hist.clone();
            (0..(32 * NUM_IDENTITIES))
                .into_par_iter()
                .for_each(move |_| {
                    let keypair = Keypair::new();
                    let hash = hasher.clone().result();
                    let signature = Signature::new(keypair.sign(&hash.as_ref()).as_ref());
                    let ix = get_identity_index_from_signature(&signature);
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
