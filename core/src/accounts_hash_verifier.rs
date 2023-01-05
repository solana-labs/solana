// Service to verify accounts hashes with other known validator nodes.
//
// Each interval, publish the snapshot hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the `--known-validator`s
// set and halt the node if a mismatch is detected.

use {
    crossbeam_channel::{Receiver, Sender},
    solana_gossip::cluster_info::{ClusterInfo, MAX_SNAPSHOT_HASHES},
    solana_measure::{measure, measure::Measure},
    solana_runtime::{
        accounts_hash::{AccountsHash, CalcAccountsHashConfig, HashStats},
        epoch_accounts_hash::EpochAccountsHash,
        snapshot_config::SnapshotConfig,
        snapshot_package::{
            self, retain_max_n_elements, AccountsPackage, AccountsPackageType,
            PendingSnapshotPackage, SnapshotPackage, SnapshotType,
        },
        sorted_storages::SortedStorages,
    },
    solana_sdk::{
        clock::{Slot, DEFAULT_MS_PER_SLOT},
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
        accounts_package_sender: Sender<AccountsPackage>,
        accounts_package_receiver: Receiver<AccountsPackage>,
        pending_snapshot_package: Option<PendingSnapshotPackage>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<ClusterInfo>,
        known_validators: Option<HashSet<Pubkey>>,
        halt_on_known_validators_accounts_hash_mismatch: bool,
        fault_injection_rate_slots: u64,
        snapshot_config: SnapshotConfig,
    ) -> Self {
        // If there are no accounts packages to process, limit how often we re-check
        const LOOP_LIMITER: Duration = Duration::from_millis(DEFAULT_MS_PER_SLOT);
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let t_accounts_hash_verifier = Builder::new()
            .name("solAcctHashVer".to_string())
            .spawn(move || {
                let mut hashes = vec![];
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Some((
                        accounts_package,
                        num_outstanding_accounts_packages,
                        num_re_enqueued_accounts_packages,
                    )) = Self::get_next_accounts_package(
                        &accounts_package_sender,
                        &accounts_package_receiver,
                    ) {
                        info!("handling accounts package: {accounts_package:?}");
                        let enqueued_time = accounts_package.enqueued.elapsed();

                        let (_, measure) = measure!(Self::process_accounts_package(
                            accounts_package,
                            &cluster_info,
                            known_validators.as_ref(),
                            halt_on_known_validators_accounts_hash_mismatch,
                            pending_snapshot_package.as_ref(),
                            &mut hashes,
                            &exit,
                            fault_injection_rate_slots,
                            &snapshot_config,
                        ));

                        datapoint_info!(
                            "accounts_hash_verifier",
                            (
                                "num-outstanding-accounts-packages",
                                num_outstanding_accounts_packages as i64,
                                i64
                            ),
                            (
                                "num-re-enqueued-accounts-packages",
                                num_re_enqueued_accounts_packages as i64,
                                i64
                            ),
                            ("enqueued-time-us", enqueued_time.as_micros() as i64, i64),
                            ("total-processing-time-us", measure.as_us() as i64, i64),
                        );
                    } else {
                        std::thread::sleep(LOOP_LIMITER);
                    }
                }
            })
            .unwrap();
        Self {
            t_accounts_hash_verifier,
        }
    }

    /// Get the next accounts package to handle
    ///
    /// Look through the accounts package channel to find the highest priority one to handle next.
    /// If there are no accounts packages in the channel, return None.  Otherwise return the
    /// highest priority one.  Unhandled accounts packages with slots GREATER-THAN the handled one
    /// will be re-enqueued.  The remaining will be dropped.
    ///
    /// Also return the number of accounts packages initially in the channel, and the number of
    /// ones re-enqueued.
    fn get_next_accounts_package(
        accounts_package_sender: &Sender<AccountsPackage>,
        accounts_package_receiver: &Receiver<AccountsPackage>,
    ) -> Option<(
        AccountsPackage,
        /*num outstanding accounts packages*/ usize,
        /*num re-enqueued accounts packages*/ usize,
    )> {
        let mut accounts_packages: Vec<_> = accounts_package_receiver.try_iter().collect();
        // `select_nth()` panics if the slice is empty, so continue if that's the case
        if accounts_packages.is_empty() {
            return None;
        }
        let accounts_packages_len = accounts_packages.len();
        debug!("outstanding accounts packages ({accounts_packages_len}): {accounts_packages:?}");
        let num_eah_packages = accounts_packages
            .iter()
            .filter(|account_package| {
                account_package.package_type == AccountsPackageType::EpochAccountsHash
            })
            .count();
        assert!(
            num_eah_packages <= 1,
            "Only a single EAH accounts package is allowed at a time! count: {num_eah_packages}"
        );

        accounts_packages.select_nth_unstable_by(
            accounts_packages_len - 1,
            snapshot_package::cmp_accounts_packages_by_priority,
        );
        // SAFETY: We know `accounts_packages` is not empty, so its len is >= 1,
        // therefore there is always an element to pop.
        let accounts_package = accounts_packages.pop().unwrap();
        let handled_accounts_package_slot = accounts_package.slot;
        // re-enqueue any remaining accounts packages for slots GREATER-THAN the accounts package
        // that will be handled
        let num_re_enqueued_accounts_packages = accounts_packages
            .into_iter()
            .filter(|accounts_package| accounts_package.slot > handled_accounts_package_slot)
            .map(|accounts_package| {
                accounts_package_sender
                    .try_send(accounts_package)
                    .expect("re-enqueue accounts package")
            })
            .count();

        Some((
            accounts_package,
            accounts_packages_len,
            num_re_enqueued_accounts_packages,
        ))
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
        snapshot_config: &SnapshotConfig,
    ) {
        let accounts_hash = Self::calculate_and_verify_accounts_hash(&accounts_package);

        Self::save_epoch_accounts_hash(&accounts_package, accounts_hash);

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
    fn calculate_and_verify_accounts_hash(accounts_package: &AccountsPackage) -> AccountsHash {
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
            .calculate_accounts_hash_from_storages(
                &CalcAccountsHashConfig {
                    use_bg_thread_pool: true,
                    check_hash: false,
                    ancestors: None,
                    epoch_schedule: &accounts_package.epoch_schedule,
                    rent_collector: &accounts_package.rent_collector,
                    store_detailed_debug_info_on_failure: false,
                    full_snapshot: None,
                },
                &sorted_storages,
                timings,
            )
            .unwrap();

        if accounts_package.expected_capitalization != lamports {
            // before we assert, run the hash calc again. This helps track down whether it could have been a failure in a race condition possibly with shrink.
            // We could add diagnostics to the hash calc here to produce a per bin cap or something to help narrow down how many pubkeys are different.
            let result_with_index = accounts_package
                .accounts
                .accounts_db
                .calculate_accounts_hash_from_index(
                    accounts_package.slot,
                    &CalcAccountsHashConfig {
                        use_bg_thread_pool: false,
                        check_hash: false,
                        ancestors: None,
                        epoch_schedule: &accounts_package.epoch_schedule,
                        rent_collector: &accounts_package.rent_collector,
                        store_detailed_debug_info_on_failure: false,
                        full_snapshot: None,
                    },
                );
            info!(
                "hash calc with index: {}, {:?}",
                accounts_package.slot, result_with_index
            );
            let _ = accounts_package
                .accounts
                .accounts_db
                .calculate_accounts_hash_from_storages(
                    &CalcAccountsHashConfig {
                        use_bg_thread_pool: false,
                        check_hash: false,
                        ancestors: None,
                        epoch_schedule: &accounts_package.epoch_schedule,
                        rent_collector: &accounts_package.rent_collector,
                        // now that we've failed, store off the failing contents that produced a bad capitalization
                        store_detailed_debug_info_on_failure: true,
                        full_snapshot: None,
                    },
                    &sorted_storages,
                    HashStats::default(),
                );
        }

        assert_eq!(
            accounts_package.expected_capitalization, lamports,
            "accounts hash capitalization mismatch"
        );
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
        if let Some(snapshot_info) = &accounts_package.snapshot_info {
            solana_runtime::serde_snapshot::reserialize_bank_with_new_accounts_hash(
                snapshot_info.snapshot_links.path(),
                accounts_package.slot,
                &accounts_hash,
                None,
            );
        }
        datapoint_info!(
            "accounts_hash_verifier",
            ("calculate_hash", measure_hash.as_us(), i64),
        );
        accounts_hash
    }

    fn save_epoch_accounts_hash(accounts_package: &AccountsPackage, accounts_hash: AccountsHash) {
        if accounts_package.package_type == AccountsPackageType::EpochAccountsHash {
            info!(
                "saving epoch accounts hash, slot: {}, hash: {}",
                accounts_package.slot, accounts_hash.0,
            );
            let epoch_accounts_hash = EpochAccountsHash::from(accounts_hash);
            accounts_package
                .accounts
                .accounts_db
                .epoch_accounts_hash_manager
                .set_valid(epoch_accounts_hash, accounts_package.slot);
        }
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
        accounts_hash: AccountsHash,
    ) {
        if fault_injection_rate_slots != 0
            && accounts_package.slot % fault_injection_rate_slots == 0
        {
            // For testing, publish an invalid hash to gossip.
            let fault_hash = Self::generate_fault_hash(&accounts_hash.0);
            warn!("inserting fault at slot: {}", accounts_package.slot);
            hashes.push((accounts_package.slot, fault_hash));
        } else {
            hashes.push((accounts_package.slot, accounts_hash.0));
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
        snapshot_config: &SnapshotConfig,
        accounts_hash: AccountsHash,
    ) {
        if pending_snapshot_package.is_none()
            || !snapshot_config.should_generate_snapshots()
            || !matches!(
                accounts_package.package_type,
                AccountsPackageType::Snapshot(_)
            )
        {
            return;
        }

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
                                error!("Fatal! Exiting! Known validator {} produced conflicting hashes for slot: {} ({} != {})",
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
        rand::seq::SliceRandom,
        solana_gossip::{cluster_info::make_accounts_hashes_message, contact_info::ContactInfo},
        solana_sdk::{
            hash::hash,
            signature::{Keypair, Signer},
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
        let expected_hash = Hash::from_str("GKot5hBsd81kMupNCXHaqbhv3huEbxAFMLnpcX2hniwn").unwrap();
        for i in 0..MAX_SNAPSHOT_HASHES + 1 {
            let slot = full_snapshot_archive_interval_slots + i as u64;
            let accounts_package = AccountsPackage {
                slot,
                block_height: slot,
                ..AccountsPackage::default_for_tests()
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
                &snapshot_config,
            );

            // sleep for 1ms to create a newer timestamp for gossip entry
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

    /// Ensure that unhandled accounts packages are properly re-enqueued or dropped
    ///
    /// The accounts package handler should re-enqueue unhandled accounts packages, if those
    /// unhandled accounts packages are for slots GREATER-THAN the last handled accounts package.
    /// Otherwise, they should be dropped.
    #[test]
    fn test_get_next_accounts_package() {
        fn new(package_type: AccountsPackageType, slot: Slot) -> AccountsPackage {
            AccountsPackage {
                package_type,
                slot,
                block_height: slot,
                ..AccountsPackage::default_for_tests()
            }
        }
        fn new_eah(slot: Slot) -> AccountsPackage {
            new(AccountsPackageType::EpochAccountsHash, slot)
        }
        fn new_fss(slot: Slot) -> AccountsPackage {
            new(
                AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
                slot,
            )
        }
        fn new_iss(slot: Slot, base: Slot) -> AccountsPackage {
            new(
                AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(base)),
                slot,
            )
        }
        fn new_ahv(slot: Slot) -> AccountsPackage {
            new(AccountsPackageType::AccountsHashVerifier, slot)
        }

        let (accounts_package_sender, accounts_package_receiver) = crossbeam_channel::unbounded();

        // Populate the channel so that re-enqueueing and dropping will be tested
        let mut accounts_packages = [
            new_ahv(99),
            new_fss(100),
            new_ahv(101),
            new_iss(110, 100),
            new_ahv(111),
            new_eah(200), // <-- handle 1st
            new_ahv(201),
            new_iss(210, 100),
            new_ahv(211),
            new_fss(300),
            new_ahv(301),
            new_iss(310, 300),
            new_ahv(311),
            new_fss(400), // <-- handle 2nd
            new_ahv(401),
            new_iss(410, 400),
            new_ahv(411),
            new_iss(420, 400), // <-- handle 3rd
            new_ahv(421),
            new_ahv(422),
            new_ahv(423), // <-- handle 4th
        ];
        // Shuffle the accounts packages to simulate receiving new accounts packages from ABS
        // simultaneously as AHV is processing them.
        accounts_packages.shuffle(&mut rand::thread_rng());
        accounts_packages
            .into_iter()
            .for_each(|accounts_package| accounts_package_sender.send(accounts_package).unwrap());

        // The EAH is handled 1st
        let (
            account_package,
            _num_outstanding_accounts_packages,
            num_re_enqueued_accounts_packages,
        ) = AccountsHashVerifier::get_next_accounts_package(
            &accounts_package_sender,
            &accounts_package_receiver,
        )
        .unwrap();
        assert_eq!(
            account_package.package_type,
            AccountsPackageType::EpochAccountsHash
        );
        assert_eq!(account_package.slot, 200);
        assert_eq!(num_re_enqueued_accounts_packages, 15);

        // The Full Snapshot from slot 400 is handled 2nd
        // (the older full snapshot from slot 300 is skipped and dropped)
        let (
            account_package,
            _num_outstanding_accounts_packages,
            num_re_enqueued_accounts_packages,
        ) = AccountsHashVerifier::get_next_accounts_package(
            &accounts_package_sender,
            &accounts_package_receiver,
        )
        .unwrap();
        assert_eq!(
            account_package.package_type,
            AccountsPackageType::Snapshot(SnapshotType::FullSnapshot)
        );
        assert_eq!(account_package.slot, 400);
        assert_eq!(num_re_enqueued_accounts_packages, 7);

        // The Incremental Snapshot from slot 420 is handled 3rd
        // (the older incremental snapshot from slot 410 is skipped and dropped)
        let (
            account_package,
            _num_outstanding_accounts_packages,
            num_re_enqueued_accounts_packages,
        ) = AccountsHashVerifier::get_next_accounts_package(
            &accounts_package_sender,
            &accounts_package_receiver,
        )
        .unwrap();
        assert_eq!(
            account_package.package_type,
            AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(400))
        );
        assert_eq!(account_package.slot, 420);
        assert_eq!(num_re_enqueued_accounts_packages, 3);

        // The Accounts Have Verifier from slot 423 is handled 4th
        // (the older accounts have verifiers from slot 421 and 422 are skipped and dropped)
        let (
            account_package,
            _num_outstanding_accounts_packages,
            num_re_enqueued_accounts_packages,
        ) = AccountsHashVerifier::get_next_accounts_package(
            &accounts_package_sender,
            &accounts_package_receiver,
        )
        .unwrap();
        assert_eq!(
            account_package.package_type,
            AccountsPackageType::AccountsHashVerifier
        );
        assert_eq!(account_package.slot, 423);
        assert_eq!(num_re_enqueued_accounts_packages, 0);

        // And now the accounts package channel is empty!
        assert!(AccountsHashVerifier::get_next_accounts_package(
            &accounts_package_sender,
            &accounts_package_receiver
        )
        .is_none());
    }
}
