// Service to verify accounts hashes with other trusted validator nodes.
//
// Each interval, publish the snapshat hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the `--known-validator`s
// set and halt the node if a mismatch is detected.

use rayon::ThreadPool;
use solana_gossip::cluster_info::{ClusterInfo, MAX_SNAPSHOT_HASHES};
use solana_runtime::{
    accounts_db,
    snapshot_archive_info::SnapshotArchiveInfoGetter,
    snapshot_config::SnapshotConfig,
    snapshot_package::{
        AccountsPackage, AccountsPackageReceiver, PendingSnapshotPackage, SnapshotPackage,
    },
    snapshot_utils,
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
        trusted_validators: Option<HashSet<Pubkey>>,
        halt_on_trusted_validators_accounts_hash_mismatch: bool,
        fault_injection_rate_slots: u64,
        snapshot_config: Option<SnapshotConfig>,
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

                            Self::process_accounts_package(
                                accounts_package,
                                &cluster_info,
                                trusted_validators.as_ref(),
                                halt_on_trusted_validators_accounts_hash_mismatch,
                                pending_snapshot_package.as_ref(),
                                &mut hashes,
                                &exit,
                                fault_injection_rate_slots,
                                snapshot_config.as_ref(),
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
    fn process_accounts_package(
        accounts_package: AccountsPackage,
        cluster_info: &ClusterInfo,
        trusted_validators: Option<&HashSet<Pubkey>>,
        halt_on_trusted_validator_accounts_hash_mismatch: bool,
        pending_snapshot_package: Option<&PendingSnapshotPackage>,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
        snapshot_config: Option<&SnapshotConfig>,
        thread_pool: Option<&ThreadPool>,
    ) {
        let snapshot_package =
            snapshot_utils::process_accounts_package(accounts_package, thread_pool, None);
        Self::process_snapshot_package(
            snapshot_package,
            cluster_info,
            trusted_validators,
            halt_on_trusted_validator_accounts_hash_mismatch,
            pending_snapshot_package,
            hashes,
            exit,
            fault_injection_rate_slots,
            snapshot_config,
        );
    }

    fn process_snapshot_package(
        snapshot_package: SnapshotPackage,
        cluster_info: &ClusterInfo,
        trusted_validators: Option<&HashSet<Pubkey>>,
        halt_on_trusted_validator_accounts_hash_mismatch: bool,
        pending_snapshot_package: Option<&PendingSnapshotPackage>,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
        snapshot_config: Option<&SnapshotConfig>,
    ) {
        let hash = *snapshot_package.hash();
        if fault_injection_rate_slots != 0
            && snapshot_package.slot() % fault_injection_rate_slots == 0
        {
            // For testing, publish an invalid hash to gossip.
            use rand::{thread_rng, Rng};
            use solana_sdk::hash::extend_and_hash;
            warn!("inserting fault at slot: {}", snapshot_package.slot());
            let rand = thread_rng().gen_range(0, 10);
            let hash = extend_and_hash(&hash, &[rand]);
            hashes.push((snapshot_package.slot(), hash));
        } else {
            hashes.push((snapshot_package.slot(), hash));
        }

        while hashes.len() > MAX_SNAPSHOT_HASHES {
            hashes.remove(0);
        }

        if halt_on_trusted_validator_accounts_hash_mismatch {
            let mut slot_to_hash = HashMap::new();
            for (slot, hash) in hashes.iter() {
                slot_to_hash.insert(*slot, *hash);
            }
            if Self::should_halt(cluster_info, trusted_validators, &mut slot_to_hash) {
                exit.store(true, Ordering::Relaxed);
            }
        }

        if let Some(snapshot_config) = snapshot_config {
            if snapshot_package.block_height % snapshot_config.full_snapshot_archive_interval_slots
                == 0
            {
                if let Some(pending_snapshot_package) = pending_snapshot_package {
                    *pending_snapshot_package.lock().unwrap() = Some(snapshot_package);
                }
            }
        }

        cluster_info.push_accounts_hashes(hashes.clone());
    }

    fn should_halt(
        cluster_info: &ClusterInfo,
        trusted_validators: Option<&HashSet<Pubkey>>,
        slot_to_hash: &mut HashMap<Slot, Hash>,
    ) -> bool {
        let mut verified_count = 0;
        let mut highest_slot = 0;
        if let Some(trusted_validators) = trusted_validators {
            for trusted_validator in trusted_validators {
                let is_conflicting = cluster_info.get_accounts_hash_for_node(trusted_validator, |accounts_hashes|
                {
                    accounts_hashes.iter().any(|(slot, hash)| {
                        if let Some(reference_hash) = slot_to_hash.get(slot) {
                            if *hash != *reference_hash {
                                error!("Trusted validator {} produced conflicting hashes for slot: {} ({} != {})",
                                    trusted_validator,
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
    use solana_runtime::{
        snapshot_config::LastFullSnapshotSlot,
        snapshot_package::SnapshotType,
        snapshot_utils::{ArchiveFormat, SnapshotVersion},
    };
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

        let mut trusted_validators = HashSet::new();
        let mut slot_to_hash = HashMap::new();
        assert!(!AccountsHashVerifier::should_halt(
            &cluster_info,
            Some(&trusted_validators),
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
        trusted_validators.insert(validator1.pubkey());
        assert!(AccountsHashVerifier::should_halt(
            &cluster_info,
            Some(&trusted_validators),
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

        let trusted_validators = HashSet::new();
        let exit = Arc::new(AtomicBool::new(false));
        let mut hashes = vec![];
        let full_snapshot_archive_interval_slots = 100;
        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots,
            incremental_snapshot_archive_interval_slots: Slot::MAX,
            snapshot_archives_dir: PathBuf::default(),
            bank_snapshots_dir: PathBuf::default(),
            archive_format: ArchiveFormat::Tar,
            snapshot_version: SnapshotVersion::default(),
            maximum_snapshots_to_retain: usize::MAX,
            last_full_snapshot_slot: LastFullSnapshotSlot::default(),
        };
        for i in 0..MAX_SNAPSHOT_HASHES + 1 {
            let slot = full_snapshot_archive_interval_slots + i as u64;
            let block_height = full_snapshot_archive_interval_slots + i as u64;
            let slot_deltas = vec![];
            let snapshot_links = TempDir::new().unwrap();
            let storages = vec![];
            let snapshot_archive_path = PathBuf::from(".");
            let hash = hash(&[i as u8]);
            let archive_format = ArchiveFormat::TarBzip2;
            let snapshot_version = SnapshotVersion::default();
            let snapshot_package = SnapshotPackage::new(
                slot,
                block_height,
                slot_deltas,
                snapshot_links,
                storages,
                snapshot_archive_path,
                hash,
                archive_format,
                snapshot_version,
                SnapshotType::FullSnapshot,
            );

            AccountsHashVerifier::process_snapshot_package(
                snapshot_package,
                &cluster_info,
                Some(&trusted_validators),
                false,
                None,
                &mut hashes,
                &exit,
                0,
                Some(&snapshot_config),
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
        assert_eq!(
            cluster_hashes[0],
            (full_snapshot_archive_interval_slots + 1, hash(&[1]))
        );
        assert_eq!(
            cluster_hashes[MAX_SNAPSHOT_HASHES - 1],
            (
                full_snapshot_archive_interval_slots + MAX_SNAPSHOT_HASHES as u64,
                hash(&[MAX_SNAPSHOT_HASHES as u8])
            )
        );
    }
}
