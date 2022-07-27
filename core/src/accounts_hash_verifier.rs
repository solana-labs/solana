// Service to verify accounts hashes with other known validator nodes.
//
// Each interval, publish the snapshot hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the `--known-validator`s
// set and halt the node if a mismatch is detected.

use {
    solana_gossip::cluster_info::{ClusterInfo, MAX_SNAPSHOT_HASHES},
    solana_measure::measure::Measure,
    solana_runtime::{
        accounts_hash::{CalcAccountsHashConfig, HashStats},
        snapshot_config::SnapshotConfig,
        snapshot_package::{
            retain_max_n_elements, AccountsPackage, PendingAccountsPackage, PendingSnapshotPackage,
            SnapshotPackage, SnapshotType,
        },
        sorted_storages::SortedStorages,
    },
    solana_sdk::{
        clock::{Slot, SLOT_MS},
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        collections::{HashMap, HashSet},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct AccountsHashVerifier {
    t_accounts_hash_verifier: JoinHandle<()>,
}

impl AccountsHashVerifier {
    pub fn new(
        pending_accounts_package: PendingAccountsPackage,
        pending_snapshot_package: Option<PendingSnapshotPackage>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<ClusterInfo>,
        known_validators: Option<HashSet<Pubkey>>,
        halt_on_known_validators_accounts_hash_mismatch: bool,
        fault_injection_rate_slots: u64,
        snapshot_config: Option<SnapshotConfig>,
    ) -> Self {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let t_accounts_hash_verifier = Builder::new()
            .name("solana-hash-accounts".to_string())
            .spawn(move || {
                let mut hashes = vec![];
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let accounts_package = pending_accounts_package.lock().unwrap().take();
                    if accounts_package.is_none() {
                        std::thread::sleep(Duration::from_millis(SLOT_MS));
                        continue;
                    }
                    let accounts_package = accounts_package.unwrap();

                    Self::process_accounts_package(
                        accounts_package,
                        &cluster_info,
                        known_validators.as_ref(),
                        halt_on_known_validators_accounts_hash_mismatch,
                        pending_snapshot_package.as_ref(),
                        &mut hashes,
                        &exit,
                        fault_injection_rate_slots,
                        snapshot_config.as_ref(),
                    );
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
        known_validators: Option<&HashSet<Pubkey>>,
        halt_on_known_validator_accounts_hash_mismatch: bool,
        pending_snapshot_package: Option<&PendingSnapshotPackage>,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
        snapshot_config: Option<&SnapshotConfig>,
    ) {
        let accounts_hash = Self::calculate_and_verify_accounts_hash(&accounts_package);

        Self::push_accounts_hashes_to_cluster(
            &accounts_package,
            cluster_info,
            known_validators,
            halt_on_known_validator_accounts_hash_mismatch,
            hashes,
            exit,
            fault_injection_rate_slots,
            accounts_hash,
        );

        Self::submit_for_packaging(
            accounts_package,
            pending_snapshot_package,
            snapshot_config,
            accounts_hash,
        );
    }

    /// returns calculated accounts hash
    fn calculate_and_verify_accounts_hash(accounts_package: &AccountsPackage) -> Hash {
        let mut measure_hash = Measure::start("hash");
        let mut sort_time = Measure::start("sort_storages");
        let sorted_storages = SortedStorages::new(&accounts_package.snapshot_storages);
        sort_time.stop();

        let mut timings = HashStats {
            storage_sort_us: sort_time.as_us(),
            ..HashStats::default()
        };
        timings.calc_storage_size_quartiles(&accounts_package.snapshot_storages);

        let (accounts_hash, lamports) = accounts_package
            .accounts
            .accounts_db
            .calculate_accounts_hash_without_index(
                &CalcAccountsHashConfig {
                    use_bg_thread_pool: true,
                    check_hash: false,
                    ancestors: None,
                    use_write_cache: false,
                    epoch_schedule: &accounts_package.epoch_schedule,
                    rent_collector: &accounts_package.rent_collector,
                    store_detailed_debug_info_on_failure: false,
                },
                &sorted_storages,
                timings,
            )
            .unwrap();

        if accounts_package.expected_capitalization != lamports {
            // before we assert, run the hash calc again. This helps track down whether it could have been a failure in a race condition possibly with shrink.
            // We could add diagnostics to the hash calc here to produce a per bin cap or something to help narrow down how many pubkeys are different.
            let _ = accounts_package
                .accounts
                .accounts_db
                .calculate_accounts_hash_without_index(
                    &CalcAccountsHashConfig {
                        use_bg_thread_pool: false,
                        check_hash: false,
                        ancestors: None,
                        use_write_cache: false,
                        epoch_schedule: &accounts_package.epoch_schedule,
                        rent_collector: &accounts_package.rent_collector,
                        // now that we've failed, store off the failing contents that produced a bad capitalization
                        store_detailed_debug_info_on_failure: true,
                    },
                    &sorted_storages,
                    HashStats::default(),
                );
        }

        assert_eq!(accounts_package.expected_capitalization, lamports);
        if let Some(expected_hash) = accounts_package.accounts_hash_for_testing {
            assert_eq!(expected_hash, accounts_hash);
        };

        accounts_package
            .accounts
            .accounts_db
            .notify_accounts_hash_calculated_complete(
                sorted_storages.max_slot_inclusive(),
                &accounts_package.epoch_schedule,
            );

        measure_hash.stop();
        solana_runtime::serde_snapshot::reserialize_bank_with_new_accounts_hash(
            accounts_package.snapshot_links.path(),
            accounts_package.slot,
            &accounts_hash,
        );
        datapoint_info!(
            "accounts_hash_verifier",
            ("calculate_hash", measure_hash.as_us(), i64),
        );
        accounts_hash
    }

    fn generate_fault_hash(original_hash: &Hash) -> Hash {
        use {
            rand::{thread_rng, Rng},
            solana_sdk::hash::extend_and_hash,
        };

        let rand = thread_rng().gen_range(0, 10);
        extend_and_hash(original_hash, &[rand])
    }

    fn push_accounts_hashes_to_cluster(
        accounts_package: &AccountsPackage,
        cluster_info: &ClusterInfo,
        known_validators: Option<&HashSet<Pubkey>>,
        halt_on_known_validator_accounts_hash_mismatch: bool,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &Arc<AtomicBool>,
        fault_injection_rate_slots: u64,
        accounts_hash: Hash,
    ) {
        if fault_injection_rate_slots != 0
            && accounts_package.slot % fault_injection_rate_slots == 0
        {
            // For testing, publish an invalid hash to gossip.
            let fault_hash = Self::generate_fault_hash(&accounts_hash);
            warn!("inserting fault at slot: {}", accounts_package.slot);
            hashes.push((accounts_package.slot, fault_hash));
        } else {
            hashes.push((accounts_package.slot, accounts_hash));
        }

        retain_max_n_elements(hashes, MAX_SNAPSHOT_HASHES);

        if halt_on_known_validator_accounts_hash_mismatch {
            let mut slot_to_hash = HashMap::new();
            for (slot, hash) in hashes.iter() {
                slot_to_hash.insert(*slot, *hash);
            }
            if Self::should_halt(cluster_info, known_validators, &mut slot_to_hash) {
                exit.store(true, Ordering::Relaxed);
            }
        }

        cluster_info.push_accounts_hashes(hashes.clone());
    }

    fn submit_for_packaging(
        accounts_package: AccountsPackage,
        pending_snapshot_package: Option<&PendingSnapshotPackage>,
        snapshot_config: Option<&SnapshotConfig>,
        accounts_hash: Hash,
    ) {
        if accounts_package.snapshot_type.is_none()
            || pending_snapshot_package.is_none()
            || snapshot_config.is_none()
        {
            return;
        };

        let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash);
        let pending_snapshot_package = pending_snapshot_package.unwrap();

        // If the snapshot package is an Incremental Snapshot, do not submit it if there's already
        // a pending Full Snapshot.
        let can_submit = match snapshot_package.snapshot_type {
            SnapshotType::FullSnapshot => true,
            SnapshotType::IncrementalSnapshot(_) => pending_snapshot_package
                .lock()
                .unwrap()
                .as_ref()
                .map_or(true, |snapshot_package| {
                    snapshot_package.snapshot_type.is_incremental_snapshot()
                }),
        };

        if can_submit {
            *pending_snapshot_package.lock().unwrap() = Some(snapshot_package);
        }
    }

    fn should_halt(
        cluster_info: &ClusterInfo,
        known_validators: Option<&HashSet<Pubkey>>,
        slot_to_hash: &mut HashMap<Slot, Hash>,
    ) -> bool {
        let mut verified_count = 0;
        let mut highest_slot = 0;
        if let Some(known_validators) = known_validators {
            for known_validator in known_validators {
                let is_conflicting = cluster_info.get_accounts_hash_for_node(known_validator, |accounts_hashes|
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
    use {
        super::*,
        solana_gossip::{cluster_info::make_accounts_hashes_message, contact_info::ContactInfo},
        solana_runtime::{
            rent_collector::RentCollector,
            snapshot_utils::{ArchiveFormat, SnapshotVersion},
        },
        solana_sdk::{
            genesis_config::ClusterType,
            hash::hash,
            signature::{Keypair, Signer},
            sysvar::epoch_schedule::EpochSchedule,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::str::FromStr,
    };

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
            Some(&known_validators),
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
            Some(&known_validators),
            &mut slot_to_hash,
        ));
    }

    #[test]
    fn test_max_hashes() {
        solana_logger::setup();
        use {std::path::PathBuf, tempfile::TempDir};
        let keypair = Keypair::new();

        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let cluster_info = new_test_cluster_info(contact_info);
        let cluster_info = Arc::new(cluster_info);

        let known_validators = HashSet::new();
        let exit = Arc::new(AtomicBool::new(false));
        let mut hashes = vec![];
        let full_snapshot_archive_interval_slots = 100;
        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots,
            incremental_snapshot_archive_interval_slots: Slot::MAX,
            ..SnapshotConfig::default()
        };
        let accounts = Arc::new(solana_runtime::accounts::Accounts::default_for_tests());
        let expected_hash = Hash::from_str("GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn").unwrap();
        for i in 0..MAX_SNAPSHOT_HASHES + 1 {
            let accounts_package = AccountsPackage {
                slot: full_snapshot_archive_interval_slots + i as u64,
                block_height: full_snapshot_archive_interval_slots + i as u64,
                slot_deltas: vec![],
                snapshot_links: TempDir::new().unwrap(),
                snapshot_storages: vec![],
                archive_format: ArchiveFormat::TarBzip2,
                snapshot_version: SnapshotVersion::default(),
                full_snapshot_archives_dir: PathBuf::default(),
                incremental_snapshot_archives_dir: PathBuf::default(),
                expected_capitalization: 0,
                accounts_hash_for_testing: None,
                cluster_type: ClusterType::MainnetBeta,
                snapshot_type: None,
                accounts: Arc::clone(&accounts),
                epoch_schedule: EpochSchedule::default(),
                rent_collector: RentCollector::default(),
            };

            AccountsHashVerifier::process_accounts_package(
                accounts_package,
                &cluster_info,
                Some(&known_validators),
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
            (full_snapshot_archive_interval_slots + 1, expected_hash)
        );
        assert_eq!(
            cluster_hashes[MAX_SNAPSHOT_HASHES - 1],
            (
                full_snapshot_archive_interval_slots + MAX_SNAPSHOT_HASHES as u64,
                expected_hash
            )
        );
    }
}
