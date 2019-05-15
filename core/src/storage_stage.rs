// A stage that handles generating the keys used to encrypt the ledger and sample it
// for storage mining. Replicators submit storage proofs, validator then bundles them
// to submit its proof for mining to be rewarded.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
#[cfg(all(feature = "chacha", feature = "cuda"))]
use crate::chacha_cuda::chacha_cbc_encrypt_file_many_keys;
use crate::cluster_info::ClusterInfo;
use crate::result::{Error, Result};
use crate::service::Service;
use bincode::deserialize;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use solana_storage_api::storage_instruction::StorageInstruction;
use solana_storage_api::{get_segment_from_slot, storage_instruction};
use std::collections::HashSet;
use std::io;
use std::mem::size_of;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
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
    storage_blockhash: Hash,
    slot: u64,
}

#[derive(Clone, Default)]
pub struct StorageState {
    state: Arc<RwLock<StorageStateInner>>,
}

pub struct StorageStage {
    t_storage_mining_verifier: JoinHandle<()>,
    t_storage_create_accounts: JoinHandle<()>,
}

pub const STORAGE_ROTATE_TEST_COUNT: u64 = 2;
// TODO: some way to dynamically size NUM_IDENTITIES
const NUM_IDENTITIES: usize = 1024;
pub const NUM_STORAGE_SAMPLES: usize = 4;
const KEY_SIZE: usize = 64;

type InstructionSender = Sender<Instruction>;

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
            slot: 0,
            storage_blockhash: Hash::default(),
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

    pub fn get_storage_blockhash(&self) -> Hash {
        self.state.read().unwrap().storage_blockhash
    }

    pub fn get_slot(&self) -> u64 {
        self.state.read().unwrap().slot
    }

    pub fn get_pubkeys_for_slot(&self, slot: u64) -> Vec<Pubkey> {
        // TODO: keep track of age?
        const MAX_PUBKEYS_TO_RETURN: usize = 5;
        let index = get_segment_from_slot(slot) as usize;
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage_state: &StorageState,
        slot_receiver: Receiver<u64>,
        blocktree: Option<Arc<Blocktree>>,
        keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        exit: &Arc<AtomicBool>,
        bank_forks: &Arc<RwLock<BankForks>>,
        storage_rotate_count: u64,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Self {
        let (instruction_sender, instruction_receiver) = channel();

        let t_storage_mining_verifier = {
            let storage_state_inner = storage_state.state.clone();
            let exit = exit.clone();
            let storage_keypair = storage_keypair.clone();
            Builder::new()
                .name("solana-storage-mining-verify-stage".to_string())
                .spawn(move || {
                    let mut current_key = 0;
                    let mut slot_count = 0;
                    loop {
                        if let Some(ref some_blocktree) = blocktree {
                            if let Err(e) = Self::process_entries(
                                &storage_keypair,
                                &storage_state_inner,
                                &slot_receiver,
                                &some_blocktree,
                                &mut slot_count,
                                &mut current_key,
                                storage_rotate_count,
                                &instruction_sender,
                            ) {
                                match e {
                                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                                        break
                                    }
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
                .unwrap()
        };

        let t_storage_create_accounts = {
            let cluster_info = cluster_info.clone();
            let exit = exit.clone();
            let keypair = keypair.clone();
            let storage_keypair = storage_keypair.clone();
            let bank_forks = bank_forks.clone();
            Builder::new()
                .name("solana-storage-create-accounts".to_string())
                .spawn(move || {
                    let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
                    loop {
                        match instruction_receiver.recv_timeout(Duration::from_secs(1)) {
                            Ok(instruction) => {
                                Self::send_transaction(
                                    &bank_forks,
                                    &cluster_info,
                                    instruction,
                                    &keypair,
                                    &storage_keypair,
                                    &transactions_socket,
                                )
                                .unwrap_or_else(|err| {
                                    info!("failed to send storage transaction: {:?}", err)
                                });
                            }
                            Err(e) => match e {
                                RecvTimeoutError::Disconnected => break,
                                RecvTimeoutError::Timeout => (),
                            },
                        };

                        if exit.load(Ordering::Relaxed) {
                            break;
                        }
                        sleep(Duration::from_millis(100));
                    }
                })
                .unwrap()
        };

        StorageStage {
            t_storage_mining_verifier,
            t_storage_create_accounts,
        }
    }

    fn send_transaction(
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        instruction: Instruction,
        keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        transactions_socket: &UdpSocket,
    ) -> io::Result<()> {
        let working_bank = bank_forks.read().unwrap().working_bank();
        let blockhash = working_bank.confirmed_last_blockhash();
        let mut instructions = vec![];
        let signer_keys = vec![keypair.as_ref(), storage_keypair.as_ref()];
        if working_bank
            .get_account(&storage_keypair.pubkey())
            .is_none()
        {
            // TODO the account space needs to be well defined somewhere
            let create_instruction = system_instruction::create_account(
                &keypair.pubkey(),
                &storage_keypair.pubkey(),
                1000,
                1024 * 4,
                &solana_storage_api::id(),
            );
            instructions.push(create_instruction);
            info!("storage account requested");
        }
        instructions.push(instruction);
        let message = Message::new_with_payer(instructions, Some(&signer_keys[0].pubkey()));
        let transaction = Transaction::new(&signer_keys, message, blockhash);

        transactions_socket.send_to(
            &bincode::serialize(&transaction).unwrap(),
            cluster_info.read().unwrap().my_data().tpu,
        )?;
        Ok(())
    }

    fn process_entry_crossing(
        storage_keypair: &Arc<Keypair>,
        state: &Arc<RwLock<StorageStateInner>>,
        _blocktree: &Arc<Blocktree>,
        entry_id: Hash,
        slot: u64,
        instruction_sender: &InstructionSender,
    ) -> Result<()> {
        let mut seed = [0u8; 32];
        let signature = storage_keypair.sign(&entry_id.as_ref());

        let ix = storage_instruction::advertise_recent_blockhash(
            &storage_keypair.pubkey(),
            entry_id,
            slot,
        );
        instruction_sender.send(ix)?;

        seed.copy_from_slice(&signature.to_bytes()[..32]);

        let mut rng = ChaChaRng::from_seed(seed);

        state.write().unwrap().slot = slot;

        // Regenerate the answers
        let num_segments = get_segment_from_slot(slot) as usize;
        if num_segments == 0 {
            info!("Ledger has 0 segments!");
            return Ok(());
        }
        // TODO: what if the validator does not have this segment
        let segment = signature.to_bytes()[0] as usize % num_segments;

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
            // Should be overwritten by the proof signatures which replace the
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

    fn process_storage_transaction(
        data: &[u8],
        slot: u64,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        current_key_idx: &mut usize,
        transaction_key0: Pubkey,
    ) {
        match deserialize(data) {
            Ok(StorageInstruction::SubmitMiningProof {
                slot: proof_slot,
                signature,
                ..
            }) => {
                if proof_slot < slot {
                    {
                        debug!(
                            "generating storage_keys from storage txs current_key_idx: {}",
                            *current_key_idx
                        );
                        let storage_keys = &mut storage_state.write().unwrap().storage_keys;
                        storage_keys[*current_key_idx..*current_key_idx + size_of::<Signature>()]
                            .copy_from_slice(signature.as_ref());
                        *current_key_idx += size_of::<Signature>();
                        *current_key_idx %= storage_keys.len();
                    }

                    let mut statew = storage_state.write().unwrap();
                    let max_segment_index = get_segment_from_slot(slot) as usize;
                    if statew.replicator_map.len() <= max_segment_index {
                        statew
                            .replicator_map
                            .resize(max_segment_index, HashSet::new());
                    }
                    let proof_segment_index = get_segment_from_slot(proof_slot) as usize;
                    if proof_segment_index < statew.replicator_map.len() {
                        statew.replicator_map[proof_segment_index].insert(transaction_key0);
                    }
                }
                debug!("storage proof: slot: {}", slot);
            }
            Ok(_) => {}
            Err(e) => {
                info!("error: {:?}", e);
            }
        }
    }

    fn process_entries(
        storage_keypair: &Arc<Keypair>,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        slot_receiver: &Receiver<u64>,
        blocktree: &Arc<Blocktree>,
        slot_count: &mut u64,
        current_key_idx: &mut usize,
        storage_rotate_count: u64,
        instruction_sender: &InstructionSender,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        let slot: u64 = slot_receiver.recv_timeout(timeout)?;
        storage_state.write().unwrap().slot = slot;
        *slot_count += 1;
        if let Ok(entries) = blocktree.get_slot_entries(slot, 0, None) {
            for entry in entries {
                // Go through the transactions, find proofs, and use them to update
                // the storage_keys with their signatures
                for tx in entry.transactions {
                    for (i, program_id) in tx.message.program_ids().iter().enumerate() {
                        if solana_storage_api::check_id(&program_id) {
                            Self::process_storage_transaction(
                                &tx.message().instructions[i].data,
                                slot,
                                storage_state,
                                current_key_idx,
                                tx.message.account_keys[0],
                            );
                        }
                    }
                }
                if *slot_count % storage_rotate_count == 0 {
                    debug!(
                        "crosses sending at slot: {}! hashes: {}",
                        slot, entry.num_hashes
                    );
                    Self::process_entry_crossing(
                        &storage_keypair,
                        &storage_state,
                        &blocktree,
                        entry.hash,
                        slot,
                        instruction_sender,
                    )?;
                }
            }
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
    use super::*;
    use crate::blocktree::{create_new_tmp_ledger, Blocktree};
    use crate::cluster_info::ClusterInfo;
    use crate::contact_info::ContactInfo;
    use crate::entry::{make_tiny_test_entries, Entry};
    use crate::genesis_utils::create_genesis_block;
    use crate::service::Service;
    use rayon::prelude::*;
    use solana_runtime::bank::Bank;
    use solana_sdk::hash::{Hash, Hasher};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_storage_api::SLOTS_PER_SEGMENT;
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
        let storage_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let cluster_info = test_cluster_info(&keypair.pubkey());
        let (genesis_block, _mint_keypair) = create_genesis_block(1000);
        let bank = Arc::new(Bank::new(&genesis_block));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(&[bank], 0)));
        let (_slot_sender, slot_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            slot_receiver,
            None,
            &keypair,
            &storage_keypair,
            &exit.clone(),
            &bank_forks,
            STORAGE_ROTATE_TEST_COUNT,
            &cluster_info,
        );
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();
    }

    fn test_cluster_info(id: &Pubkey) -> Arc<RwLock<ClusterInfo>> {
        let contact_info = ContactInfo::new_localhost(id, 0);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
        Arc::new(RwLock::new(cluster_info))
    }

    #[test]
    fn test_storage_stage_process_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (genesis_block, _mint_keypair) = create_genesis_block(1000);
        let ticks_per_slot = genesis_block.ticks_per_slot;
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        let entries = make_tiny_test_entries(64);
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        let slot = 1;
        let bank = Arc::new(Bank::new(&genesis_block));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(&[bank], 0)));
        blocktree
            .write_entries(slot, 0, 0, ticks_per_slot, &entries)
            .unwrap();

        let cluster_info = test_cluster_info(&keypair.pubkey());

        let (slot_sender, slot_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            slot_receiver,
            Some(blocktree.clone()),
            &keypair,
            &storage_keypair,
            &exit.clone(),
            &bank_forks,
            STORAGE_ROTATE_TEST_COUNT,
            &cluster_info,
        );
        slot_sender.send(slot).unwrap();

        let keypair = Keypair::new();
        let hash = Hash::default();
        let signature = keypair.sign_message(&hash.as_ref());
        let mut result = storage_state.get_mining_result(&signature);
        assert_eq!(result, Hash::default());

        for i in slot..slot + SLOTS_PER_SEGMENT + 1 {
            blocktree
                .write_entries(i, 0, 0, ticks_per_slot, &entries)
                .unwrap();

            slot_sender.send(i).unwrap();
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
    fn test_storage_stage_process_proof_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let (genesis_block, _mint_keypair) = create_genesis_block(1000);
        let ticks_per_slot = genesis_block.ticks_per_slot;;
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        let entries = make_tiny_test_entries(128);
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        blocktree
            .write_entries(1, 0, 0, ticks_per_slot, &entries)
            .unwrap();
        let bank = Arc::new(Bank::new(&genesis_block));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(&[bank], 0)));
        let cluster_info = test_cluster_info(&keypair.pubkey());

        let (slot_sender, slot_receiver) = channel();
        let storage_state = StorageState::new();
        let storage_stage = StorageStage::new(
            &storage_state,
            slot_receiver,
            Some(blocktree.clone()),
            &keypair,
            &storage_keypair,
            &exit.clone(),
            &bank_forks,
            STORAGE_ROTATE_TEST_COUNT,
            &cluster_info,
        );
        slot_sender.send(1).unwrap();

        let mut reference_keys;
        {
            let keys = &storage_state.state.read().unwrap().storage_keys;
            reference_keys = vec![0; keys.len()];
            reference_keys.copy_from_slice(keys);
        }

        let keypair = Keypair::new();
        let mining_proof_ix = storage_instruction::mining_proof(
            &keypair.pubkey(),
            Hash::default(),
            0,
            keypair.sign_message(b"test"),
        );
        let mining_proof_tx = Transaction::new_unsigned_instructions(vec![mining_proof_ix]);
        let mining_txs = vec![mining_proof_tx];

        let proof_entries = vec![Entry::new(&Hash::default(), 1, mining_txs)];
        blocktree
            .write_entries(2, 0, 0, ticks_per_slot, &proof_entries)
            .unwrap();
        slot_sender.send(2).unwrap();

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
