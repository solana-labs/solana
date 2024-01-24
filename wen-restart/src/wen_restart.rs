//! The `wen-restart` module handles automatic repair during a cluster restart

use {
    crate::{
        last_voted_fork_slots_aggregate::LastVotedForkSlotsAggregate,
        solana::wen_restart_proto::{
            LastVotedForkSlotsAggregateRecord, LastVotedForkSlotsRecord, State as RestartState,
            WenRestartProgress,
        },
    },
    log::*,
    prost::Message,
    solana_gossip::{
        cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS},
        restart_crds_values::RestartLastVotedForkSlots,
    },
    solana_ledger::{ancestor_iterator::AncestorIterator, blockstore::Blockstore},
    solana_program::{clock::Slot, hash::Hash},
    solana_runtime::bank_forks::BankForks,
    solana_sdk::timing::timestamp,
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        collections::{HashMap, HashSet},
        fs::{read, File},
        io::{Cursor, Error, Write},
        path::PathBuf,
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::sleep,
        time::Duration,
    },
};

// If >42% of the validators have this block, repair this block locally.
const REPAIR_THRESHOLD: f64 = 0.42;

fn send_restart_last_voted_fork_slots(
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
    cluster_info: Arc<ClusterInfo>,
    progress: &mut WenRestartProgress,
) -> Result<(), Box<dyn std::error::Error>> {
    let last_vote_fork_slots;
    let last_vote_hash;
    match &progress.my_last_voted_fork_slots {
        Some(my_last_voted_fork_slots) => {
            last_vote_fork_slots = my_last_voted_fork_slots.last_vote_fork_slots.clone();
            last_vote_hash = Hash::from_str(&my_last_voted_fork_slots.last_vote_bankhash).unwrap();
        }
        None => {
            // repair and restart option does not work without last voted slot.
            if let VoteTransaction::Vote(ref vote) = last_vote {
                if let Some(last_vote_slot) = vote.last_voted_slot() {
                    last_vote_hash = vote.hash;
                    last_vote_fork_slots =
                        AncestorIterator::new_inclusive(last_vote_slot, &blockstore)
                            .take(RestartLastVotedForkSlots::MAX_SLOTS)
                            .collect();
                } else {
                    return Err("last vote does not have last voted slot".into());
                }
            } else {
                return Err("last vote is not a vote transaction".into());
            }
        }
    }
    info!(
        "wen_restart last voted fork {} {:?}",
        last_vote_hash, last_vote_fork_slots
    );
    cluster_info.push_restart_last_voted_fork_slots(&last_vote_fork_slots, last_vote_hash)?;
    progress.my_last_voted_fork_slots = Some(LastVotedForkSlotsRecord {
        last_vote_fork_slots,
        last_vote_bankhash: last_vote.hash().to_string(),
        shred_version: cluster_info.my_shred_version() as u32,
    });
    Ok(())
}

fn aggregate_restart_last_voted_fork_slots(
    wen_restart_path: &PathBuf,
    wait_for_supermajority_threshold_percent: u64,
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
    wen_restart_repair_slots: Arc<RwLock<Vec<Slot>>>,
    exit: Arc<AtomicBool>,
    progress: &mut WenRestartProgress,
) -> Result<(), Box<dyn std::error::Error>> {
    let root_bank;
    {
        root_bank = bank_forks.read().unwrap().root_bank().clone();
    }
    let root_slot = root_bank.slot();
    let mut last_voted_fork_slots_aggregate = LastVotedForkSlotsAggregate::new(
        root_slot,
        REPAIR_THRESHOLD,
        root_bank.epoch_stakes(root_bank.epoch()).unwrap(),
        &progress
            .my_last_voted_fork_slots
            .as_ref()
            .unwrap()
            .last_vote_fork_slots,
        &cluster_info.id(),
    );
    let mut cursor = solana_gossip::crds::Cursor::default();
    let mut is_full_slots = HashSet::new();
    progress.last_voted_fork_slots_aggregate = Some(LastVotedForkSlotsAggregateRecord {
        received: HashMap::new(),
    });
    loop {
        if exit.load(Ordering::Relaxed) {
            return Err("Exiting".into());
        }
        let start = timestamp();
        for new_last_voted_fork_slots in cluster_info.get_restart_last_voted_fork_slots(&mut cursor)
        {
            let from = new_last_voted_fork_slots.from.to_string();
            if let Some(record) =
                last_voted_fork_slots_aggregate.aggregate(new_last_voted_fork_slots)
            {
                progress
                    .last_voted_fork_slots_aggregate
                    .as_mut()
                    .unwrap()
                    .received
                    .insert(from, record);
            }
        }
        let result = last_voted_fork_slots_aggregate.get_aggregate_result();
        let mut filtered_slots: Vec<Slot>;
        {
            let my_bank_forks = bank_forks.read().unwrap();
            filtered_slots = result
                .slots_to_repair
                .into_iter()
                .filter(|slot| {
                    if slot <= &root_slot || is_full_slots.contains(slot) {
                        return false;
                    }
                    let is_full = my_bank_forks
                        .get(*slot)
                        .map_or(false, |bank| bank.is_frozen());
                    if is_full {
                        is_full_slots.insert(*slot);
                    }
                    !is_full
                })
                .collect();
        }
        filtered_slots.sort();
        info!(
            "Active peers: {} Slots to repair: {:?}",
            result.active_percent, &filtered_slots
        );
        if filtered_slots.is_empty()
            && result.active_percent > wait_for_supermajority_threshold_percent as f64
        {
            *wen_restart_repair_slots.write().unwrap() = vec![];
            break;
        }
        {
            *wen_restart_repair_slots.write().unwrap() = filtered_slots;
        }
        write_wen_restart_records(wen_restart_path, progress)?;
        let elapsed = timestamp().saturating_sub(start);
        let time_left = GOSSIP_SLEEP_MILLIS.saturating_sub(elapsed);
        if time_left > 0 {
            sleep(Duration::from_millis(time_left));
        }
    }
    Ok(())
}

pub fn wait_for_wen_restart(
    wen_restart_path: &PathBuf,
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
    wen_restart_repair_slots: Option<Arc<RwLock<Vec<Slot>>>>,
    wait_for_supermajority_threshold_percent: u64,
    exit: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut progress = initialize(wen_restart_path)?;
    loop {
        match progress.state() {
            RestartState::Init => send_restart_last_voted_fork_slots(
                last_vote.clone(),
                blockstore.clone(),
                cluster_info.clone(),
                &mut progress,
            )?,
            RestartState::LastVotedForkSlots => aggregate_restart_last_voted_fork_slots(
                wen_restart_path,
                wait_for_supermajority_threshold_percent,
                cluster_info.clone(),
                bank_forks.clone(),
                wen_restart_repair_slots.clone().unwrap(),
                exit.clone(),
                &mut progress,
            )?,
            RestartState::HeaviestFork => {
                warn!("Not implemented yet, make it empty to complete test")
            }
            RestartState::FinishedSnapshot => return Err("Not implemented!".into()),
            RestartState::GeneratingSnapshot => return Err("Not implemented!".into()),
            RestartState::WaitingForSupermajority => return Err("Not implemented!".into()),
            RestartState::Done => return Ok(()),
        }
        increment_and_write_wen_restart_records(wen_restart_path, &mut progress)?;
    }
}

pub(crate) fn increment_and_write_wen_restart_records(
    records_path: &PathBuf,
    new_progress: &mut WenRestartProgress,
) -> Result<(), Box<dyn std::error::Error>> {
    let new_state = match new_progress.state() {
        RestartState::Init => RestartState::LastVotedForkSlots,
        RestartState::LastVotedForkSlots => RestartState::HeaviestFork,
        RestartState::HeaviestFork => RestartState::Done,
        RestartState::Done
        | RestartState::FinishedSnapshot
        | RestartState::GeneratingSnapshot
        | RestartState::WaitingForSupermajority => {
            error!("Unexpected state {:?}", new_progress.state());
            return Err("unexpected state".into());
        }
    };
    new_progress.set_state(new_state);
    write_wen_restart_records(records_path, new_progress)?;
    Ok(())
}

fn initialize(records_path: &PathBuf) -> Result<WenRestartProgress, Error> {
    match read_wen_restart_records(records_path) {
        Ok(mut progress) => {
            if progress.state() == RestartState::Done {
                progress.set_state(RestartState::Init);
                write_wen_restart_records(records_path, &progress)?;
            }
            Ok(progress)
        }
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(e);
            }
            info!(
                "wen restart proto file not found at {:?}, write init state",
                records_path
            );
            let progress = WenRestartProgress {
                state: RestartState::Init.into(),
                my_last_voted_fork_slots: None,
                last_voted_fork_slots_aggregate: None,
            };
            write_wen_restart_records(records_path, &progress)?;
            Ok(progress)
        }
    }
}

fn read_wen_restart_records(records_path: &PathBuf) -> Result<WenRestartProgress, Error> {
    let buffer = read(records_path)?;
    let progress = WenRestartProgress::decode(&mut Cursor::new(buffer))?;
    info!("read record {:?}", progress);
    Ok(progress)
}

pub(crate) fn write_wen_restart_records(
    records_path: &PathBuf,
    new_progress: &WenRestartProgress,
) -> Result<(), Error> {
    // overwrite anything if exists
    let mut file = File::create(records_path)?;
    info!("writing new record {:?}", new_progress);
    let mut buf = Vec::with_capacity(new_progress.encoded_len());
    new_progress.encode(&mut buf)?;
    file.write_all(&buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        crate::wen_restart::*,
        assert_matches::assert_matches,
        solana_entry::entry,
        solana_gossip::{
            cluster_info::ClusterInfo,
            contact_info::ContactInfo,
            crds::GossipRoute,
            crds_value::{CrdsData, CrdsValue},
            legacy_contact_info::LegacyContactInfo,
            restart_crds_values::RestartLastVotedForkSlots,
        },
        solana_ledger::{blockstore, get_tmp_ledger_path_auto_delete},
        solana_program::{
            hash::Hash,
            vote::state::{Vote, VoteStateUpdate},
        },
        solana_runtime::{
            bank::Bank,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{fs::read, sync::Arc, thread::Builder},
        tempfile::TempDir,
    };

    fn push_restart_last_voted_fork_slots(
        cluster_info: Arc<ClusterInfo>,
        node: &LegacyContactInfo,
        expected_slots_to_repair: &[Slot],
        last_vote_hash: &Hash,
        shred_version: u16,
        node_keypair: &Keypair,
    ) {
        let slots = RestartLastVotedForkSlots::new(
            *node.pubkey(),
            timestamp(),
            expected_slots_to_repair,
            *last_vote_hash,
            shred_version,
        )
        .unwrap();
        let entries = vec![
            CrdsValue::new_signed(CrdsData::LegacyContactInfo(node.clone()), node_keypair),
            CrdsValue::new_signed(CrdsData::RestartLastVotedForkSlots(slots), node_keypair),
        ];
        {
            let mut gossip_crds = cluster_info.gossip.crds.write().unwrap();
            for entry in entries {
                assert!(gossip_crds
                    .insert(entry, /*now=*/ 0, GossipRoute::LocalMessage)
                    .is_ok());
            }
        }
    }

    fn wen_restart_test_init(
        ledger_path: &TempDir,
        expected_slots: usize,
        shred_version: u16,
        last_vote_fork_slots: &mut Vec<Slot>,
    ) -> (
        Vec<ValidatorVoteKeypairs>,
        Arc<Blockstore>,
        Arc<ClusterInfo>,
        Arc<RwLock<BankForks>>,
    ) {
        let validator_voting_keypairs: Vec<_> =
            (0..10).map(|_| ValidatorVoteKeypairs::new_rand()).collect();
        let node_keypair = Arc::new(validator_voting_keypairs[0].node_keypair.insecure_clone());
        let cluster_info = Arc::new(ClusterInfo::new(
            {
                let mut contact_info =
                    ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp());
                contact_info.set_shred_version(shred_version);
                contact_info
            },
            node_keypair.clone(),
            SocketAddrSpace::Unspecified,
        ));
        let blockstore = Arc::new(blockstore::Blockstore::open(ledger_path.path()).unwrap());
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
            10_000,
            &validator_voting_keypairs,
            vec![100; validator_voting_keypairs.len()],
        );
        let (_, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        let last_parent = (RestartLastVotedForkSlots::MAX_SLOTS >> 1)
            .try_into()
            .unwrap();
        for i in 0..expected_slots {
            let entries = entry::create_ticks(1, 0, Hash::default());
            let parent_slot = if i > 0 {
                (RestartLastVotedForkSlots::MAX_SLOTS.saturating_add(i))
                    .try_into()
                    .unwrap()
            } else {
                last_parent
            };
            let slot = (RestartLastVotedForkSlots::MAX_SLOTS
                .saturating_add(i)
                .saturating_add(1)) as Slot;
            let shreds = blockstore::entries_to_test_shreds(
                &entries,
                slot,
                parent_slot,
                false,
                0,
                true, // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            last_vote_fork_slots.push(slot);
        }
        // link directly to slot 1 whose distance to last_vote > RestartLastVotedForkSlots::MAX_SLOTS so it will not be included.
        let entries = entry::create_ticks(1, 0, Hash::default());
        let shreds = blockstore::entries_to_test_shreds(
            &entries,
            last_parent,
            1,
            false,
            0,
            true, // merkle_variant
        );
        last_vote_fork_slots.extend([last_parent, 1]);
        blockstore.insert_shreds(shreds, None, false).unwrap();
        (
            validator_voting_keypairs,
            blockstore,
            cluster_info,
            bank_forks,
        )
    }

    #[test]
    fn test_wen_restart_normal_flow() {
        let shred_version = 2;
        let expected_slots = 400;
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let mut wen_restart_proto_path = ledger_path.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let mut last_vote_fork_slots = Vec::new();
        let last_vote_slot = (RestartLastVotedForkSlots::MAX_SLOTS + expected_slots)
            .try_into()
            .unwrap();
        let last_vote_bankhash = Hash::new_unique();
        let wen_restart_repair_slots = Some(Arc::new(RwLock::new(Vec::new())));
        let (validator_voting_keypairs, blockstore, cluster_info, bank_forks) =
            wen_restart_test_init(
                &ledger_path,
                expected_slots,
                shred_version,
                &mut last_vote_fork_slots,
            );
        let wen_restart_proto_path_clone = wen_restart_proto_path.clone();
        let cluster_info_clone = cluster_info.clone();
        let expected_slots_to_repair: Vec<Slot> =
            (last_vote_slot + 1..last_vote_slot + 3).collect();
        let bank_forks_clone = bank_forks.clone();
        let wen_restart_thread_handle = Builder::new()
            .name("solana-wen-restart".to_string())
            .spawn(move || {
                assert!(wait_for_wen_restart(
                    &wen_restart_proto_path_clone,
                    VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
                    blockstore,
                    cluster_info_clone,
                    bank_forks_clone,
                    wen_restart_repair_slots.clone(),
                    80,
                    Arc::new(AtomicBool::new(false)),
                )
                .is_ok());
            })
            .unwrap();
        let mut rng = rand::thread_rng();
        let mut expected_messages = HashMap::new();
        // Skip the first 2 validators, because 0 is myself, we only need 8 more to reach > 80%.
        for keypairs in validator_voting_keypairs.iter().skip(2) {
            let node_pubkey = keypairs.node_keypair.pubkey();
            let node = LegacyContactInfo::new_rand(&mut rng, Some(node_pubkey));
            let last_vote_hash = Hash::new_unique();
            push_restart_last_voted_fork_slots(
                cluster_info.clone(),
                &node,
                &expected_slots_to_repair,
                &last_vote_hash,
                shred_version,
                &keypairs.node_keypair,
            );
            expected_messages.insert(
                node_pubkey.to_string(),
                LastVotedForkSlotsRecord {
                    last_vote_fork_slots: expected_slots_to_repair.clone(),
                    last_vote_bankhash: last_vote_hash.to_string(),
                    shred_version: shred_version as u32,
                },
            );
        }
        let mut parent_bank = bank_forks.read().unwrap().root_bank();
        for slot in expected_slots_to_repair {
            let mut my_bank_forks = bank_forks.write().unwrap();
            my_bank_forks.insert(Bank::new_from_parent(
                parent_bank.clone(),
                &Pubkey::default(),
                slot,
            ));
            parent_bank = my_bank_forks.get(slot).unwrap();
            parent_bank.freeze();
        }
        let _ = wen_restart_thread_handle.join();
        let buffer = read(wen_restart_proto_path).unwrap();
        let progress = WenRestartProgress::decode(&mut std::io::Cursor::new(buffer)).unwrap();
        last_vote_fork_slots.sort();
        last_vote_fork_slots.reverse();
        assert_eq!(
            progress,
            WenRestartProgress {
                state: RestartState::Done.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_vote_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: 2,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: expected_messages
                }),
            }
        )
    }

    fn change_proto_file_readonly(wen_restart_proto_path: &PathBuf, readonly: bool) {
        let mut perms = std::fs::metadata(wen_restart_proto_path)
            .unwrap()
            .permissions();
        perms.set_readonly(readonly);
        std::fs::set_permissions(wen_restart_proto_path, perms).unwrap();
    }

    #[test]
    fn test_wen_restart_failures() {
        let shred_version = 2;
        let expected_slots = 400;
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let mut wen_restart_proto_path = ledger_path.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let wen_restart_repair_slots = Some(Arc::new(RwLock::new(Vec::new())));
        let mut last_vote_fork_slots = Vec::new();
        let (_, blockstore, cluster_info, bank_forks) = wen_restart_test_init(
            &ledger_path,
            expected_slots,
            shred_version,
            &mut last_vote_fork_slots,
        );
        let init_progress = WenRestartProgress {
            state: RestartState::Init.into(),
            my_last_voted_fork_slots: None,
            last_voted_fork_slots_aggregate: None,
        };
        let last_vote_slot: Slot = (RestartLastVotedForkSlots::MAX_SLOTS + expected_slots)
            .try_into()
            .unwrap();
        let last_vote_bankhash = Hash::new_unique();
        let last_voted_fork_slots_progress = WenRestartProgress {
            state: RestartState::LastVotedForkSlots.into(),
            my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                last_vote_fork_slots: vec![last_vote_slot],
                last_vote_bankhash: last_vote_bankhash.to_string(),
                shred_version: shred_version as u32,
            }),
            last_voted_fork_slots_aggregate: None,
        };
        for (progress, last_vote, setup, expected_err, cleanup) in vec![
            (
                init_progress.clone(),
                VoteTransaction::from(VoteStateUpdate::from(vec![(0, 8), (last_vote_slot, 1)])),
                None,
                "last vote is not a vote transaction",
                None,
            ),
            (
                init_progress,
                VoteTransaction::from(Vote::new(vec![], Hash::new_unique())),
                None,
                "last vote does not have last voted slot",
                None,
            ),
            (
                last_voted_fork_slots_progress,
                VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
                Some(|| change_proto_file_readonly(&wen_restart_proto_path, true)),
                "Permission denied",
                Some(|| change_proto_file_readonly(&wen_restart_proto_path, false)),
            ),
        ] {
            assert!(write_wen_restart_records(&wen_restart_proto_path, &progress,).is_ok());
            if let Some(setup_function) = setup {
                setup_function();
            }
            assert_matches!(wait_for_wen_restart(
                &wen_restart_proto_path,
                last_vote,
                blockstore.clone(),
                cluster_info.clone(),
                bank_forks.clone(),
                wen_restart_repair_slots.clone(),
                80,
                Arc::new(AtomicBool::new(false)),
            ).err().unwrap().to_string(), message if message.contains(expected_err));
            if let Some(cleanup_function) = cleanup {
                cleanup_function();
            }
        }
    }

    #[test]
    fn test_increment_and_write_wen_restart_records() {
        let mut progress = WenRestartProgress {
            state: RestartState::Init.into(),
            my_last_voted_fork_slots: None,
            last_voted_fork_slots_aggregate: None,
        };
        let my_dir = TempDir::new().unwrap();
        let mut wen_restart_proto_path = my_dir.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        for expected_state in [
            RestartState::LastVotedForkSlots,
            RestartState::HeaviestFork,
            RestartState::Done,
        ] {
            assert!(increment_and_write_wen_restart_records(
                &wen_restart_proto_path,
                &mut progress
            )
            .is_ok());
            assert_eq!(progress.state(), expected_state);
        }
        assert!(
            increment_and_write_wen_restart_records(&wen_restart_proto_path, &mut progress)
                .is_err()
        );
    }
}
