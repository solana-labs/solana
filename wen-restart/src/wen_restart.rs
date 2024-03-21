//! The `wen-restart` module handles automatic repair during a cluster restart

use {
    crate::{
        last_voted_fork_slots_aggregate::{
            LastVotedForkSlotsAggregate, LastVotedForkSlotsFinalResult,
        },
        solana::wen_restart_proto::{
            self, HeaviestFork, LastVotedForkSlotsAggregateFinal,
            LastVotedForkSlotsAggregateRecord, LastVotedForkSlotsRecord, State as RestartState,
            WenRestartProgress,
        },
    },
    anyhow::Result,
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
        io::{Cursor, Write},
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
// When counting Heaviest Fork, only count those with no less than
// 67% - 5% - (100% - active_stake) = active_stake - 38% stake.
const HEAVIEST_FORK_THRESHOLD_DELTA: f64 = 0.38;

#[derive(Debug, PartialEq)]
pub enum WenRestartError {
    BlockNotFound(Slot),
    BlockNotLinkedToExpectedParent(Slot, Option<Slot>, Slot),
    ChildStakeLargerThanParent(Slot, u64, Slot, u64),
    Exiting,
    InvalidLastVoteType(VoteTransaction),
    MalformedLastVotedForkSlotsProtobuf(Option<LastVotedForkSlotsRecord>),
    MissingLastVotedForkSlots,
    UnexpectedState(wen_restart_proto::State),
}

impl std::fmt::Display for WenRestartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WenRestartError::BlockNotFound(slot) => {
                write!(f, "Block not found: {}", slot)
            }
            WenRestartError::BlockNotLinkedToExpectedParent(slot, parent, expected_parent) => {
                write!(
                    f,
                    "Block {} is not linked to expected parent {} but to {:?}",
                    slot, expected_parent, parent
                )
            }
            WenRestartError::ChildStakeLargerThanParent(
                slot,
                child_stake,
                parent,
                parent_stake,
            ) => {
                write!(
                    f,
                    "Block {} has more stake {} than its parent {} with stake {}",
                    slot, child_stake, parent, parent_stake
                )
            }
            WenRestartError::Exiting => write!(f, "Exiting"),
            WenRestartError::InvalidLastVoteType(vote) => {
                write!(f, "Invalid last vote type: {:?}", vote)
            }
            WenRestartError::MalformedLastVotedForkSlotsProtobuf(record) => {
                write!(f, "Malformed last voted fork slots protobuf: {:?}", record)
            }
            WenRestartError::MissingLastVotedForkSlots => {
                write!(f, "Missing last voted fork slots")
            }
            WenRestartError::UnexpectedState(state) => {
                write!(f, "Unexpected state: {:?}", state)
            }
        }
    }
}

impl std::error::Error for WenRestartError {}

// We need a WenRestartProgressInternalState so we can convert the protobuf written in file
// into internal data structure in the initialize function. It should be easily
// convertable to and from WenRestartProgress protobuf.
#[derive(Debug, PartialEq)]
pub(crate) enum WenRestartProgressInternalState {
    Init {
        last_voted_fork_slots: Vec<Slot>,
        last_vote_bankhash: Hash,
    },
    LastVotedForkSlots {
        last_voted_fork_slots: Vec<Slot>,
        aggregate_final_result: Option<LastVotedForkSlotsFinalResult>,
    },
    FindHeaviestFork {
        aggregate_final_result: LastVotedForkSlotsFinalResult,
        my_heaviest_fork: Option<HeaviestFork>,
    },
    Done,
}

pub(crate) fn send_restart_last_voted_fork_slots(
    cluster_info: Arc<ClusterInfo>,
    last_voted_fork_slots: &[Slot],
    last_vote_bankhash: Hash,
) -> Result<LastVotedForkSlotsRecord> {
    cluster_info.push_restart_last_voted_fork_slots(last_voted_fork_slots, last_vote_bankhash)?;
    Ok(LastVotedForkSlotsRecord {
        last_voted_fork_slots: last_voted_fork_slots.to_vec(),
        last_vote_bankhash: last_vote_bankhash.to_string(),
        shred_version: cluster_info.my_shred_version() as u32,
        wallclock: timestamp(),
    })
}

pub(crate) fn aggregate_restart_last_voted_fork_slots(
    wen_restart_path: &PathBuf,
    wait_for_supermajority_threshold_percent: u64,
    cluster_info: Arc<ClusterInfo>,
    last_voted_fork_slots: &Vec<Slot>,
    bank_forks: Arc<RwLock<BankForks>>,
    blockstore: Arc<Blockstore>,
    wen_restart_repair_slots: Arc<RwLock<Vec<Slot>>>,
    exit: Arc<AtomicBool>,
    progress: &mut WenRestartProgress,
) -> Result<LastVotedForkSlotsFinalResult> {
    let root_bank;
    {
        root_bank = bank_forks.read().unwrap().root_bank().clone();
    }
    let root_slot = root_bank.slot();
    let mut last_voted_fork_slots_aggregate = LastVotedForkSlotsAggregate::new(
        root_slot,
        REPAIR_THRESHOLD,
        root_bank.epoch_stakes(root_bank.epoch()).unwrap(),
        last_voted_fork_slots,
        &cluster_info.id(),
    );
    if let Some(aggregate_record) = &progress.last_voted_fork_slots_aggregate {
        for (key_string, message) in &aggregate_record.received {
            if let Err(e) =
                last_voted_fork_slots_aggregate.aggregate_from_record(key_string, message)
            {
                error!("Failed to aggregate from record: {:?}", e);
            }
        }
    } else {
        progress.last_voted_fork_slots_aggregate = Some(LastVotedForkSlotsAggregateRecord {
            received: HashMap::new(),
            final_result: None,
        });
    }
    let mut cursor = solana_gossip::crds::Cursor::default();
    let mut is_full_slots = HashSet::new();
    loop {
        if exit.load(Ordering::Relaxed) {
            return Err(WenRestartError::Exiting.into());
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
        // Because all operations on the aggregate are called from this single thread, we can
        // fetch all results separately without worrying about them being out of sync. We can
        // also use returned iterator without the vector changing underneath us.
        let active_percent = last_voted_fork_slots_aggregate.active_percent();
        let mut filtered_slots: Vec<Slot>;
        {
            filtered_slots = last_voted_fork_slots_aggregate
                .slots_to_repair_iter()
                .filter(|slot| {
                    if *slot <= &root_slot || is_full_slots.contains(*slot) {
                        return false;
                    }
                    if blockstore.is_full(**slot) {
                        is_full_slots.insert(**slot);
                        false
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
        }
        filtered_slots.sort();
        info!(
            "Active peers: {} Slots to repair: {:?}",
            active_percent, &filtered_slots
        );
        if filtered_slots.is_empty()
            && active_percent > wait_for_supermajority_threshold_percent as f64
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
    Ok(last_voted_fork_slots_aggregate.get_final_result())
}

// Verify that all blocks with at least (active_stake_percnet - 38%) of the stake form a
// single chain from the root, and use the highest slot in the blocks as the heaviest fork.
// Please see SIMD 46 "gossip current heaviest fork" for correctness proof.
pub(crate) fn find_heaviest_fork(
    aggregate_final_result: LastVotedForkSlotsFinalResult,
    bank_forks: Arc<RwLock<BankForks>>,
    blockstore: Arc<Blockstore>,
    exit: Arc<AtomicBool>,
) -> Result<(Slot, Hash)> {
    // Because everything else is stopped, it's okay to grab a big lock on bank_forks.
    let my_bank_forks = bank_forks.read().unwrap();
    let root_bank = my_bank_forks.root_bank().clone();
    let root_slot = root_bank.slot();
    // TODO: Should use better epoch_stakes later.
    let epoch_stake = root_bank.epoch_stakes(root_bank.epoch()).unwrap();
    let total_stake = epoch_stake.total_stake();
    let stake_threshold = aggregate_final_result
        .total_active_stake
        .saturating_sub((HEAVIEST_FORK_THRESHOLD_DELTA * total_stake as f64) as u64);
    let mut slots = aggregate_final_result
        .slots_stake_map
        .iter()
        .filter(|(slot, stake)| **slot > root_slot && **stake > stake_threshold)
        .map(|(slot, _)| *slot)
        .collect::<Vec<Slot>>();
    slots.sort();
    let mut expected_parent = root_slot;
    for slot in slots {
        if exit.load(Ordering::Relaxed) {
            return Err(WenRestartError::Exiting.into());
        }
        if let Ok(Some(block_meta)) = blockstore.meta(slot) {
            if block_meta.parent_slot != Some(expected_parent) {
                if expected_parent == root_slot {
                    error!("First block {} in repair list not linked to local root {}, this could mean our root is too old",
                        slot, root_slot);
                } else {
                    error!(
                        "Block {} in blockstore is not linked to expected parent from Wen Restart {} but to Block {:?}",
                        slot, expected_parent, block_meta.parent_slot
                    );
                }
                return Err(WenRestartError::BlockNotLinkedToExpectedParent(
                    slot,
                    block_meta.parent_slot,
                    expected_parent,
                )
                .into());
            }
            expected_parent = slot;
        } else {
            return Err(WenRestartError::BlockNotFound(slot).into());
        }
    }
    Ok((expected_parent, Hash::default()))
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
) -> Result<()> {
    let (mut state, mut progress) =
        initialize(wen_restart_path, last_vote.clone(), blockstore.clone())?;
    loop {
        state = match state {
            WenRestartProgressInternalState::Init {
                last_voted_fork_slots,
                last_vote_bankhash,
            } => {
                progress.my_last_voted_fork_slots = Some(send_restart_last_voted_fork_slots(
                    cluster_info.clone(),
                    &last_voted_fork_slots,
                    last_vote_bankhash,
                )?);
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots,
                    last_vote_bankhash,
                }
            }
            WenRestartProgressInternalState::LastVotedForkSlots {
                last_voted_fork_slots,
                aggregate_final_result,
            } => {
                let final_result = match aggregate_final_result {
                    Some(result) => result,
                    None => aggregate_restart_last_voted_fork_slots(
                        wen_restart_path,
                        wait_for_supermajority_threshold_percent,
                        cluster_info.clone(),
                        &last_voted_fork_slots,
                        bank_forks.clone(),
                        blockstore.clone(),
                        wen_restart_repair_slots.clone().unwrap(),
                        exit.clone(),
                        &mut progress,
                    )?,
                };
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots,
                    aggregate_final_result: Some(final_result),
                }
            }
            WenRestartProgressInternalState::FindHeaviestFork {
                aggregate_final_result,
                my_heaviest_fork,
            } => {
                let heaviest_fork = match my_heaviest_fork {
                    Some(heaviest_fork) => heaviest_fork,
                    None => {
                        let total_active_stake = aggregate_final_result.total_active_stake;
                        let (slot, bankhash) = find_heaviest_fork(
                            aggregate_final_result.clone(),
                            bank_forks.clone(),
                            blockstore.clone(),
                            exit.clone(),
                        )?;
                        info!(
                            "Heaviest fork found: slot: {}, bankhash: {}",
                            slot, bankhash
                        );
                        HeaviestFork {
                            slot,
                            bankhash: bankhash.to_string(),
                            total_active_stake,
                        }
                    }
                };
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result,
                    my_heaviest_fork: Some(heaviest_fork),
                }
            }
            WenRestartProgressInternalState::Done => return Ok(()),
        };
        state = increment_and_write_wen_restart_records(wen_restart_path, state, &mut progress)?;
    }
}

pub(crate) fn increment_and_write_wen_restart_records(
    records_path: &PathBuf,
    current_state: WenRestartProgressInternalState,
    progress: &mut WenRestartProgress,
) -> Result<WenRestartProgressInternalState> {
    let new_state = match current_state {
        WenRestartProgressInternalState::Init {
            last_voted_fork_slots,
            last_vote_bankhash: _,
        } => {
            progress.set_state(RestartState::LastVotedForkSlots);
            WenRestartProgressInternalState::LastVotedForkSlots {
                last_voted_fork_slots,
                aggregate_final_result: None,
            }
        }
        WenRestartProgressInternalState::LastVotedForkSlots {
            last_voted_fork_slots: _,
            aggregate_final_result,
        } => {
            if let Some(aggregate_final_result) = aggregate_final_result {
                progress.set_state(RestartState::HeaviestFork);
                if let Some(aggregate_record) = progress.last_voted_fork_slots_aggregate.as_mut() {
                    aggregate_record.final_result = Some(LastVotedForkSlotsAggregateFinal {
                        slots_stake_map: aggregate_final_result.slots_stake_map.clone(),
                        total_active_stake: aggregate_final_result.total_active_stake,
                    });
                }
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result,
                    my_heaviest_fork: None,
                }
            } else {
                return Err(
                    WenRestartError::UnexpectedState(RestartState::LastVotedForkSlots).into(),
                );
            }
        }
        WenRestartProgressInternalState::FindHeaviestFork {
            aggregate_final_result: _,
            my_heaviest_fork,
        } => {
            if let Some(my_heaviest_fork) = my_heaviest_fork {
                progress.set_state(RestartState::Done);
                progress.my_heaviest_fork = Some(my_heaviest_fork.clone());
                WenRestartProgressInternalState::Done
            } else {
                return Err(WenRestartError::UnexpectedState(RestartState::HeaviestFork).into());
            }
        }
        WenRestartProgressInternalState::Done => {
            return Err(WenRestartError::UnexpectedState(RestartState::Done).into())
        }
    };
    write_wen_restart_records(records_path, progress)?;
    Ok(new_state)
}

pub(crate) fn initialize(
    records_path: &PathBuf,
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
) -> Result<(WenRestartProgressInternalState, WenRestartProgress)> {
    let progress = match read_wen_restart_records(records_path) {
        Ok(progress) => progress,
        Err(e) => {
            let stdio_err = e.downcast_ref::<std::io::Error>();
            if stdio_err.is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
                info!(
                    "wen restart proto file not found at {:?}, write init state",
                    records_path
                );
                let progress = WenRestartProgress {
                    state: RestartState::Init.into(),
                    ..Default::default()
                };
                write_wen_restart_records(records_path, &progress)?;
                progress
            } else {
                return Err(e);
            }
        }
    };
    match progress.state() {
        RestartState::Done => Ok((WenRestartProgressInternalState::Done, progress)),
        RestartState::Init => {
            let last_voted_fork_slots;
            let last_vote_bankhash;
            match &progress.my_last_voted_fork_slots {
                Some(my_last_voted_fork_slots) => {
                    last_voted_fork_slots = my_last_voted_fork_slots.last_voted_fork_slots.clone();
                    last_vote_bankhash =
                        Hash::from_str(&my_last_voted_fork_slots.last_vote_bankhash).unwrap();
                }
                None => {
                    // repair and restart option does not work without last voted slot.
                    if let VoteTransaction::Vote(ref vote) = last_vote {
                        if let Some(last_vote_slot) = vote.last_voted_slot() {
                            last_vote_bankhash = vote.hash;
                            last_voted_fork_slots =
                                AncestorIterator::new_inclusive(last_vote_slot, &blockstore)
                                    .take(RestartLastVotedForkSlots::MAX_SLOTS)
                                    .collect();
                        } else {
                            error!("
                                Cannot find last voted slot in the tower storage, it either means that this node has never \
                                voted or the tower storage is corrupted. Unfortunately, since WenRestart is a consensus protocol \
                                depending on each participant to send their last voted fork slots, your validator cannot participate.\
                                Please check discord for the conclusion of the WenRestart protocol, then generate a snapshot and use \
                                --wait-for-supermajority to restart the validator.");
                            return Err(WenRestartError::MissingLastVotedForkSlots.into());
                        }
                    } else {
                        return Err(WenRestartError::InvalidLastVoteType(last_vote).into());
                    }
                }
            }
            Ok((
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots,
                    last_vote_bankhash,
                },
                progress,
            ))
        }
        RestartState::LastVotedForkSlots => {
            if let Some(record) = progress.my_last_voted_fork_slots.as_ref() {
                Ok((
                    WenRestartProgressInternalState::LastVotedForkSlots {
                        last_voted_fork_slots: record.last_voted_fork_slots.clone(),
                        aggregate_final_result: progress
                            .last_voted_fork_slots_aggregate
                            .as_ref()
                            .and_then(|r| {
                                r.final_result.as_ref().map(|result| {
                                    LastVotedForkSlotsFinalResult {
                                        slots_stake_map: result.slots_stake_map.clone(),
                                        total_active_stake: result.total_active_stake,
                                    }
                                })
                            }),
                    },
                    progress,
                ))
            } else {
                Err(WenRestartError::MalformedLastVotedForkSlotsProtobuf(None).into())
            }
        }
        RestartState::HeaviestFork => Ok((
            WenRestartProgressInternalState::FindHeaviestFork {
                aggregate_final_result: progress
                    .last_voted_fork_slots_aggregate
                    .as_ref()
                    .and_then(|r| {
                        r.final_result
                            .as_ref()
                            .map(|result| LastVotedForkSlotsFinalResult {
                                slots_stake_map: result.slots_stake_map.clone(),
                                total_active_stake: result.total_active_stake,
                            })
                    })
                    .unwrap(),
                my_heaviest_fork: progress.my_heaviest_fork.clone(),
            },
            progress,
        )),
        _ => Err(WenRestartError::UnexpectedState(progress.state()).into()),
    }
}

fn read_wen_restart_records(records_path: &PathBuf) -> Result<WenRestartProgress> {
    let buffer = read(records_path)?;
    let progress = WenRestartProgress::decode(&mut Cursor::new(buffer))?;
    info!("read record {:?}", progress);
    Ok(progress)
}

pub(crate) fn write_wen_restart_records(
    records_path: &PathBuf,
    new_progress: &WenRestartProgress,
) -> Result<()> {
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
        crate::wen_restart::{tests::wen_restart_proto::LastVotedForkSlotsAggregateFinal, *},
        assert_matches::assert_matches,
        solana_gossip::{
            cluster_info::ClusterInfo,
            contact_info::ContactInfo,
            crds::GossipRoute,
            crds_value::{CrdsData, CrdsValue},
            legacy_contact_info::LegacyContactInfo,
            restart_crds_values::RestartLastVotedForkSlots,
        },
        solana_ledger::{
            blockstore::{make_chaining_slot_entries, Blockstore},
            get_tmp_ledger_path_auto_delete,
        },
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
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{fs::remove_file, sync::Arc, thread::Builder},
        tempfile::TempDir,
    };

    const SHRED_VERSION: u16 = 2;
    const EXPECTED_SLOTS: usize = 400;

    fn push_restart_last_voted_fork_slots(
        cluster_info: Arc<ClusterInfo>,
        node: &LegacyContactInfo,
        last_voted_fork_slots: &[Slot],
        last_vote_hash: &Hash,
        node_keypair: &Keypair,
        wallclock: u64,
    ) {
        let slots = RestartLastVotedForkSlots::new(
            *node.pubkey(),
            wallclock,
            last_voted_fork_slots,
            *last_vote_hash,
            SHRED_VERSION,
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

    struct WenRestartTestInitResult {
        pub validator_voting_keypairs: Vec<ValidatorVoteKeypairs>,
        pub blockstore: Arc<Blockstore>,
        pub cluster_info: Arc<ClusterInfo>,
        pub bank_forks: Arc<RwLock<BankForks>>,
        pub last_voted_fork_slots: Vec<Slot>,
        pub wen_restart_proto_path: PathBuf,
    }

    fn insert_slots_into_blockstore(
        blockstore: Arc<Blockstore>,
        first_parent: Slot,
        slots_to_insert: &[Slot],
    ) {
        for (shreds, _) in make_chaining_slot_entries(slots_to_insert, 2, first_parent) {
            blockstore.insert_shreds(shreds, None, false).unwrap();
        }
    }

    fn wen_restart_test_init(ledger_path: &TempDir) -> WenRestartTestInitResult {
        let validator_voting_keypairs: Vec<_> =
            (0..10).map(|_| ValidatorVoteKeypairs::new_rand()).collect();
        let node_keypair = Arc::new(validator_voting_keypairs[0].node_keypair.insecure_clone());
        let cluster_info = Arc::new(ClusterInfo::new(
            {
                let mut contact_info =
                    ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp());
                contact_info.set_shred_version(SHRED_VERSION);
                contact_info
            },
            node_keypair.clone(),
            SocketAddrSpace::Unspecified,
        ));
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
            10_000,
            &validator_voting_keypairs,
            vec![100; validator_voting_keypairs.len()],
        );
        let (_, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        let last_parent = (RestartLastVotedForkSlots::MAX_SLOTS >> 1)
            .try_into()
            .unwrap();
        let mut last_voted_fork_slots = Vec::new();
        last_voted_fork_slots.extend([1, last_parent]);
        for i in 0..EXPECTED_SLOTS {
            last_voted_fork_slots.push(
                (RestartLastVotedForkSlots::MAX_SLOTS
                    .saturating_add(i)
                    .saturating_add(1)) as Slot,
            );
        }
        insert_slots_into_blockstore(blockstore.clone(), 0, &last_voted_fork_slots);
        last_voted_fork_slots.insert(0, 0);
        last_voted_fork_slots.reverse();
        let mut wen_restart_proto_path = ledger_path.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let _ = remove_file(&wen_restart_proto_path);
        WenRestartTestInitResult {
            validator_voting_keypairs,
            blockstore,
            cluster_info,
            bank_forks,
            last_voted_fork_slots,
            wen_restart_proto_path,
        }
    }

    const WAIT_FOR_THREAD_TIMEOUT: u64 = 10_000;

    fn wait_on_expected_progress_with_timeout(
        wen_restart_proto_path: PathBuf,
        expected_progress: WenRestartProgress,
    ) {
        let start = timestamp();
        let mut progress = WenRestartProgress {
            state: RestartState::Init.into(),
            ..Default::default()
        };
        loop {
            if let Ok(new_progress) = read_wen_restart_records(&wen_restart_proto_path) {
                progress = new_progress;
                if let Some(my_last_voted_fork_slots) = &expected_progress.my_last_voted_fork_slots
                {
                    if let Some(record) = progress.my_last_voted_fork_slots.as_mut() {
                        record.wallclock = my_last_voted_fork_slots.wallclock;
                    }
                }
                if progress == expected_progress {
                    return;
                }
            }
            if timestamp().saturating_sub(start) > WAIT_FOR_THREAD_TIMEOUT {
                assert_eq!(
                    progress.my_last_voted_fork_slots,
                    expected_progress.my_last_voted_fork_slots
                );
                assert_eq!(
                    progress.last_voted_fork_slots_aggregate,
                    expected_progress.last_voted_fork_slots_aggregate
                );
                panic!(
                    "wait_on_expected_progress_with_timeout failed to get expected progress {:?} expected {:?}",
                    &progress,
                    expected_progress
                );
            }
            sleep(Duration::from_millis(10));
        }
    }

    fn wen_restart_test_succeed_after_failure(
        test_state: WenRestartTestInitResult,
        last_vote_bankhash: Hash,
        expected_progress: WenRestartProgress,
    ) {
        let wen_restart_proto_path_clone = test_state.wen_restart_proto_path.clone();
        // continue normally after the error, we should be good.
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let last_vote_slot: Slot = test_state.last_voted_fork_slots[0];
        let wen_restart_thread_handle = Builder::new()
            .name("solana-wen-restart".to_string())
            .spawn(move || {
                let _ = wait_for_wen_restart(
                    &wen_restart_proto_path_clone,
                    VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
                    test_state.blockstore,
                    test_state.cluster_info,
                    test_state.bank_forks,
                    Some(Arc::new(RwLock::new(Vec::new()))),
                    80,
                    exit_clone,
                );
            })
            .unwrap();
        wait_on_expected_progress_with_timeout(
            test_state.wen_restart_proto_path.clone(),
            expected_progress,
        );
        exit.store(true, Ordering::Relaxed);
        let _ = wen_restart_thread_handle.join();
        let _ = remove_file(&test_state.wen_restart_proto_path);
    }

    #[test]
    fn test_wen_restart_normal_flow() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let wen_restart_repair_slots = Some(Arc::new(RwLock::new(Vec::new())));
        let test_state = wen_restart_test_init(&ledger_path);
        let wen_restart_proto_path_clone = test_state.wen_restart_proto_path.clone();
        let cluster_info_clone = test_state.cluster_info.clone();
        let last_vote_slot = test_state.last_voted_fork_slots[0];
        let last_vote_bankhash = Hash::new_unique();
        let expected_slots_to_repair: Vec<Slot> =
            (last_vote_slot + 1..last_vote_slot + 3).collect();
        let blockstore_clone = test_state.blockstore.clone();
        let bank_forks_clone = test_state.bank_forks.clone();
        let wen_restart_thread_handle = Builder::new()
            .name("solana-wen-restart".to_string())
            .spawn(move || {
                assert!(wait_for_wen_restart(
                    &wen_restart_proto_path_clone,
                    VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
                    blockstore_clone,
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
        let mut last_voted_fork_slots_from_others = test_state.last_voted_fork_slots.clone();
        last_voted_fork_slots_from_others.reverse();
        last_voted_fork_slots_from_others.append(&mut expected_slots_to_repair.clone());
        for keypairs in test_state.validator_voting_keypairs.iter().skip(2) {
            let node_pubkey = keypairs.node_keypair.pubkey();
            let node = LegacyContactInfo::new_rand(&mut rng, Some(node_pubkey));
            let last_vote_hash = Hash::new_unique();
            let now = timestamp();
            push_restart_last_voted_fork_slots(
                test_state.cluster_info.clone(),
                &node,
                &last_voted_fork_slots_from_others,
                &last_vote_hash,
                &keypairs.node_keypair,
                now,
            );
            expected_messages.insert(
                node_pubkey.to_string(),
                LastVotedForkSlotsRecord {
                    last_voted_fork_slots: last_voted_fork_slots_from_others.clone(),
                    last_vote_bankhash: last_vote_hash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                },
            );
        }

        // Simulating successful repair of missing blocks.
        insert_slots_into_blockstore(
            test_state.blockstore.clone(),
            last_vote_slot,
            &expected_slots_to_repair,
        );

        let _ = wen_restart_thread_handle.join();
        let progress = read_wen_restart_records(&test_state.wen_restart_proto_path).unwrap();
        let progress_start_time = progress
            .my_last_voted_fork_slots
            .as_ref()
            .unwrap()
            .wallclock;
        let mut expected_slots_stake_map: HashMap<Slot, u64> = test_state
            .last_voted_fork_slots
            .iter()
            .map(|slot| (*slot, 900))
            .collect();
        expected_slots_stake_map.extend(expected_slots_to_repair.iter().map(|slot| (*slot, 800)));
        let expected_heaviest_fork_slot = last_vote_slot + 2;
        let expected_heaviest_fork_bankhash = Hash::default();
        assert_eq!(
            progress,
            WenRestartProgress {
                state: RestartState::Done.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots: test_state.last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: progress_start_time,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: expected_messages,
                    final_result: Some(LastVotedForkSlotsAggregateFinal {
                        slots_stake_map: expected_slots_stake_map,
                        total_active_stake: 900,
                    }),
                }),
                my_heaviest_fork: Some(HeaviestFork {
                    slot: expected_heaviest_fork_slot,
                    bankhash: expected_heaviest_fork_bankhash.to_string(),
                    total_active_stake: 900
                }),
            }
        );
    }

    fn change_proto_file_readonly(wen_restart_proto_path: &PathBuf, readonly: bool) {
        let mut perms = std::fs::metadata(wen_restart_proto_path)
            .unwrap()
            .permissions();
        perms.set_readonly(readonly);
        std::fs::set_permissions(wen_restart_proto_path, perms).unwrap();
    }

    #[test]
    fn test_wen_restart_initialize_failures() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_bankhash = Hash::new_unique();
        let mut last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        last_voted_fork_slots.reverse();
        let mut file = File::create(&test_state.wen_restart_proto_path).unwrap();
        file.write_all(b"garbage").unwrap();
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .unwrap_err()
            .downcast::<prost::DecodeError>()
            .unwrap(),
            prost::DecodeError::new("invalid wire type value: 7")
        );
        remove_file(&test_state.wen_restart_proto_path).unwrap();
        let invalid_last_vote = VoteTransaction::from(VoteStateUpdate::from(vec![(0, 8), (1, 1)]));
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                invalid_last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::InvalidLastVoteType(invalid_last_vote)
        );
        let empty_last_vote = VoteTransaction::from(Vote::new(vec![], last_vote_bankhash));
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                empty_last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::MissingLastVotedForkSlots,
        );
        // Test the case where the file is not found.
        let _ = remove_file(&test_state.wen_restart_proto_path);
        assert_matches!(
            initialize(&test_state.wen_restart_proto_path, VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)), test_state.blockstore.clone()),
            Ok((WenRestartProgressInternalState::Init { last_voted_fork_slots, last_vote_bankhash: bankhash }, progress)) => {
                assert_eq!(last_voted_fork_slots, test_state.last_voted_fork_slots);
                assert_eq!(bankhash, last_vote_bankhash);
                assert_eq!(progress, WenRestartProgress {
                    state: RestartState::Init.into(),
                    ..Default::default()
                });
            }
        );
        let _ = write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                ..Default::default()
            },
        );
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .err()
            .unwrap()
            .to_string(),
            "Malformed last voted fork slots protobuf: None"
        );
        let _ = write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &WenRestartProgress {
                state: RestartState::WaitingForSupermajority.into(),
                ..Default::default()
            },
        );
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots, last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .err()
            .unwrap()
            .to_string(),
            "Unexpected state: WaitingForSupermajority"
        );
    }

    #[test]
    fn test_wen_restart_send_last_voted_fork_failures() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let progress = wen_restart_proto::WenRestartProgress {
            state: RestartState::Init.into(),
            ..Default::default()
        };
        let original_progress = progress.clone();
        assert_eq!(
            send_restart_last_voted_fork_slots(
                test_state.cluster_info.clone(),
                &[],
                Hash::new_unique(),
            )
            .err()
            .unwrap()
            .to_string(),
            "Last voted fork cannot be empty"
        );
        assert_eq!(progress, original_progress);
        let last_vote_bankhash = Hash::new_unique();
        let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        wen_restart_test_succeed_after_failure(
            test_state,
            last_vote_bankhash,
            WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: 0,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: HashMap::new(),
                    final_result: None,
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_write_wen_restart_records_failure() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let progress = wen_restart_proto::WenRestartProgress {
            state: RestartState::Init.into(),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress).is_ok());
        change_proto_file_readonly(&test_state.wen_restart_proto_path, true);
        assert_eq!(
            write_wen_restart_records(&test_state.wen_restart_proto_path, &progress)
                .unwrap_err()
                .downcast::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::PermissionDenied,
        );
        change_proto_file_readonly(&test_state.wen_restart_proto_path, false);
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress).is_ok());
        let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        let last_vote_bankhash = Hash::new_unique();
        wen_restart_test_succeed_after_failure(
            test_state,
            last_vote_bankhash,
            WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: 0,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: HashMap::new(),
                    final_result: None,
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_wen_restart_aggregate_last_voted_fork_stop_and_restart() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_slot: Slot = test_state.last_voted_fork_slots[0];
        let last_vote_bankhash = Hash::new_unique();
        let start_time = timestamp();
        assert!(write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: start_time,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: HashMap::new(),
                    final_result: None,
                }),
                ..Default::default()
            }
        )
        .is_ok());
        let mut rng = rand::thread_rng();
        let mut expected_messages = HashMap::new();
        let expected_slots_to_repair: Vec<Slot> =
            (last_vote_slot + 1..last_vote_slot + 3).collect();
        let mut last_voted_fork_slots_from_others = test_state.last_voted_fork_slots.clone();
        last_voted_fork_slots_from_others.reverse();
        last_voted_fork_slots_from_others.append(&mut expected_slots_to_repair.clone());
        // Skip the first 2 validators, because 0 is myself, we need 8 so it hits 80%.
        assert_eq!(test_state.validator_voting_keypairs.len(), 10);
        let progress = WenRestartProgress {
            state: RestartState::LastVotedForkSlots.into(),
            my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: last_vote_bankhash.to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: start_time,
            }),
            ..Default::default()
        };
        for keypairs in test_state.validator_voting_keypairs.iter().skip(2) {
            let wen_restart_proto_path_clone = test_state.wen_restart_proto_path.clone();
            let cluster_info_clone = test_state.cluster_info.clone();
            let bank_forks_clone = test_state.bank_forks.clone();
            let blockstore_clone = test_state.blockstore.clone();
            let exit = Arc::new(AtomicBool::new(false));
            let exit_clone = exit.clone();
            let mut progress_clone = progress.clone();
            let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
            let wen_restart_thread_handle = Builder::new()
                .name("solana-wen-restart".to_string())
                .spawn(move || {
                    let _ = aggregate_restart_last_voted_fork_slots(
                        &wen_restart_proto_path_clone,
                        80,
                        cluster_info_clone,
                        &last_voted_fork_slots,
                        bank_forks_clone,
                        blockstore_clone,
                        Arc::new(RwLock::new(Vec::new())),
                        exit_clone,
                        &mut progress_clone,
                    );
                })
                .unwrap();
            let node_pubkey = keypairs.node_keypair.pubkey();
            let node = LegacyContactInfo::new_rand(&mut rng, Some(node_pubkey));
            let last_vote_hash = Hash::new_unique();
            let now = timestamp();
            push_restart_last_voted_fork_slots(
                test_state.cluster_info.clone(),
                &node,
                &last_voted_fork_slots_from_others,
                &last_vote_hash,
                &keypairs.node_keypair,
                now,
            );
            expected_messages.insert(
                node_pubkey.to_string(),
                LastVotedForkSlotsRecord {
                    last_voted_fork_slots: last_voted_fork_slots_from_others.clone(),
                    last_vote_bankhash: last_vote_hash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                },
            );
            wait_on_expected_progress_with_timeout(
                test_state.wen_restart_proto_path.clone(),
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                        last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                        last_vote_bankhash: last_vote_bankhash.to_string(),
                        shred_version: SHRED_VERSION as u32,
                        wallclock: start_time,
                    }),
                    last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                        received: expected_messages.clone(),
                        final_result: None,
                    }),
                    ..Default::default()
                },
            );
            exit.store(true, Ordering::Relaxed);
            let _ = wen_restart_thread_handle.join();
        }

        // Simulating successful repair of missing blocks.
        insert_slots_into_blockstore(
            test_state.blockstore.clone(),
            last_vote_slot,
            &expected_slots_to_repair,
        );

        let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        wen_restart_test_succeed_after_failure(
            test_state,
            last_vote_bankhash,
            WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: start_time,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: expected_messages,
                    final_result: None,
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_increment_and_write_wen_restart_records() {
        solana_logger::setup();
        let my_dir = TempDir::new().unwrap();
        let mut wen_restart_proto_path = my_dir.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let last_vote_bankhash = Hash::new_unique();
        let my_last_voted_fork_slots = Some(LastVotedForkSlotsRecord {
            last_voted_fork_slots: vec![0, 1],
            last_vote_bankhash: last_vote_bankhash.to_string(),
            shred_version: 0,
            wallclock: 0,
        });
        let last_voted_fork_slots_aggregate = Some(LastVotedForkSlotsAggregateRecord {
            received: HashMap::new(),
            final_result: Some(LastVotedForkSlotsAggregateFinal {
                slots_stake_map: vec![(0, 900), (1, 800)].into_iter().collect(),
                total_active_stake: 900,
            }),
        });
        let expected_slots_stake_map: HashMap<Slot, u64> =
            vec![(0, 900), (1, 800)].into_iter().collect();
        for (entrance_state, exit_state, entrance_progress, exit_progress) in [
            (
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots: vec![0, 1],
                    last_vote_bankhash,
                },
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots: vec![0, 1],
                    aggregate_final_result: None,
                },
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    ..Default::default()
                },
            ),
            (
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots: vec![0, 1],
                    aggregate_final_result: Some(LastVotedForkSlotsFinalResult {
                        slots_stake_map: expected_slots_stake_map.clone(),
                        total_active_stake: 900,
                    }),
                },
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result: LastVotedForkSlotsFinalResult {
                        slots_stake_map: expected_slots_stake_map.clone(),
                        total_active_stake: 900,
                    },
                    my_heaviest_fork: None,
                },
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    ..Default::default()
                },
            ),
            (
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result: LastVotedForkSlotsFinalResult {
                        slots_stake_map: expected_slots_stake_map,
                        total_active_stake: 900,
                    },
                    my_heaviest_fork: Some(HeaviestFork {
                        slot: 1,
                        bankhash: Hash::default().to_string(),
                        total_active_stake: 900,
                    }),
                },
                WenRestartProgressInternalState::Done,
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::Done.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    my_heaviest_fork: Some(HeaviestFork {
                        slot: 1,
                        bankhash: Hash::default().to_string(),
                        total_active_stake: 900,
                    }),
                },
            ),
        ] {
            let mut progress = entrance_progress;
            let state = increment_and_write_wen_restart_records(
                &wen_restart_proto_path,
                entrance_state,
                &mut progress,
            )
            .unwrap();
            assert_eq!(&state, &exit_state);
            assert_eq!(&progress, &exit_progress);
        }
        let mut progress = WenRestartProgress {
            state: RestartState::Done.into(),
            my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
            last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
            ..Default::default()
        };
        assert_eq!(
            increment_and_write_wen_restart_records(
                &wen_restart_proto_path,
                WenRestartProgressInternalState::Done,
                &mut progress
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::UnexpectedState(RestartState::Done),
        );
    }

    #[test]
    fn test_find_heaviest_fork_failures() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let exit = Arc::new(AtomicBool::new(false));
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_slot = test_state.last_voted_fork_slots[0];
        let slot_with_no_block = last_vote_slot + 5;
        // This fails because corresponding block is not found, which is wrong, we should have
        // repaired all eligible blocks when we exit LastVotedForkSlots state.
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map: vec![(0, 900), (slot_with_no_block, 800)]
                        .into_iter()
                        .collect(),
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotFound(slot_with_no_block),
        );
        // The following fails because we expect to see the first slot in slots_stake_map doesn't chain to local root.
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map: vec![(last_vote_slot, 900)].into_iter().collect(),
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotLinkedToExpectedParent(
                last_vote_slot,
                Some(last_vote_slot - 1),
                0
            ),
        );
        // The following fails because we expect to see the some slot in slots_stake_map doesn't chain to the
        // one before it.
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map: vec![(1, 900), (last_vote_slot, 900)].into_iter().collect(),
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotLinkedToExpectedParent(
                last_vote_slot,
                Some(last_vote_slot - 1),
                1
            ),
        );
    }
}
