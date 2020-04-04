// Service to verify accounts hashes with other trusted validator nodes.
//
// Each interval, publish the snapshat hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the --trusted-validators
// set and halt the node if a mismatch is detected.

use crate::cluster_info::{ClusterInfo, MAX_SNAPSHOT_HASHES};
use solana_ledger::staking_utils::vote_account_stakes;
use solana_ledger::{
    bank_forks::BankForks, snapshot_package::AccountsPackage,
    snapshot_package::AccountsPackageReceiver, snapshot_package::AccountsPackageSender,
};
use solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey};
use std::collections::{HashMap, HashSet};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
        Arc, RwLock,
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

#[derive(Debug, Default)]
struct StakeInfo {
    hash: Hash,
    disagree_stake: u64,
    total_stake: u64,
}

pub struct AccountsHashVerifier {
    t_accounts_hash_verifier: JoinHandle<()>,
}

#[derive(Debug, Default, Clone)]
pub struct HashVerifierConfig {
    pub halt_on_trusted_validators_accounts_hash_mismatch: bool,
    pub trusted_validators: Option<HashSet<Pubkey>>,
    pub accounts_hash_fault_injection_slots: u64,
    pub snapshot_interval_slots: u64,
    pub halt_on_supermajority_hash_mismatch: bool,
}

impl AccountsHashVerifier {
    pub fn new(
        accounts_package_receiver: AccountsPackageReceiver,
        accounts_package_sender: Option<AccountsPackageSender>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        bank_forks: &Arc<RwLock<BankForks>>,
        config: HashVerifierConfig,
    ) -> Self {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let bank_forks = bank_forks.clone();
        let t_accounts_hash_verifier = Builder::new()
            .name("solana-accounts-hash".to_string())
            .spawn(move || {
                let mut hashes = vec![];
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    match accounts_package_receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(accounts_package) => {
                            Self::process_accounts_package(
                                accounts_package,
                                &cluster_info,
                                &accounts_package_sender,
                                &mut hashes,
                                &exit,
                                &bank_forks,
                                &config,
                            );
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => (),
                    }
                }
            })
            .unwrap();
        Self {
            t_accounts_hash_verifier,
        }
    }

    fn process_accounts_package(
        accounts_package: AccountsPackage,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        accounts_package_sender: &Option<AccountsPackageSender>,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        bank_forks: &Arc<RwLock<BankForks>>,
        config: &HashVerifierConfig,
    ) {
        if config.accounts_hash_fault_injection_slots != 0
            && accounts_package.root % config.accounts_hash_fault_injection_slots == 0
        {
            // For testing, publish an invalid hash to gossip.
            use rand::{thread_rng, Rng};
            use solana_sdk::hash::extend_and_hash;
            warn!("inserting fault at slot: {}", accounts_package.root);
            let rand = thread_rng().gen_range(0, 10);
            let hash = extend_and_hash(&accounts_package.hash, &[rand]);
            hashes.push((accounts_package.root, hash));
        } else {
            hashes.push((accounts_package.root, accounts_package.hash));
        }

        while hashes.len() > MAX_SNAPSHOT_HASHES {
            hashes.remove(0);
        }

        if config.halt_on_trusted_validators_accounts_hash_mismatch {
            let mut slot_to_hash = HashMap::new();
            for (slot, hash) in hashes.iter() {
                slot_to_hash.insert(*slot, *hash);
            }
            if Self::should_halt(&cluster_info, &config.trusted_validators, &mut slot_to_hash) {
                exit.store(true, Ordering::Relaxed);
            }
        }

        if config.halt_on_supermajority_hash_mismatch {
            let bank_forks_r = bank_forks.read().unwrap();
            let root_bank = bank_forks_r.root_bank().clone();
            drop(bank_forks_r);
            let vote_account_stakes = vote_account_stakes(&root_bank);

            if Self::should_halt_for_supermajority(cluster_info, vote_account_stakes, hashes) {
                exit.store(true, Ordering::Relaxed);
            }
        }

        if accounts_package.block_height % config.snapshot_interval_slots == 0 {
            if let Some(sender) = accounts_package_sender.as_ref() {
                if sender.send(accounts_package).is_err() {}
            }
        }

        cluster_info
            .write()
            .unwrap()
            .push_accounts_hashes(hashes.clone());
    }

    fn should_halt_for_supermajority(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        vote_account_stakes: HashMap<Pubkey, u64>,
        hashes: &[(Slot, Hash)],
    ) -> bool {
        let mut slot_to_hash = HashMap::new();
        for (slot, hash) in hashes.iter() {
            slot_to_hash.insert(
                *slot,
                StakeInfo {
                    hash: *hash,
                    disagree_stake: 0,
                    total_stake: 0,
                },
            );
        }

        let cluster_info_r = cluster_info.read().unwrap();
        for (id, stake) in &vote_account_stakes {
            if let Some(hashes) = cluster_info_r.get_accounts_hash_for_node(id) {
                for (slot, hash) in hashes {
                    if let Some(entry) = slot_to_hash.get_mut(slot) {
                        entry.total_stake += stake;
                        if entry.hash != *hash {
                            entry.disagree_stake += stake;
                        }
                    }
                }
            }
        }
        for (slot, entry) in &slot_to_hash {
            if entry.disagree_stake as f64 / entry.total_stake as f64 > 0.66666 {
                info!(
                    "Supermajority of the stake disagrees with my hash value! slot: {} hash: {}",
                    slot, entry.hash
                );
                return true;
            }
        }
        false
    }

    fn should_halt(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        trusted_validators: &Option<HashSet<Pubkey>>,
        slot_to_hash: &mut HashMap<Slot, Hash>,
    ) -> bool {
        let mut verified_count = 0;
        let mut highest_slot = 0;
        if let Some(trusted_validators) = trusted_validators.as_ref() {
            for trusted_validator in trusted_validators {
                let cluster_info_r = cluster_info.read().unwrap();
                if let Some(accounts_hashes) =
                    cluster_info_r.get_accounts_hash_for_node(trusted_validator)
                {
                    for (slot, hash) in accounts_hashes {
                        if let Some(reference_hash) = slot_to_hash.get(slot) {
                            if *hash != *reference_hash {
                                error!("Trusted validator {} produced conflicting hashes for slot: {} ({} != {})",
                                    trusted_validator,
                                    slot,
                                    hash,
                                    reference_hash,
                                );

                                return true;
                            } else {
                                verified_count += 1;
                            }
                        } else {
                            highest_slot = std::cmp::max(*slot, highest_slot);
                            slot_to_hash.insert(*slot, *hash);
                        }
                    }
                }
            }
        }
        inc_new_counter_info!("accounts_hash_verifier-hashes_verified", verified_count);
        datapoint_info!(
            "accounts_hash_verifier",
            ("highest_slot_verified", highest_slot, i64),
        );
        false
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_accounts_hash_verifier.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_info::make_accounts_hashes_message;
    use crate::contact_info::ContactInfo;
    use solana_ledger::bank_forks::CompressionType;
    use solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use solana_runtime::bank::Bank;
    use solana_sdk::{
        hash::hash,
        signature::{Keypair, Signer},
    };

    fn insert_validator_hash(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        keys: &mut HashMap<Pubkey, u64>,
        stake: u64,
        hashes: Vec<(u64, Hash)>,
    ) {
        let validator = Keypair::new();
        keys.insert(validator.pubkey(), stake);
        let message = make_accounts_hashes_message(&validator, hashes).unwrap();
        let mut cluster_info_w = cluster_info.write().unwrap();
        cluster_info_w.push_message(message);
    }

    #[test]
    fn test_should_halt_supermajority() {
        solana_logger::setup();
        let keypair = Keypair::new();

        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let mut keys = HashMap::new();
        let hashes = vec![];
        assert!(!AccountsHashVerifier::should_halt_for_supermajority(
            &cluster_info,
            keys.clone(),
            &hashes,
        ));

        let hash1 = hash(&[1]);
        let hash2 = hash(&[2]);
        for _ in 0..3 {
            insert_validator_hash(&cluster_info, &mut keys, 10, vec![(1, hash1), (2, hash2)]);
        }
        let hashes = vec![(1, hash1)];
        assert!(!AccountsHashVerifier::should_halt_for_supermajority(
            &cluster_info,
            keys.clone(),
            &hashes,
        ));

        let hashes = vec![(1, hash2)];
        assert!(AccountsHashVerifier::should_halt_for_supermajority(
            &cluster_info,
            keys.clone(),
            &hashes,
        ));

        insert_validator_hash(&cluster_info, &mut keys, 100, vec![(1, hash2)]);

        assert!(!AccountsHashVerifier::should_halt_for_supermajority(
            &cluster_info,
            keys.clone(),
            &hashes,
        ));
    }

    #[test]
    fn test_should_halt() {
        let keypair = Keypair::new();

        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let mut trusted_validators = HashSet::new();
        let mut slot_to_hash = HashMap::new();
        assert!(!AccountsHashVerifier::should_halt(
            &cluster_info,
            &Some(trusted_validators.clone()),
            &mut slot_to_hash,
        ));

        let validator1 = Keypair::new();
        let hash1 = hash(&[1]);
        let hash2 = hash(&[2]);
        {
            let message = make_accounts_hashes_message(&validator1, vec![(0, hash1)]).unwrap();
            let mut cluster_info_w = cluster_info.write().unwrap();
            cluster_info_w.push_message(message);
        }
        slot_to_hash.insert(0, hash2);
        trusted_validators.insert(validator1.pubkey());
        assert!(AccountsHashVerifier::should_halt(
            &cluster_info,
            &Some(trusted_validators.clone()),
            &mut slot_to_hash,
        ));
    }

    #[test]
    fn test_max_hashes() {
        solana_logger::setup();
        use std::path::PathBuf;
        use tempfile::TempDir;
        let keypair = Keypair::new();

        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let cluster_info = ClusterInfo::new_with_invalid_keypair(contact_info);
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));

        let exit = Arc::new(AtomicBool::new(false));
        let mut hashes = vec![];
        for i in 0..MAX_SNAPSHOT_HASHES + 1 {
            let snapshot_links = TempDir::new().unwrap();
            let accounts_package = AccountsPackage {
                hash: hash(&[i as u8]),
                block_height: 100 + i as u64,
                root: 100 + i as u64,
                slot_deltas: vec![],
                snapshot_links,
                tar_output_file: PathBuf::from("."),
                storages: vec![],
                compression: CompressionType::Bzip2,
            };

            let config = HashVerifierConfig {
                halt_on_trusted_validators_accounts_hash_mismatch: false,
                accounts_hash_fault_injection_slots: 0,
                snapshot_interval_slots: 100,
                halt_on_supermajority_hash_mismatch: false,
                trusted_validators: None,
            };
            AccountsHashVerifier::process_accounts_package(
                accounts_package,
                &cluster_info,
                &None,
                &mut hashes,
                &exit,
                &bank_forks,
                &config,
            );
        }
        let cluster_info_r = cluster_info.read().unwrap();
        let cluster_hashes = cluster_info_r
            .get_accounts_hash_for_node(&keypair.pubkey())
            .unwrap();
        info!("{:?}", cluster_hashes);
        assert_eq!(hashes.len(), MAX_SNAPSHOT_HASHES);
        assert_eq!(cluster_hashes.len(), MAX_SNAPSHOT_HASHES);
        assert_eq!(cluster_hashes[0], (101, hash(&[1])));
        assert_eq!(
            cluster_hashes[MAX_SNAPSHOT_HASHES - 1],
            (
                100 + MAX_SNAPSHOT_HASHES as u64,
                hash(&[MAX_SNAPSHOT_HASHES as u8])
            )
        );
    }
}
