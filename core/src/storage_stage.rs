// A stage that handles generating the keys used to encrypt the ledger and sample it
// for storage mining. Archivers submit storage proofs, validator then bundles them
// to submit its proof for mining to be rewarded.

use crate::{
    cluster_info::ClusterInfo,
    commitment::BlockCommitmentCache,
    contact_info::ContactInfo,
    result::{Error, Result},
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use solana_chacha_cuda::chacha_cuda::chacha_cbc_encrypt_file_many_keys;
use solana_ledger::{bank_forks::BankForks, blockstore::Blockstore};
use solana_runtime::{bank::Bank, storage_utils::archiver_accounts};
use solana_sdk::{
    account::Account,
    account_utils::StateMut,
    clock::{get_segment_from_slot, Slot},
    hash::Hash,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use solana_storage_program::{
    storage_contract::{Proof, ProofStatus, StorageContract},
    storage_instruction,
    storage_instruction::proof_validation,
};
use solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY;
use std::{
    cmp,
    collections::HashMap,
    io,
    mem::size_of,
    net::UdpSocket,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    sync::{Arc, RwLock},
    thread::{self, sleep, Builder, JoinHandle},
    time::{Duration, Instant},
};

// Block of hash answers to validate against
// Vec of [ledger blocks] x [keys]
type StorageResults = Vec<Hash>;
type StorageKeys = Vec<u8>;
type ArchiverMap = Vec<HashMap<Pubkey, Vec<Proof>>>;

#[derive(Default)]
pub struct StorageStateInner {
    storage_results: StorageResults,
    pub storage_keys: StorageKeys,
    archiver_map: ArchiverMap,
    storage_blockhash: Hash,
    slot: Slot,
    slots_per_segment: u64,
    slots_per_turn: u64,
}

// Used to track root slots in storage stage
#[derive(Default)]
struct StorageSlots {
    last_root: u64,
    slot_count: u64,
    pending_root_banks: Vec<Arc<Bank>>,
}

#[derive(Clone, Default)]
pub struct StorageState {
    pub state: Arc<RwLock<StorageStateInner>>,
}

pub struct StorageStage {
    t_storage_mining_verifier: JoinHandle<()>,
    t_storage_create_accounts: JoinHandle<()>,
}

pub const SLOTS_PER_TURN_TEST: u64 = 2;
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
    pub fn new(hash: &Hash, slots_per_turn: u64, slots_per_segment: u64) -> Self {
        let storage_keys = vec![0u8; KEY_SIZE * NUM_IDENTITIES];
        let storage_results = vec![Hash::default(); NUM_IDENTITIES];
        let archiver_map = vec![];

        let state = StorageStateInner {
            storage_keys,
            storage_results,
            archiver_map,
            slots_per_turn,
            slot: 0,
            slots_per_segment,
            storage_blockhash: *hash,
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

    pub fn get_storage_turn_rate(&self) -> u64 {
        self.state.read().unwrap().slots_per_turn
    }

    pub fn get_slot(&self) -> u64 {
        self.state.read().unwrap().slot
    }

    pub fn get_pubkeys_for_slot(
        &self,
        slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) -> Vec<Pubkey> {
        // TODO: keep track of age?
        const MAX_PUBKEYS_TO_RETURN: usize = 5;
        let index =
            get_segment_from_slot(slot, self.state.read().unwrap().slots_per_segment) as usize;
        let archiver_map = &self.state.read().unwrap().archiver_map;
        let working_bank = bank_forks.read().unwrap().working_bank();
        let accounts = archiver_accounts(&working_bank);
        if index < archiver_map.len() {
            //perform an account owner lookup
            let mut slot_archivers = archiver_map[index]
                .keys()
                .filter_map(|account_id| {
                    accounts.get(account_id).and_then(|account| {
                        if let Ok(StorageContract::ArchiverStorage { owner, .. }) = account.state()
                        {
                            Some(owner)
                        } else {
                            None
                        }
                    })
                })
                .collect::<Vec<_>>();
            slot_archivers.truncate(MAX_PUBKEYS_TO_RETURN);
            slot_archivers
        } else {
            vec![]
        }
    }
}

impl StorageStage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage_state: &StorageState,
        bank_receiver: Receiver<Vec<Arc<Bank>>>,
        blockstore: Option<Arc<Blockstore>>,
        keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        exit: &Arc<AtomicBool>,
        bank_forks: &Arc<RwLock<BankForks>>,
        cluster_info: &Arc<ClusterInfo>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    ) -> Self {
        let (instruction_sender, instruction_receiver) = channel();

        let t_storage_mining_verifier = {
            let slots_per_turn = storage_state.state.read().unwrap().slots_per_turn;
            let storage_state_inner = storage_state.state.clone();
            let exit = exit.clone();
            let storage_keypair = storage_keypair.clone();
            Builder::new()
                .name("solana-storage-mining-verify-stage".to_string())
                .spawn(move || {
                    let mut current_key = 0;
                    let mut storage_slots = StorageSlots::default();
                    loop {
                        if let Some(ref some_blockstore) = blockstore {
                            if let Err(e) = Self::process_entries(
                                &storage_keypair,
                                &storage_state_inner,
                                &bank_receiver,
                                &some_blockstore,
                                &mut storage_slots,
                                &mut current_key,
                                slots_per_turn,
                                &instruction_sender,
                            ) {
                                match e {
                                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => {
                                        break;
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
                                    &block_commitment_cache,
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
        cluster_info: &ClusterInfo,
        instruction: Instruction,
        keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        transactions_socket: &UdpSocket,
        block_commitment_cache: &Arc<RwLock<BlockCommitmentCache>>,
    ) -> io::Result<()> {
        let working_bank = bank_forks.read().unwrap().working_bank();
        let blockhash = working_bank.confirmed_last_blockhash().0;
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
        let message = Message::new_with_payer(&[instruction], Some(&signer_keys[0].pubkey()));
        let transaction = Transaction::new(&signer_keys, message, blockhash);
        // try sending the transaction upto 5 times
        for _ in 0..5 {
            transactions_socket.send_to(
                &bincode::serialize(&transaction).unwrap(),
                cluster_info.my_contact_info().tpu,
            )?;
            sleep(Duration::from_millis(100));
            if Self::poll_for_signature_confirmation(
                bank_forks,
                block_commitment_cache,
                &transaction.signatures[0],
                0,
            )
            .is_ok()
            {
                break;
            };
        }
        Ok(())
    }

    fn poll_for_signature_confirmation(
        bank_forks: &Arc<RwLock<BankForks>>,
        block_commitment_cache: &Arc<RwLock<BlockCommitmentCache>>,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> Result<()> {
        let mut now = Instant::now();
        let mut confirmed_blocks = 0;
        loop {
            let working_bank = bank_forks.read().unwrap().working_bank();
            let response = working_bank.get_signature_status_slot(signature);
            if let Some((slot, status)) = response {
                let confirmations = if working_bank.src.roots().contains(&slot) {
                    MAX_LOCKOUT_HISTORY + 1
                } else {
                    let r_block_commitment_cache = block_commitment_cache.read().unwrap();
                    r_block_commitment_cache
                        .get_confirmation_count(slot)
                        .unwrap_or(0)
                };
                if status.is_ok() {
                    if confirmed_blocks != confirmations {
                        now = Instant::now();
                        confirmed_blocks = confirmations;
                    }
                    if confirmations >= min_confirmed_blocks {
                        break;
                    }
                }
            };
            if now.elapsed().as_secs() > 5 {
                return Err(Error::from(io::Error::new(
                    io::ErrorKind::Other,
                    "signature not found",
                )));
            }
            sleep(Duration::from_millis(250));
        }
        Ok(())
    }

    fn process_turn(
        storage_keypair: &Arc<Keypair>,
        state: &Arc<RwLock<StorageStateInner>>,
        blockstore: &Arc<Blockstore>,
        blockhash: Hash,
        slot: Slot,
        slots_per_segment: u64,
        instruction_sender: &InstructionSender,
        total_proofs: usize,
    ) -> Result<()> {
        let mut seed = [0u8; 32];
        let signature = storage_keypair.sign_message(&blockhash.as_ref());

        let ix = storage_instruction::advertise_recent_blockhash(
            &storage_keypair.pubkey(),
            blockhash,
            get_segment_from_slot(slot, slots_per_segment),
        );
        instruction_sender.send(ix)?;

        seed.copy_from_slice(&signature.as_ref()[..32]);

        let mut rng = ChaChaRng::from_seed(seed);

        {
            let mut w_state = state.write().unwrap();
            w_state.slot = slot;
            w_state.storage_blockhash = blockhash;
        }

        if total_proofs == 0 {
            return Ok(());
        }

        // Regenerate the answers
        let num_segments = get_segment_from_slot(slot, slots_per_segment) as usize;
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
        if solana_perf::perf_libs::api().is_some() {
            // Lock the keys, since this is the IV memory,
            // it will be updated in-place by the encryption.
            // Should be overwritten by the proof signatures which replace the
            // key values by the time it runs again.

            let mut statew = state.write().unwrap();

            match chacha_cbc_encrypt_file_many_keys(
                blockstore,
                segment as u64,
                statew.slots_per_segment,
                &mut statew.storage_keys,
                &samples,
            ) {
                Ok(hashes) => {
                    debug!("Success! encrypted ledger segment: {}", segment);
                    statew.storage_results.copy_from_slice(&hashes);
                }
                Err(e) => {
                    info!("error encrypting file: {:?}", e);
                    return Err(e.into());
                }
            }
        }
        Ok(())
    }

    fn collect_proofs(
        slot: Slot,
        slots_per_segment: u64,
        account_id: Pubkey,
        account: Account,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        current_key_idx: &mut usize,
    ) -> usize {
        let mut proofs_collected = 0;
        if let Ok(StorageContract::ArchiverStorage { proofs, .. }) = account.state() {
            //convert slot to segment
            let segment = get_segment_from_slot(slot, slots_per_segment);
            if let Some(proofs) = proofs.get(&segment) {
                for proof in proofs.iter() {
                    {
                        // TODO do this only once per account and segment? and maybe do it somewhere else
                        debug!(
                            "generating storage_keys from storage txs current_key_idx: {}",
                            *current_key_idx
                        );
                        let storage_keys = &mut storage_state.write().unwrap().storage_keys;
                        storage_keys[*current_key_idx..*current_key_idx + size_of::<Signature>()]
                            .copy_from_slice(proof.signature.as_ref());
                        *current_key_idx += size_of::<Signature>();
                        *current_key_idx %= storage_keys.len();
                    }

                    let mut statew = storage_state.write().unwrap();
                    if statew.archiver_map.len() < segment as usize {
                        statew.archiver_map.resize(segment as usize, HashMap::new());
                    }
                    let proof_segment_index = proof.segment_index as usize;
                    if proof_segment_index < statew.archiver_map.len() {
                        // TODO randomly select and verify the proof first
                        // Copy the submitted proof
                        statew.archiver_map[proof_segment_index]
                            .entry(account_id)
                            .or_default()
                            .push(proof.clone());
                        proofs_collected += 1;
                    }
                }
                debug!("storage proof: slot: {}", slot);
            }
        }
        proofs_collected
    }

    fn process_entries(
        storage_keypair: &Arc<Keypair>,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        bank_receiver: &Receiver<Vec<Arc<Bank>>>,
        blockstore: &Arc<Blockstore>,
        storage_slots: &mut StorageSlots,
        current_key_idx: &mut usize,
        slots_per_turn: u64,
        instruction_sender: &InstructionSender,
    ) -> Result<()> {
        let timeout = Duration::new(1, 0);
        storage_slots
            .pending_root_banks
            .append(&mut bank_receiver.recv_timeout(timeout)?);
        storage_slots
            .pending_root_banks
            .sort_unstable_by(|a, b| b.slot().cmp(&a.slot()));
        // check if any rooted slots were missed leading up to this one and bump slot count and process proofs for each missed root
        while let Some(bank) = storage_slots.pending_root_banks.pop() {
            if bank.slot() > storage_slots.last_root {
                storage_slots.slot_count += 1;
                storage_slots.last_root = bank.slot();
                if storage_slots.slot_count % slots_per_turn == 0 {
                    // load all the archiver accounts in the bank. collect all their proofs at the current slot
                    let archiver_accounts = archiver_accounts(bank.as_ref());
                    // find proofs, and use them to update
                    // the storage_keys with their signatures
                    let mut total_proofs = 0;
                    for (account_id, account) in archiver_accounts.into_iter() {
                        total_proofs += Self::collect_proofs(
                            bank.slot(),
                            bank.slots_per_segment(),
                            account_id,
                            account,
                            storage_state,
                            current_key_idx,
                        );
                    }

                    // TODO un-ignore this result and be sure to drain all pending proofs
                    let _ignored = Self::process_turn(
                        &storage_keypair,
                        &storage_state,
                        &blockstore,
                        bank.last_blockhash(),
                        bank.slot(),
                        bank.slots_per_segment(),
                        instruction_sender,
                        total_proofs,
                    );
                    Self::submit_verifications(
                        get_segment_from_slot(bank.slot(), bank.slots_per_segment()),
                        &storage_state,
                        &storage_keypair,
                        instruction_sender,
                    )?
                }
            }
        }
        Ok(())
    }

    fn submit_verifications(
        current_segment: u64,
        storage_state: &Arc<RwLock<StorageStateInner>>,
        storage_keypair: &Arc<Keypair>,
        ix_sender: &Sender<Instruction>,
    ) -> Result<()> {
        // bundle up mining submissions from archivers
        // and submit them in a tx to the leader to get rewarded.
        let mut w_state = storage_state.write().unwrap();
        let mut max_proof_mask = 0;
        let proof_mask_limit = storage_instruction::proof_mask_limit();
        let instructions: Vec<_> = w_state
            .archiver_map
            .iter_mut()
            .enumerate()
            .flat_map(|(_, proof_map)| {
                let checked_proofs = proof_map
                    .iter_mut()
                    .filter_map(|(id, proofs)| {
                        if !proofs.is_empty() {
                            if (proofs.len() as u64) >= proof_mask_limit {
                                proofs.clear();
                                None
                            } else {
                                max_proof_mask = cmp::max(max_proof_mask, proofs.len());
                                Some((
                                    *id,
                                    proofs
                                        .drain(..)
                                        .map(|_| ProofStatus::Valid)
                                        .collect::<Vec<_>>(),
                                ))
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<(_, _)>>();

                if !checked_proofs.is_empty() {
                    let max_accounts_per_ix =
                        storage_instruction::validation_account_limit(max_proof_mask);
                    let ixs = checked_proofs
                        .chunks(max_accounts_per_ix as usize)
                        .map(|checked_proofs| {
                            proof_validation(
                                &storage_keypair.pubkey(),
                                current_segment,
                                checked_proofs.to_vec(),
                            )
                        })
                        .collect::<Vec<_>>();
                    Some(ixs)
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        let res: std::result::Result<_, _> = instructions
            .into_iter()
            .map(|ix| {
                sleep(Duration::from_millis(100));
                ix_sender.send(ix)
            })
            .collect();
        res?;
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_storage_create_accounts.join().unwrap();
        self.t_storage_mining_verifier.join()
    }
}

pub fn test_cluster_info(id: &Pubkey) -> Arc<ClusterInfo> {
    let contact_info = ContactInfo::new_localhost(id, 0);
    let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
    Arc::new(cluster_info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;
    use solana_ledger::{
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
    };
    use solana_runtime::bank::Bank;
    use solana_sdk::{
        hash::Hasher,
        signature::{Keypair, Signer},
    };
    use std::{
        cmp::{max, min},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc::channel,
            Arc, RwLock,
        },
    };

    #[test]
    fn test_storage_stage_none_ledger() {
        let keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());
        let exit = Arc::new(AtomicBool::new(false));

        let cluster_info = test_cluster_info(&keypair.pubkey());
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new(&genesis_config));
        let bank_forks = Arc::new(RwLock::new(BankForks::new_from_banks(
            &[bank.clone()],
            vec![0],
        )));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore),
        ));
        let (_slot_sender, slot_receiver) = channel();
        let storage_state = StorageState::new(
            &bank.last_blockhash(),
            SLOTS_PER_TURN_TEST,
            bank.slots_per_segment(),
        );
        let storage_stage = StorageStage::new(
            &storage_state,
            slot_receiver,
            None,
            &keypair,
            &storage_keypair,
            &exit.clone(),
            &bank_forks,
            &cluster_info,
            block_commitment_cache,
        );
        exit.store(true, Ordering::Relaxed);
        storage_stage.join().unwrap();
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
