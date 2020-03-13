// Service to verify accounts hashes with other trusted validator nodes.
//
// Each interval, publish the snapshat hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the --trusted-validators
// set and halt the node if a mismatch is detected.

use crate::cluster_info::ClusterInfo;
use solana_ledger::{
    snapshot_package::SnapshotPackage, snapshot_package::SnapshotPackageReceiver,
    snapshot_package::SnapshotPackageSender,
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

pub struct AccountsHashVerifier {
    t_accounts_hash_verifier: JoinHandle<()>,
}

impl AccountsHashVerifier {
    pub fn new(
        snapshot_package_receiver: SnapshotPackageReceiver,
        snapshot_package_sender: Option<SnapshotPackageSender>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        trusted_validators: Option<HashSet<Pubkey>>,
        halt_on_trusted_validators_accounts_hash_mismatch: bool,
        fault_injection_rate_slots: u64,
    ) -> Self {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let t_accounts_hash_verifier = Builder::new()
            .name("solana-accounts-hash".to_string())
            .spawn(move || {
                let mut hashes = vec![];
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    match snapshot_package_receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(snapshot_package) => {
                            Self::process_snapshot(
                                snapshot_package,
                                &cluster_info,
                                &trusted_validators,
                                halt_on_trusted_validators_accounts_hash_mismatch,
                                &snapshot_package_sender,
                                &mut hashes,
                                &exit,
                                fault_injection_rate_slots,
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

    fn process_snapshot(
        snapshot_package: SnapshotPackage,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        trusted_validators: &Option<HashSet<Pubkey>>,
        halt_on_trusted_validator_accounts_hash_mismatch: bool,
        snapshot_package_sender: &Option<SnapshotPackageSender>,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
    ) {
        if fault_injection_rate_slots != 0
            && snapshot_package.root % fault_injection_rate_slots == 0
        {
            // For testing, publish an invalid hash to gossip.
            use rand::{thread_rng, Rng};
            use solana_sdk::hash::extend_and_hash;
            warn!("inserting fault at slot: {}", snapshot_package.root);
            let rand = thread_rng().gen_range(0, 10);
            let hash = extend_and_hash(&snapshot_package.hash, &[rand]);
            hashes.push((snapshot_package.root, hash));
        } else {
            hashes.push((snapshot_package.root, snapshot_package.hash));
        }

        if halt_on_trusted_validator_accounts_hash_mismatch {
            let mut slot_to_hash = HashMap::new();
            for (slot, hash) in hashes.iter() {
                slot_to_hash.insert(*slot, *hash);
            }
            if Self::should_halt(&cluster_info, trusted_validators, &mut slot_to_hash) {
                exit.store(true, Ordering::Relaxed);
            }
        }
        if let Some(sender) = snapshot_package_sender.as_ref() {
            if sender.send(snapshot_package).is_err() {}
        }

        cluster_info
            .write()
            .unwrap()
            .push_accounts_hashes(hashes.clone());
    }

    fn should_halt(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        trusted_validators: &Option<HashSet<Pubkey>>,
        slot_to_hash: &mut HashMap<Slot, Hash>,
    ) -> bool {
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
                            }
                        } else {
                            slot_to_hash.insert(*slot, *hash);
                        }
                    }
                }
            }
        }
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
    use solana_sdk::{
        hash::hash,
        signature::{Keypair, Signer},
    };

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
}
