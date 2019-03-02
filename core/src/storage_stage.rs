// A stage that handles generating the keys used to encrypt the ledger and sample it
// for storage mining. Replicators submit storage proofs, validator then bundles them
// to submit its proof for mining to be rewarded.

use crate::blocktree::Blocktree;
#[cfg(all(feature = "chacha", feature = "cuda"))]
use crate::chacha_cuda::chacha_cbc_encrypt_file_many_keys;
use crate::client::mk_client_with_timeout;
use crate::cluster_info::ClusterInfo;
use crate::entry::{Entry, EntryReceiver};
use crate::result::{Error, Result};
use crate::service::Service;
use bincode::deserialize;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::storage_program::{self, StorageProgram, StorageTransaction};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program;
use std::collections::HashSet;
use std::io;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

// Block of hash answers to validate against
// Vec of [ledger blocks] x [keys]
type StorageResults = Vec<Hash>;
type StorageKeys = Vec<u8>;
type ReplicatorMap = Vec<HashSet<Pubkey>>;

#[derive(Default)]
pub struct StorageStateInner {
    storage_results: StorageResults,
    storage_keys: StorageKeys,
    replicator_map: ReplicatorMap,
    storage_block_hash: Hash,
    entry_height: u64,
}

#[derive(Clone, Default)]
pub struct StorageState {
    state: Arc<RwLock<StorageStateInner>>,
}

pub struct StorageStage {
    t_storage_mining_verifier: JoinHandle<()>,
    t_storage_create_accounts: JoinHandle<()>,
}

macro_rules! cross_boundary {
    ($start:expr, $len:expr, $boundary:expr) => {
        (($start + $len) & !($boundary - 1)) > $start & !($boundary - 1)
    };
}

pub const STORAGE_ROTATE_TEST_COUNT: u64 = 128;
// TODO: some way to dynamically size NUM_IDENTITIES
const NUM_IDENTITIES: usize = 1024;
pub const NUM_STORAGE_SAMPLES: usize = 4;
pub const ENTRIES_PER_SEGMENT: u64 = 16;
const KEY_SIZE: usize = 64;

type TransactionSender = Sender<Transaction>;

pub fn get_segment_from_entry(entry_height: u64) -> u64 {
    entry_height / ENTRIES_PER_SEGMENT
}

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
            entry_height: 0,
            storage_block_hash: Hash::default(),
        };

        StorageState {
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub fn get_mining_key(&self, key: &Signature) -> Vec<u8> {
        let idx = get_identity_index_from_signature(key);
        self.state.read().unwrap().storage_keys[idx..idx + KEY_SIZE].to_vec()
    }

    pub fn get_mining_result(&self, key: &Signature) -> Hash {
        let idx = get_identity_index_from_signature(key);
        self.state.read().unwrap().storage_results[idx]
    }

    pub fn get_storage_block_hash(&self) -> Hash {
        self.state.read().unwrap().storage_block_hash
    }

    pub fn get_entry_height(&self) -> u64 {
        self.state.read().unwrap().entry_height
    }

    pub fn get_pubkeys_for_entry_height(&self, entry_height: u64) -> Vec<Pubkey> {
        // TODO: keep track of age?
        const MAX_PUBKEYS_TO_RETURN: usize = 5;
        let index = (entry_height / ENTRIES_PER_SEGMENT) as usize;
        let replicator_map = &self.state.read().unwrap().replicator_map;
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
        blocktree: Option<Arc<Blocktree>>,
        keypair: &Arc<Keypair>,
        exit: &Arc<AtomicBool>,
        entry_height: u64,
        storage_rotate_count: u64,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Self {
        debug!("storage_stage::new: entry_height: {}", entry_height);
        storage_state.state.write().unwrap().entry_height = entry_height;
        let storage_state_inner = storage_state.state.clone();
        let exit0 = exit.clone();
        let keypair0 = keypair.clone();

        let (tx_sender, tx_receiver) = channel();

        let t_storage_mining_verifier = Builder::new()
            .name("solana-storage-mining-verify-stage".to_string())
            .spawn(move || {
                let mut poh_height = 0;
                let mut current_key = 0;
                let mut entry_height = entry_height;
                loop {
                    if let Some(ref some_blocktree) = blocktree {
                        if let Err(e) = Self::process_entries(
                            &keypair0,
                            &storage_state_inner,
                            &storage_entry_receiver,
                            &some_blocktree,
                            &mut poh_height,
                            &mut entry_height,
                            &mut current_key,
                            storage_rotate_count,
                            &tx_sender,
                        ) {
                            match e {
                                Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                                Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                                _ => info!("Error from process_entries: {:?}", e),
                            }
                        }
                    }
                    if exit0.load(Ordering::Relaxed) {
                        break;
                    }
                }
            })
            .unwrap();

        let cluster_info0 = cluster_info.clone();
        let exit1 = exit.clone();
        let keypair1 = keypair.clone();
        let t_storage_create_accounts = Builder::new()
            .name("solana-storage-create-accounts".to_string())
            .spawn(move || loop {
                match tx_receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(mut tx) => {
                        if Self::send_tx(&cluster_info0, &mut tx, &exit1, &keypair1, None).is_ok() {
                            debug!("sent tx: {:?}", tx);
                        }
                    }
                    Err(e) => match e {
                        RecvTimeoutError::Disconnected => break,
                        RecvTimeoutError::Timeout => (),
                    },
                };

                if exit1.load(Ordering::Relaxed) {
                    break;
                }
                sleep(Duration::from_millis(100));
            })
            .unwrap();

        StorageStage {
            t_storage_mining_verifier,
            t_storage_create_accounts,
        }
    }

    fn send_tx(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        tx: &mut Transaction,
        exit: &Arc<AtomicBool>,
        keypair: &Arc<Keypair>,
        account_to_create: Option<Pubkey>,
    ) -> io::Result<()> {
        if let Some(leader_info) = cluster_info.read().unwrap().leader_data() {
            let mut client = mk_client_with_timeout(leader_info, Duration::from_secs(5));

            if let Some(account) = account_to_create {
                if client.get_account_userdata(&account).is_ok() {
                    return Ok(());
                }
            }

            let mut block_hash = None;
            for _ in 0..10 {
                if let Some(new_block_hash) = client.try_get_recent_block_hash(1) {
                    block_hash = Some(new_block_hash);
                    break;
                }

                if exit.load(Ordering::Relaxed) {
                    Err(io::Error::new(io::ErrorKind::Other, "exit signaled"))?;
                }
            }

            if let Some(block_hash) = block_hash {
                tx.sign(&[keypair.as_ref()], block_hash);

                if exit.load(Ordering::Relaxed) {
                    Err(io::Error::new(io::ErrorKind::Other, "exit signaled"))?;
                }

                if let Ok(signature) = client.transfer_signed(&tx) {
                    for _ in 0..10 {
                        if client.check_signature(&signature) {
                            return Ok(());
                        }

                        if exit.load(Ordering::Relaxed) {
                            Err(io::Error::new(io::ErrorKind::Other, "exit signaled"))?;
                        }

                        sleep(Duration::from_millis(200));
                    }
                }
            }
        }

        Err(io::Error::new(io::ErrorKind::Other, "leader not found"))
    }

    pub fn process_entry_crossing(
        state: &Arc<RwLock<StorageStateInner>>,
        keypair: &Arc<Keypair>,
        _blocktree: &Arc<Blocktree>,
        entry_id: Hash,
        entry_height: u64,
        tx_sender: &TransactionSender,
    ) -> Result<()> {
        let mut seed = [0u8; 32];
        let signature = keypair.sign(&entry_id.as_ref());

        let tx = StorageTransaction::new_advertise_recent_block_hash(
            keypair,
            entry_id,
            Hash::default(),
            entry_height,
        );
        tx_sender.send(tx)?;

        seed.copy_from_slice(&signature.as_ref()[..32]);

        let mut rng = ChaChaRng::from_seed(seed);

        state.write().unwrap().entry_height = entry_height;

        // Regenerate the answers
        let num_segments = (entry_height / ENTRIES_PER_SEGMENT) as usize;
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

            let mut statew = state.write().unwrap();

            match chacha_cbc_encrypt_file_many_keys(
                _blocktree,
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

    pub fn process_entries(
        keypair: &Arc<Keypair>,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        entry_receiver: &EntryReceiver,
        blocktree: &Arc<Blocktree>,
        poh_height: &mut u64,
        entry_height: &mut u64,
        current_key_idx: &mut usize,
        storage_rotate_count: u64,
        tx_sender: &TransactionSender,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let entries: Vec<Entry> = entry_receiver.recv_timeout(timeout)?;
        for entry in entries {
            // Go through the transactions, find votes, and use them to update
            // the storage_keys with their signatures.
            for tx in entry.transactions {
                for (i, program_id) in tx.program_ids.iter().enumerate() {
                    if vote_program::check_id(&program_id) {
                        debug!(
                            "generating storage_keys from votes current_key_idx: {}",
                            *current_key_idx
                        );
                        let storage_keys = &mut storage_state.write().unwrap().storage_keys;
                        storage_keys[*current_key_idx..*current_key_idx + size_of::<Signature>()]
                            .copy_from_slice(tx.signatures[0].as_ref());
                        *current_key_idx += size_of::<Signature>();
                        *current_key_idx %= storage_keys.len();
                    } else if storage_program::check_id(&program_id) {
                        match deserialize(&tx.instructions[i].userdata) {
                            Ok(StorageProgram::SubmitMiningProof {
                                entry_height: proof_entry_height,
                                ..
                            }) => {
                                if proof_entry_height < *entry_height {
                                    let mut statew = storage_state.write().unwrap();
                                    let max_segment_index =
                                        (*entry_height / ENTRIES_PER_SEGMENT) as usize;
                                    if statew.replicator_map.len() <= max_segment_index {
                                        statew
                                            .replicator_map
                                            .resize(max_segment_index, HashSet::new());
                                    }
                                    let proof_segment_index =
                                        (proof_entry_height / ENTRIES_PER_SEGMENT) as usize;
                                    if proof_segment_index < statew.replicator_map.len() {
                                        statew.replicator_map[proof_segment_index]
                                            .insert(tx.account_keys[0]);
                                    }
                                }
                                debug!("storage proof: entry_height: {}", entry_height);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                info!("error: {:?}", e);
                            }
                        }
                    }
                }
            }
            if cross_boundary!(*poh_height, entry.num_hashes, storage_rotate_count) {
                trace!(
                    "crosses sending at poh_height: {} entry_height: {}! hashes: {}",
                    *poh_height,
                    entry_height,
                    entry.num_hashes
                );
                Self::process_entry_crossing(
                    &storage_state,
                    &keypair,
                    &blocktree,
                    entry.hash,
                    *entry_height,
                    tx_sender,
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
        self.t_storage_create_accounts.join().unwrap();
        self.t_storage_mining_verifier.join()
    }
}

#[cfg(test)]
mod tests {
    use crate::blocktree::{create_new_tmp_ledger, Blocktree};
    use crate::cluster_info::{ClusterInfo, NodeInfo};
    use crate::entry::{make_tiny_test_entries, Entry};
    use crate::service::Service;
    use crate::storage_stage::StorageState;
    use crate::storage_stage::NUM_IDENTITIES;
    use crate::storage_stage::{
        get_identity_index_from_signature, StorageStage, STORAGE_ROTATE_TEST_COUNT,
    };
    use rayon::prelude::*;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::hash::Hasher;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::vote_transaction::VoteTransaction;
    use std::cmp::{max, min};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_storage_stage_none_ledger() {
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let cluster_info = test_cluster_info(keypair.pubkey());

        let (_storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            None,
            &keypair,
            &exit.clone(),
            0,
            STORAGE_ROTATE_TEST_COUNT,
            &cluster_info,
        );
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();
    }

    fn test_cluster_info(id: Pubkey) -> Arc<RwLock<ClusterInfo>> {
        let node_info = NodeInfo::new_localhost(id, 0);
        let cluster_info = ClusterInfo::new(node_info);
        Arc::new(RwLock::new(cluster_info))
    }

    #[test]
    fn test_storage_stage_process_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (genesis_block, _mint_keypair) = GenesisBlock::new(1000);
        let ticks_per_slot = genesis_block.ticks_per_slot;
        let (ledger_path, _block_hash) = create_new_tmp_ledger!(&genesis_block);

        let entries = make_tiny_test_entries(64);
        let blocktree = Blocktree::open_config(&ledger_path, ticks_per_slot).unwrap();
        blocktree.write_entries(1, 0, 0, &entries).unwrap();

        let cluster_info = test_cluster_info(keypair.pubkey());

        let (storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            Some(Arc::new(blocktree)),
            &keypair,
            &exit.clone(),
            0,
            STORAGE_ROTATE_TEST_COUNT,
            &cluster_info,
        );
        storage_entry_sender.send(entries.clone()).unwrap();

        let keypair = Keypair::new();
        let hash = Hash::default();
        let signature = keypair.sign_message(&hash.as_ref());
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
    fn test_storage_stage_process_vote_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (genesis_block, _mint_keypair) = GenesisBlock::new(1000);
        let ticks_per_slot = genesis_block.ticks_per_slot;;
        let (ledger_path, _block_hash) = create_new_tmp_ledger!(&genesis_block);

        let entries = make_tiny_test_entries(128);
        let blocktree = Blocktree::open_config(&ledger_path, ticks_per_slot).unwrap();
        blocktree.write_entries(1, 0, 0, &entries).unwrap();

        let cluster_info = test_cluster_info(keypair.pubkey());

        let (storage_entry_sender, storage_entry_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            storage_entry_receiver,
            Some(Arc::new(blocktree)),
            &keypair,
            &exit.clone(),
            0,
            STORAGE_ROTATE_TEST_COUNT,
            &cluster_info,
        );
        storage_entry_sender.send(entries.clone()).unwrap();

        let mut reference_keys;
        {
            let keys = &storage_state.state.read().unwrap().storage_keys;
            reference_keys = vec![0; keys.len()];
            reference_keys.copy_from_slice(keys);
        }
        let mut vote_txs: Vec<_> = Vec::new();
        let keypair = Keypair::new();
        let vote_tx = VoteTransaction::new_vote(&keypair, 123456, Hash::default(), 1);
        vote_txs.push(vote_tx);
        let vote_entries = vec![Entry::new(&Hash::default(), 1, vote_txs)];
        storage_entry_sender.send(vote_entries).unwrap();

        for _ in 0..5 {
            {
                let keys = &storage_state.state.read().unwrap().storage_keys;
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
            let keys = &storage_state.state.read().unwrap().storage_keys;
            assert_ne!(keys[..], *reference_keys);
        }

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
                    let signature = keypair.sign_message(&hash.as_ref());
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
