// Service to verify accounts hashes with other known validator nodes.
//
// Each interval, publish the snapshot hash which is the full accounts state
// hash on gossip. Monitor gossip for messages from validators in the `--known-validator`s
// set and halt the node if a mismatch is detected.

use {
    crossbeam_channel::{Receiver, Sender},
    solana_gossip::cluster_info::{ClusterInfo, MAX_ACCOUNTS_HASHES},
    solana_measure::measure_us,
    solana_runtime::{
        accounts_db::CalcAccountsHashFlavor,
        accounts_hash::{
            AccountsHash, AccountsHashEnum, CalcAccountsHashConfig, HashStats,
            IncrementalAccountsHash,
        },
        serde_snapshot::BankIncrementalSnapshotPersistence,
        snapshot_config::SnapshotConfig,
        snapshot_package::{
            self, retain_max_n_elements, AccountsPackage, AccountsPackageType, SnapshotPackage,
            SnapshotType,
        },
        snapshot_utils,
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

pub type AccountsHashFaultInjector = fn(&Hash, Slot) -> Option<Hash>;

pub struct AccountsHashVerifier {
    t_accounts_hash_verifier: JoinHandle<()>,
}

impl AccountsHashVerifier {
    pub fn new(
        accounts_package_sender: Sender<AccountsPackage>,
        accounts_package_receiver: Receiver<AccountsPackage>,
        snapshot_package_sender: Option<Sender<SnapshotPackage>>,
        exit: Arc<AtomicBool>,
        cluster_info: Arc<ClusterInfo>,
        known_validators: Option<HashSet<Pubkey>>,
        halt_on_known_validators_accounts_hash_mismatch: bool,
        accounts_hash_fault_injector: Option<AccountsHashFaultInjector>,
        snapshot_config: SnapshotConfig,
    ) -> Self {
        // If there are no accounts packages to process, limit how often we re-check
        const LOOP_LIMITER: Duration = Duration::from_millis(DEFAULT_MS_PER_SLOT);
        let t_accounts_hash_verifier = Builder::new()
            .name("solAcctHashVer".to_string())
            .spawn(move || {
                info!("AccountsHashVerifier has started");
                let mut hashes = vec![];
                // To support fastboot, we must ensure the storages used in the latest POST snapshot are
                // not recycled nor removed early.  Hold an Arc of their AppendVecs to prevent them from
                // expiring.
                let mut last_snapshot_storages = None;
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let Some((
                        accounts_package,
                        num_outstanding_accounts_packages,
                        num_re_enqueued_accounts_packages,
                    )) = Self::get_next_accounts_package(
                        &accounts_package_sender,
                        &accounts_package_receiver,
                    ) else {
                        std::thread::sleep(LOOP_LIMITER);
                        continue;
                    };
                    info!("handling accounts package: {accounts_package:?}");
                    let enqueued_time = accounts_package.enqueued.elapsed();

                    let snapshot_storages = accounts_package.snapshot_storages.clone();

                    let (_, handling_time_us) = measure_us!(Self::process_accounts_package(
                        accounts_package,
                        &cluster_info,
                        known_validators.as_ref(),
                        halt_on_known_validators_accounts_hash_mismatch,
                        snapshot_package_sender.as_ref(),
                        &mut hashes,
                        &exit,
                        &snapshot_config,
                        accounts_hash_fault_injector,
                    ));

                    // Done processing the current snapshot, so the current snapshot dir
                    // has been converted to POST state.  It is the time to update
                    // last_snapshot_storages to release the reference counts for the
                    // previous POST snapshot dir, and save the new ones for the new
                    // POST snapshot dir.
                    last_snapshot_storages = Some(snapshot_storages);
                    debug!(
                        "Number of snapshot storages kept alive for fastboot: {}",
                        last_snapshot_storages
                            .as_ref()
                            .map(|storages| storages.len())
                            .unwrap_or(0)
                    );

                    datapoint_info!(
                        "accounts_hash_verifier",
                        (
                            "num_outstanding_accounts_packages",
                            num_outstanding_accounts_packages,
                            i64
                        ),
                        (
                            "num_re_enqueued_accounts_packages",
                            num_re_enqueued_accounts_packages,
                            i64
                        ),
                        ("enqueued_time_us", enqueued_time.as_micros(), i64),
                        ("handling_time_us", handling_time_us, i64),
                    );
                }
                debug!(
                    "Number of snapshot storages kept alive for fastboot: {}",
                    last_snapshot_storages
                        .as_ref()
                        .map(|storages| storages.len())
                        .unwrap_or(0)
                );
                info!("AccountsHashVerifier has stopped");
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
        snapshot_package_sender: Option<&Sender<SnapshotPackage>>,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &AtomicBool,
        snapshot_config: &SnapshotConfig,
        accounts_hash_fault_injector: Option<AccountsHashFaultInjector>,
    ) {
        let accounts_hash =
            Self::calculate_and_verify_accounts_hash(&accounts_package, snapshot_config);

        Self::save_epoch_accounts_hash(&accounts_package, accounts_hash);

        Self::push_accounts_hashes_to_cluster(
            &accounts_package,
            cluster_info,
            known_validators,
            halt_on_known_validator_accounts_hash_mismatch,
            hashes,
            exit,
            accounts_hash,
            accounts_hash_fault_injector,
        );

        Self::submit_for_packaging(
            accounts_package,
            snapshot_package_sender,
            snapshot_config,
            accounts_hash,
        );
    }

    /// returns calculated accounts hash
    fn calculate_and_verify_accounts_hash(
        accounts_package: &AccountsPackage,
        snapshot_config: &SnapshotConfig,
    ) -> AccountsHashEnum {
        let accounts_hash_calculation_flavor = match accounts_package.package_type {
            AccountsPackageType::AccountsHashVerifier => CalcAccountsHashFlavor::Full,
            AccountsPackageType::EpochAccountsHash => CalcAccountsHashFlavor::Full,
            AccountsPackageType::Snapshot(snapshot_type) => match snapshot_type {
                SnapshotType::FullSnapshot => CalcAccountsHashFlavor::Full,
                SnapshotType::IncrementalSnapshot(_) => {
                    if accounts_package.is_incremental_accounts_hash_feature_enabled {
                        CalcAccountsHashFlavor::Incremental
                    } else {
                        CalcAccountsHashFlavor::Full
                    }
                }
            },
        };

        let (
            accounts_hash_enum,
            accounts_hash_for_reserialize,
            bank_incremental_snapshot_persistence,
        ) = match accounts_hash_calculation_flavor {
            CalcAccountsHashFlavor::Full => {
                let (accounts_hash, _capitalization) =
                    Self::_calculate_full_accounts_hash(accounts_package);
                (accounts_hash.into(), accounts_hash, None)
            }
            CalcAccountsHashFlavor::Incremental => {
                let AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(base_slot)) = accounts_package.package_type else {
                    panic!("Calculating incremental accounts hash requires a base slot");
                };
                let (base_accounts_hash, base_capitalization) = accounts_package
                    .accounts
                    .accounts_db
                    .get_accounts_hash(base_slot)
                    .expect("incremental snapshot requires accounts hash and capitalization from the full snapshot it is based on");
                let (incremental_accounts_hash, incremental_capitalization) =
                    Self::_calculate_incremental_accounts_hash(accounts_package, base_slot);
                let bank_incremental_snapshot_persistence = BankIncrementalSnapshotPersistence {
                    full_slot: base_slot,
                    full_hash: base_accounts_hash.into(),
                    full_capitalization: base_capitalization,
                    incremental_hash: incremental_accounts_hash.into(),
                    incremental_capitalization,
                };
                (
                    incremental_accounts_hash.into(),
                    AccountsHash(Hash::default()), // value does not matter; not used for incremental snapshots
                    Some(bank_incremental_snapshot_persistence),
                )
            }
        };

        if let Some(snapshot_info) = &accounts_package.snapshot_info {
            solana_runtime::serde_snapshot::reserialize_bank_with_new_accounts_hash(
                &snapshot_info.bank_snapshot_dir,
                accounts_package.slot,
                &accounts_hash_for_reserialize,
                bank_incremental_snapshot_persistence.as_ref(),
            );
        }

        if accounts_package.package_type
            == AccountsPackageType::Snapshot(SnapshotType::FullSnapshot)
        {
            accounts_package
                .accounts
                .accounts_db
                .purge_old_accounts_hashes(accounts_package.slot);
        }

        // After an accounts package has had its accounts hash calculated and
        // has been reserialized to become a BankSnapshotPost, it is now safe
        // to clean up some older bank snapshots.
        //
        // If we are generating snapshots, then this accounts package will be sent
        // to SnapshotPackagerService to be archived.  SPS needs the bank snapshots
        // to make the archives, so we cannot purge any bank snapshots that SPS
        // may still be using. Therefore, we defer purging to SPS.
        //
        // If we are *not* generating snapshots, then purge old bank snapshots here.
        if !snapshot_config.should_generate_snapshots() {
            snapshot_utils::purge_bank_snapshots_older_than_slot(
                &snapshot_config.bank_snapshots_dir,
                accounts_package.slot,
            );
        }

        accounts_hash_enum
    }

    fn _calculate_full_accounts_hash(
        accounts_package: &AccountsPackage,
    ) -> (AccountsHash, /*capitalization*/ u64) {
        let (sorted_storages, storage_sort_us) =
            measure_us!(SortedStorages::new(&accounts_package.snapshot_storages));

        let mut timings = HashStats {
            storage_sort_us,
            ..HashStats::default()
        };
        timings.calc_storage_size_quartiles(&accounts_package.snapshot_storages);

        let calculate_accounts_hash_config = CalcAccountsHashConfig {
            use_bg_thread_pool: true,
            check_hash: false,
            ancestors: None,
            epoch_schedule: &accounts_package.epoch_schedule,
            rent_collector: &accounts_package.rent_collector,
            store_detailed_debug_info_on_failure: false,
            include_slot_in_hash: accounts_package.include_slot_in_hash,
        };

        let ((accounts_hash, lamports), measure_hash_us) = measure_us!(accounts_package
            .accounts
            .accounts_db
            .calculate_accounts_hash_from_storages(
                &calculate_accounts_hash_config,
                &sorted_storages,
                timings,
            )
            .unwrap()); // unwrap here will never fail since check_hash = false

        let slot = accounts_package.slot;
        let old_accounts_hash = accounts_package
            .accounts
            .accounts_db
            .set_accounts_hash(slot, (accounts_hash, lamports));
        if let Some(old_accounts_hash) = old_accounts_hash {
            warn!("Accounts hash was already set for slot {slot}! old: {old_accounts_hash:?}, new: {accounts_hash:?}");
        }

        if accounts_package.expected_capitalization != lamports {
            // before we assert, run the hash calc again. This helps track down whether it could have been a failure in a race condition possibly with shrink.
            // We could add diagnostics to the hash calc here to produce a per bin cap or something to help narrow down how many pubkeys are different.
            let calculate_accounts_hash_config = CalcAccountsHashConfig {
                // since we're going to assert, use the fg thread pool to go faster
                use_bg_thread_pool: false,
                ..calculate_accounts_hash_config
            };
            let result_with_index = accounts_package
                .accounts
                .accounts_db
                .calculate_accounts_hash_from_index(slot, &calculate_accounts_hash_config);
            info!("hash calc with index: {slot}, {result_with_index:?}",);
            let calculate_accounts_hash_config = CalcAccountsHashConfig {
                // now that we've failed, store off the failing contents that produced a bad capitalization
                store_detailed_debug_info_on_failure: true,
                ..calculate_accounts_hash_config
            };
            _ = accounts_package
                .accounts
                .accounts_db
                .calculate_accounts_hash_from_storages(
                    &calculate_accounts_hash_config,
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

        datapoint_info!(
            "accounts_hash_verifier",
            ("calculate_hash", measure_hash_us, i64),
        );

        (accounts_hash, lamports)
    }

    fn _calculate_incremental_accounts_hash(
        accounts_package: &AccountsPackage,
        base_slot: Slot,
    ) -> (IncrementalAccountsHash, /*capitalization*/ u64) {
        let incremental_storages =
            accounts_package
                .snapshot_storages
                .iter()
                .filter_map(|storage| {
                    let storage_slot = storage.slot();
                    (storage_slot > base_slot).then_some((storage, storage_slot))
                });
        let sorted_storages = SortedStorages::new_with_slots(incremental_storages, None, None);

        let calculate_accounts_hash_config = CalcAccountsHashConfig {
            use_bg_thread_pool: true,
            check_hash: false,
            ancestors: None,
            epoch_schedule: &accounts_package.epoch_schedule,
            rent_collector: &accounts_package.rent_collector,
            store_detailed_debug_info_on_failure: false,
            include_slot_in_hash: accounts_package.include_slot_in_hash,
        };

        let (incremental_accounts_hash, measure_hash_us) = measure_us!(
            accounts_package
                .accounts
                .accounts_db
                .update_incremental_accounts_hash(
                    &calculate_accounts_hash_config,
                    &sorted_storages,
                    accounts_package.slot,
                    HashStats::default(),
                )
                .unwrap() // unwrap here will never fail since check_hash = false
        );

        datapoint_info!(
            "accounts_hash_verifier",
            (
                "calculate_incremental_accounts_hash_us",
                measure_hash_us,
                i64
            ),
        );

        incremental_accounts_hash
    }

    fn save_epoch_accounts_hash(
        accounts_package: &AccountsPackage,
        accounts_hash: AccountsHashEnum,
    ) {
        if accounts_package.package_type == AccountsPackageType::EpochAccountsHash {
            let AccountsHashEnum::Full(accounts_hash) = accounts_hash else {
                panic!("EAH requires a full accounts hash!");
            };
            info!(
                "saving epoch accounts hash, slot: {}, hash: {}",
                accounts_package.slot, accounts_hash.0,
            );
            accounts_package
                .accounts
                .accounts_db
                .epoch_accounts_hash_manager
                .set_valid(accounts_hash.into(), accounts_package.slot);
        }
    }

    fn push_accounts_hashes_to_cluster(
        accounts_package: &AccountsPackage,
        cluster_info: &ClusterInfo,
        known_validators: Option<&HashSet<Pubkey>>,
        halt_on_known_validator_accounts_hash_mismatch: bool,
        hashes: &mut Vec<(Slot, Hash)>,
        exit: &AtomicBool,
        accounts_hash: AccountsHashEnum,
        accounts_hash_fault_injector: Option<AccountsHashFaultInjector>,
    ) {
        let hash = accounts_hash_fault_injector
            .and_then(|f| f(accounts_hash.as_hash(), accounts_package.slot))
            .or(Some(*accounts_hash.as_hash()));
        hashes.push((accounts_package.slot, hash.unwrap()));

        retain_max_n_elements(hashes, MAX_ACCOUNTS_HASHES);

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
        snapshot_package_sender: Option<&Sender<SnapshotPackage>>,
        snapshot_config: &SnapshotConfig,
        accounts_hash: AccountsHashEnum,
    ) {
        if !snapshot_config.should_generate_snapshots()
            || !matches!(
                accounts_package.package_type,
                AccountsPackageType::Snapshot(_)
            )
        {
            return;
        }
        let Some(snapshot_package_sender) = snapshot_package_sender else {
            return;
        };

        let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash);
        snapshot_package_sender
            .send(snapshot_package)
            .expect("send snapshot package");
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
        datapoint_info!(
            "accounts_hash_verifier",
            ("highest_slot_verified", highest_slot, i64),
            ("num_verified", verified_count, i64),
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
        solana_runtime::snapshot_package::SnapshotType,
        solana_sdk::{
            hash::hash,
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::str::FromStr,
    };

    fn new_test_cluster_info() -> ClusterInfo {
        let keypair = Arc::new(Keypair::new());
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), timestamp());
        ClusterInfo::new(contact_info, keypair, SocketAddrSpace::Unspecified)
    }

    #[test]
    fn test_should_halt() {
        let cluster_info = new_test_cluster_info();
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
        let cluster_info = new_test_cluster_info();
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
        for i in 0..MAX_ACCOUNTS_HASHES + 1 {
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
                &snapshot_config,
                None,
            );

            // sleep for 1ms to create a newer timestamp for gossip entry
            // otherwise the timestamp won't be newer.
            std::thread::sleep(Duration::from_millis(1));
        }
        cluster_info.flush_push_queue();
        let cluster_hashes = cluster_info
            .get_accounts_hash_for_node(&cluster_info.id(), |c| c.clone())
            .unwrap();
        info!("{:?}", cluster_hashes);
        assert_eq!(hashes.len(), MAX_ACCOUNTS_HASHES);
        assert_eq!(cluster_hashes.len(), MAX_ACCOUNTS_HASHES);
        assert_eq!(
            cluster_hashes[0],
            (full_snapshot_archive_interval_slots + 1, expected_hash)
        );
        assert_eq!(
            cluster_hashes[MAX_ACCOUNTS_HASHES - 1],
            (
                full_snapshot_archive_interval_slots + MAX_ACCOUNTS_HASHES as u64,
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
