// A stage that handles generating the keys used to encrypt the ledger and sample it
// for storage mining. Replicators submit storage proofs, validator then bundles them
// to submit its proof for mining to be rewarded.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
#[cfg(cuda)]
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
use solana_sdk::transaction::Transaction;
use solana_storage_api::storage_contract::{CheckedProof, Proof, ProofStatus};
use solana_storage_api::storage_instruction::{proof_validation, StorageInstruction};
use solana_storage_api::{get_segment_from_slot, storage_instruction};
use std::collections::HashMap;
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
type ReplicatorMap = Vec<HashMap<Pubkey, Vec<Proof>>>;

#[derive(Default)]
pub struct StorageStateInner {
    storage_results: StorageResults,
    storage_keys: StorageKeys,
    replicator_map: ReplicatorMap,
    storage_blockhash: Hash,
    slot: u64,
}

// Used to track root slots in storage stage
#[derive(Default)]
struct StorageSlots {
    last_root: u64,
    slot_count: u64,
    pending_roots: Vec<u64>,
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
                .keys()
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
        slot_receiver: Receiver<Vec<u64>>,
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
                    let mut storage_slots = StorageSlots::default();
                    loop {
                        if let Some(ref some_blocktree) = blocktree {
                            if let Err(e) = Self::process_entries(
                                &storage_keypair,
                                &storage_state_inner,
                                &slot_receiver,
                                &some_blocktree,
                                &mut storage_slots,
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

                    {
                        let working_bank = bank_forks.read().unwrap().working_bank();
                        let storage_account = working_bank.get_account(&storage_keypair.pubkey());
                        if storage_account.is_none() {
                            warn!("Storage account not found: {}", storage_keypair.pubkey());
                        }
                    }

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
        let keypair_balance = working_bank.get_balance(&keypair.pubkey());

        if keypair_balance == 0 {
            warn!("keypair account balance empty: {}", keypair.pubkey(),);
        } else {
            debug!(
                "keypair account balance: {}: {}",
                keypair.pubkey(),
                keypair_balance
            );
        }
        if working_bank
            .get_account(&storage_keypair.pubkey())
            .is_none()
        {
            warn!(
                "storage account does not exist: {}",
                storage_keypair.pubkey()
            );
        }

        let signer_keys = vec![keypair.as_ref(), storage_keypair.as_ref()];
        let message = Message::new_with_payer(vec![instruction], Some(&signer_keys[0].pubkey()));
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
        blockhash: Hash,
        slot: u64,
        instruction_sender: &InstructionSender,
    ) -> Result<()> {
        let mut seed = [0u8; 32];
        let signature = storage_keypair.sign(&blockhash.as_ref());

        let ix = storage_instruction::advertise_recent_blockhash(
            &storage_keypair.pubkey(),
            blockhash,
            slot,
        );
        instruction_sender.send(ix)?;

        seed.copy_from_slice(&signature.to_bytes()[..32]);

        let mut rng = ChaChaRng::from_seed(seed);

        {
            let mut w_state = state.write().unwrap();
            w_state.slot = slot;
            w_state.storage_blockhash = blockhash;
        }

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
        #[cfg(cuda)]
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
        Ok(())
    }

    fn process_storage_transaction(
        data: &[u8],
        slot: u64,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        current_key_idx: &mut usize,
        storage_account_key: Pubkey,
    ) {
        match deserialize(data) {
            Ok(StorageInstruction::SubmitMiningProof {
                slot: proof_slot,
                signature,
                sha_state,
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
                    if statew.replicator_map.len() < max_segment_index {
                        statew
                            .replicator_map
                            .resize(max_segment_index, HashMap::new());
                    }
                    let proof_segment_index = get_segment_from_slot(proof_slot) as usize;
                    if proof_segment_index < statew.replicator_map.len() {
                        // Copy the submitted proof
                        statew.replicator_map[proof_segment_index]
                            .entry(storage_account_key)
                            .or_default()
                            .push(Proof {
                                signature,
                                sha_state,
                            });
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
        slot_receiver: &Receiver<Vec<u64>>,
        blocktree: &Arc<Blocktree>,
        storage_slots: &mut StorageSlots,
        current_key_idx: &mut usize,
        storage_rotate_count: u64,
        instruction_sender: &InstructionSender,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        storage_slots
            .pending_roots
            .append(&mut slot_receiver.recv_timeout(timeout)?);
        storage_slots
            .pending_roots
            .sort_unstable_by(|a, b| b.cmp(a));
        // check if any rooted slots were missed leading up to this one and bump slot count and process proofs for each missed root
        while let Some(slot) = storage_slots.pending_roots.pop() {
            if slot > storage_slots.last_root {
                if !blocktree.is_full(slot) {
                    // stick this slot back into pending_roots. Evaluate it next time around.
                    storage_slots.pending_roots.push(slot);
                    break;
                }
                storage_slots.slot_count += 1;
                storage_slots.last_root = slot;

                if let Ok(entries) = blocktree.get_slot_entries(slot, 0, None) {
                    for entry in &entries {
                        // Go through the transactions, find proofs, and use them to update
                        // the storage_keys with their signatures
                        for tx in &entry.transactions {
                            for instruction in tx.message.instructions.iter() {
                                let program_id =
                                    tx.message.account_keys[instruction.program_ids_index as usize];
                                if solana_storage_api::check_id(&program_id) {
                                    let storage_account_key =
                                        tx.message.account_keys[instruction.accounts[0] as usize];
                                    Self::process_storage_transaction(
                                        &instruction.data,
                                        slot,
                                        storage_state,
                                        current_key_idx,
                                        storage_account_key,
                                    );
                                }
                            }
                        }
                    }
                    if storage_slots.slot_count % storage_rotate_count == 0 {
                        // assume the last entry in the slot is the blockhash for that slot
                        let entry_hash = entries.last().unwrap().hash;
                        debug!(
                            "crosses sending at root slot: {}! with last entry's hash {}",
                            storage_slots.slot_count, entry_hash
                        );
                        Self::process_entry_crossing(
                            &storage_keypair,
                            &storage_state,
                            &blocktree,
                            entry_hash,
                            slot,
                            instruction_sender,
                        )?;
                        Self::submit_verifications(
                            &storage_state,
                            &storage_keypair,
                            instruction_sender,
                        )?
                    }
                }
            }
        }
        Ok(())
    }

    fn submit_verifications(
        storage_state: &Arc<RwLock<StorageStateInner>>,
        storage_keypair: &Arc<Keypair>,
        ix_sender: &Sender<Instruction>,
    ) -> Result<()> {
        // bundle up mining submissions from replicators
        // and submit them in a tx to the leader to get rewarded.
        let mut w_state = storage_state.write().unwrap();
        let instructions: Vec<_> = w_state
            .replicator_map
            .iter_mut()
            .enumerate()
            .flat_map(|(segment, proof_map)| {
                let checked_proofs = proof_map
                    .iter_mut()
                    .map(|(id, proofs)| {
                        (
                            *id,
                            proofs
                                .drain(..)
                                .map(|proof| CheckedProof {
                                    proof,
                                    status: ProofStatus::Valid,
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<HashMap<_, _>>();
                if !checked_proofs.is_empty() {
                    let ix =
                        proof_validation(&storage_keypair.pubkey(), segment as u64, checked_proofs);
                    Some(ix)
                } else {
                    None
                }
            })
            .collect();
        // TODO Avoid AccountInUse errors in this loop
        let res: std::result::Result<_, _> = instructions
            .into_iter()
            .map(|ix| ix_sender.send(ix))
            .collect();
        res?;
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
    use crate::blocktree::tests::{entries_to_blobs, make_slot_entries};
    use crate::blocktree::{create_new_tmp_ledger, Blocktree};
    use crate::cluster_info::ClusterInfo;
    use crate::contact_info::ContactInfo;
    use crate::entry::Entry;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
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
        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(1000);
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

        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(1000);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        let (blobs, _entries) = make_slot_entries(1, 0, 64);
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        let slot = 1;
        let bank = Arc::new(Bank::new(&genesis_block));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(&[bank], 0)));
        blocktree.insert_data_blobs(blobs).unwrap();

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
        slot_sender.send(vec![slot]).unwrap();

        let keypair = Keypair::new();
        let hash = Hash::default();
        let signature = keypair.sign_message(&hash.as_ref());
        let mut result = storage_state.get_mining_result(&signature);
        assert_eq!(result, Hash::default());

        let rooted_slots = (slot..slot + SLOTS_PER_SEGMENT + 1)
            .map(|i| {
                let (blobs, _entries) = make_slot_entries(i, i - 1, 64);
                blocktree.insert_data_blobs(blobs).unwrap();
                i
            })
            .collect::<Vec<_>>();
        slot_sender.send(rooted_slots).unwrap();
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

        #[cfg(not(cuda))]
        assert_eq!(result, Hash::default());

        #[cfg(cuda)]
        assert_ne!(result, Hash::default());

        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_storage_stage_process_proof_entries() {
        solana_logger::setup();
        let keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(1000);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        let (blobs, entries) = make_slot_entries(1, 0, 128);
        blocktree.insert_data_blobs(blobs).unwrap();

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
        slot_sender.send(vec![1]).unwrap();

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

        let next_hash = solana_sdk::hash::hash(entries.last().unwrap().hash.as_ref());
        let proof_entry = Entry::new(&next_hash, 1, mining_txs);
        blocktree
            .insert_data_blobs(entries_to_blobs(&vec![proof_entry], 2, 1, true))
            .unwrap();
        slot_sender.send(vec![2]).unwrap();

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
