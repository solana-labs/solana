// Service to verify accounts hashes with other known validator nodes.
//
// Each interval, publish the snapshat hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the `--known-validator`s
// set and halt the node if a mismatch is detected.

use crate::snapshot_packager_service::PendingSnapshotPackage;
use rayon::ThreadPool;
use solana_gossip::cluster_info::{ClusterInfo, MAX_SNAPSHOT_HASHES};
use solana_runtime::{
    accounts_db,
    snapshot_package::{AccountsPackage, AccountsPackagePre, AccountsPackageReceiver},
};
use solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey};
use std::collections::{HashMap, HashSet};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
        Arc,
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub struct AccountsHashVerifier {
    t_accounts_hash_verifier: JoinHandle<()>,
}

impl AccountsHashVerifier {
    pub fn new(
        accounts_package_receiver: AccountsPackageReceiver,
        pending_snapshot_package: Option<PendingSnapshotPackage>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<ClusterInfo>,
        known_validators: Option<HashSet<Pubkey>>,
        halt_on_known_validators_accounts_hash_mismatch: bool,
        fault_injection_rate_slots: u64,
        snapshot_interval_slots: u64,
    ) -> Self {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let t_accounts_hash_verifier = Builder::new()
            .name("solana-hash-accounts".to_string())
            .spawn(move || {
                let mut hashes = vec![];
                let mut thread_pool_storage = None;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    match accounts_package_receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(accounts_package) => {
                            if accounts_package.hash_for_testing.is_some()
                                && thread_pool_storage.is_none()
                            {
                                thread_pool_storage =
                                    Some(accounts_db::make_min_priority_thread_pool());
                            }

                            Self::process_accounts_package_pre(
                                accounts_package,
                                &cluster_info,
<<<<<<< HEAD
                                &trusted_validators,
                                halt_on_trusted_validators_accounts_hash_mismatch,
                                &pending_snapshot_package,
=======
                                known_validators.as_ref(),
                                halt_on_known_validators_accounts_hash_mismatch,
                                pending_snapshot_package.as_ref(),
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
                                &mut hashes,
                                &exit,
                                fault_injection_rate_slots,
                                snapshot_interval_slots,
                                thread_pool_storage.as_ref(),
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

    #[allow(clippy::too_many_arguments)]
    fn process_accounts_package_pre(
        accounts_package: AccountsPackagePre,
        cluster_info: &ClusterInfo,
<<<<<<< HEAD
        trusted_validators: &Option<HashSet<Pubkey>>,
        halt_on_trusted_validator_accounts_hash_mismatch: bool,
        pending_snapshot_package: &Option<PendingSnapshotPackage>,
=======
        known_validators: Option<&HashSet<Pubkey>>,
        halt_on_known_validator_accounts_hash_mismatch: bool,
        pending_snapshot_package: Option<&PendingSnapshotPackage>,
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
        snapshot_interval_slots: u64,
        thread_pool: Option<&ThreadPool>,
    ) {
        let accounts_package = solana_runtime::snapshot_utils::process_accounts_package_pre(
            accounts_package,
            thread_pool,
        );
        Self::process_accounts_package(
            accounts_package,
            cluster_info,
<<<<<<< HEAD
            trusted_validators,
            halt_on_trusted_validator_accounts_hash_mismatch,
            pending_snapshot_package,
=======
            known_validators,
            halt_on_known_validator_accounts_hash_mismatch,
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
            hashes,
            exit,
            fault_injection_rate_slots,
            snapshot_interval_slots,
        );
    }

    fn process_accounts_package(
        accounts_package: AccountsPackage,
        cluster_info: &ClusterInfo,
<<<<<<< HEAD
        trusted_validators: &Option<HashSet<Pubkey>>,
        halt_on_trusted_validator_accounts_hash_mismatch: bool,
        pending_snapshot_package: &Option<PendingSnapshotPackage>,
=======
        known_validators: Option<&HashSet<Pubkey>>,
        halt_on_known_validator_accounts_hash_mismatch: bool,
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
        snapshot_interval_slots: u64,
    ) {
        let hash = accounts_package.hash;
        if fault_injection_rate_slots != 0
            && accounts_package.slot % fault_injection_rate_slots == 0
        {
            // For testing, publish an invalid hash to gossip.
            use rand::{thread_rng, Rng};
            use solana_sdk::hash::extend_and_hash;
            warn!("inserting fault at slot: {}", accounts_package.slot);
            let rand = thread_rng().gen_range(0, 10);
            let hash = extend_and_hash(&hash, &[rand]);
            hashes.push((accounts_package.slot, hash));
        } else {
            hashes.push((accounts_package.slot, hash));
        }

        while hashes.len() > MAX_SNAPSHOT_HASHES {
            hashes.remove(0);
        }

        if halt_on_known_validator_accounts_hash_mismatch {
            let mut slot_to_hash = HashMap::new();
            for (slot, hash) in hashes.iter() {
                slot_to_hash.insert(*slot, *hash);
            }
            if Self::should_halt(cluster_info, known_validators, &mut slot_to_hash) {
                exit.store(true, Ordering::Relaxed);
            }
        }

        if accounts_package.block_height % snapshot_interval_slots == 0 {
            if let Some(pending_snapshot_package) = pending_snapshot_package.as_ref() {
                *pending_snapshot_package.lock().unwrap() = Some(accounts_package);
            }
        }

        cluster_info.push_accounts_hashes(hashes.clone());
    }

    fn should_halt(
        cluster_info: &ClusterInfo,
<<<<<<< HEAD
        trusted_validators: &Option<HashSet<Pubkey>>,
=======
        known_validators: Option<&HashSet<Pubkey>>,
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
        slot_to_hash: &mut HashMap<Slot, Hash>,
    ) -> bool {
        let mut verified_count = 0;
        let mut highest_slot = 0;
<<<<<<< HEAD
        if let Some(trusted_validators) = trusted_validators.as_ref() {
            for trusted_validator in trusted_validators {
                let is_conflicting = cluster_info.get_accounts_hash_for_node(trusted_validator, |accounts_hashes|
=======
        if let Some(known_validators) = known_validators {
            for known_validator in known_validators {
                let is_conflicting = cluster_info.get_accounts_hash_for_node(known_validator, |accounts_hashes|
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
                {
                    accounts_hashes.iter().any(|(slot, hash)| {
                        if let Some(reference_hash) = slot_to_hash.get(slot) {
                            if *hash != *reference_hash {
                                error!("Known validator {} produced conflicting hashes for slot: {} ({} != {})",
                                    known_validator,
                                    slot,
                                    hash,
                                    reference_hash,
                                );
                                true
                            } else {
                                verified_count += 1;
                                false
                            }
                        } else {
                            highest_slot = std::cmp::max(*slot, highest_slot);
                            slot_to_hash.insert(*slot, *hash);
                            false
                        }
                    })
                }).unwrap_or(false);

                if is_conflicting {
                    return true;
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
    use solana_gossip::{cluster_info::make_accounts_hashes_message, contact_info::ContactInfo};
    use solana_runtime::bank_forks::ArchiveFormat;
    use solana_runtime::snapshot_utils::SnapshotVersion;
    use solana_sdk::{
        hash::hash,
        signature::{Keypair, Signer},
    };
    use solana_streamer::socket::SocketAddrSpace;

    fn new_test_cluster_info(contact_info: ContactInfo) -> ClusterInfo {
        ClusterInfo::new(
            contact_info,
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        )
    }

    #[test]
    fn test_should_halt() {
        let keypair = Keypair::new();

        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let cluster_info = new_test_cluster_info(contact_info);
        let cluster_info = Arc::new(cluster_info);

        let mut known_validators = HashSet::new();
        let mut slot_to_hash = HashMap::new();
        assert!(!AccountsHashVerifier::should_halt(
            &cluster_info,
<<<<<<< HEAD
            &Some(trusted_validators.clone()),
=======
            Some(&known_validators),
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
            &mut slot_to_hash,
        ));

        let validator1 = Keypair::new();
        let hash1 = hash(&[1]);
        let hash2 = hash(&[2]);
        {
            let message = make_accounts_hashes_message(&validator1, vec![(0, hash1)]).unwrap();
            cluster_info.push_message(message);
            cluster_info.flush_push_queue();
        }
        slot_to_hash.insert(0, hash2);
        known_validators.insert(validator1.pubkey());
        assert!(AccountsHashVerifier::should_halt(
            &cluster_info,
<<<<<<< HEAD
            &Some(trusted_validators),
=======
            Some(&known_validators),
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
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
        let cluster_info = new_test_cluster_info(contact_info);
        let cluster_info = Arc::new(cluster_info);

        let known_validators = HashSet::new();
        let exit = Arc::new(AtomicBool::new(false));
        let mut hashes = vec![];
        for i in 0..MAX_SNAPSHOT_HASHES + 1 {
            let snapshot_links = TempDir::new().unwrap();
            let accounts_package = AccountsPackage {
                hash: hash(&[i as u8]),
                block_height: 100 + i as u64,
                slot: 100 + i as u64,
                slot_deltas: vec![],
                snapshot_links,
                tar_output_file: PathBuf::from("."),
                storages: vec![],
                archive_format: ArchiveFormat::TarBzip2,
                snapshot_version: SnapshotVersion::default(),
            };

            AccountsHashVerifier::process_accounts_package(
                accounts_package,
                &cluster_info,
<<<<<<< HEAD
                &Some(trusted_validators.clone()),
=======
                Some(&known_validators),
>>>>>>> b0ca33546 (Rename "trusted" to "known" in `validators/` (#21197))
                false,
                &None,
                &mut hashes,
                &exit,
                0,
                100,
            );
            // sleep for 1ms to create a newer timestmap for gossip entry
            // otherwise the timestamp won't be newer.
            std::thread::sleep(Duration::from_millis(1));
        }
        cluster_info.flush_push_queue();
        let cluster_hashes = cluster_info
            .get_accounts_hash_for_node(&keypair.pubkey(), |c| c.clone())
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
