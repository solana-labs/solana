//! Functions that change worker state according to the commands it receives, as described in
//! [`super::Command`].

use {
    super::{
        DataSetFirstShred, DataSetOptimisticStatus, LastSlotDataSet, SharedResults, SlotStatus,
        SlotVerificationStatus, Todo, TodoItem, TodoOp,
    },
    solana_entry::entry::Entry,
    solana_ledger::{blockstore::Blockstore, shred::Shred},
    solana_metrics::datapoint_error,
    solana_sdk::{clock::Slot, hash::Hash},
    std::{
        collections::{hash_map, HashMap},
        ops::RangeInclusive,
        sync::Arc,
    },
};

#[cfg(test)]
mod test;

/// Adds new data set into the data structures that track slot and data set verification progress.
///
/// Updates `todo`, with a verification request necessary to make sure that the newly added data set
/// is indeed valid.
///
/// Returns the parent slot, unless an error occurred, or no new work has been scheduled.
pub(super) fn add_data_set(
    todo: &mut Todo,
    blockstore: &Blockstore,
    processing_slots: &mut HashMap<Slot, SlotStatus>,
    waiting_for_parent_hash: &mut HashMap<Slot, Vec<Slot>>,
    results: &SharedResults,
    slot: Slot,
    shard_indices: RangeInclusive<u32>,
    last_in_slot: bool,
    entries: Arc<Vec<Entry>>,
) -> Option<Slot> {
    // Shreds should always contain some entries.  Or we would need to reevaluate the indexing
    // and the predecessor search logic.
    if entries.is_empty() {
        mark_slot_failed(processing_slots, waiting_for_parent_hash, results, slot);
        return None;
    }

    let data_set_start = DataSetFirstShred::from(*shard_indices.start());
    let next_data_set_start = DataSetFirstShred::from(*shard_indices.end() + 1);

    let last_entry_hash = entries.last().unwrap().hash;

    let (slot_status, parent_slot_last_entry_hash) = match get_or_insert_slot_for(
        blockstore,
        processing_slots,
        slot,
        data_set_start,
        last_in_slot.then(|| last_entry_hash),
    ) {
        (Some(slot_status), parent_slot_last_entry_hash) => {
            (slot_status, parent_slot_last_entry_hash)
        }
        (None, _) => {
            mark_slot_failed(processing_slots, waiting_for_parent_hash, results, slot);
            return None;
        }
    };

    let (parent_slot, data_sets) = match slot_status {
        SlotStatus::Processing {
            parent_slot,
            last_data_set,
            data_sets,
            ..
        } => {
            if last_in_slot {
                *last_data_set = Some(LastSlotDataSet {
                    index: data_set_start,
                    last_entry_hash,
                });
            }

            (*parent_slot, data_sets)
        }
        SlotStatus::Verified { .. } | SlotStatus::Failed => return None,
    };
    insert_data_set_entries(
        todo,
        waiting_for_parent_hash,
        data_sets,
        parent_slot,
        parent_slot_last_entry_hash,
        slot,
        data_set_start,
        entries,
        next_data_set_start,
    );

    // Insertion of the PoH start hash into a subsequent data set has to be split into two cases.
    //
    // If the update happens to be in the same slot, we need to do it before we get an exclusive
    // reference to the slot `data_sets`, needed for the main data set entries update via
    // `insert_data_set_entries()`.
    //
    // At the same time if the update crosses the slot boundary, we need to access other slots, and
    // for that we need to release the `slot_status` exclusive reference.  Making it possible to
    // access other slot entries in the `processing_slots`.
    //
    // First case, when we are not crossing the slot boundary.
    if !last_in_slot {
        insert_poh_start_hash(
            todo,
            slot,
            slot_status,
            next_data_set_start,
            last_entry_hash,
        );
    } else {
        insert_poh_start_hash_into_child_slots(
            todo,
            processing_slots,
            waiting_for_parent_hash,
            slot,
            last_entry_hash,
        );
    }

    Some(parent_slot)
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub(super) enum AccumulationDecision {
    CanWaitMore,
    RunVerification,
}

pub(super) fn waiting_for(
    todo: &Todo,
    client_waiting_for: &mut Option<(Slot, DataSetFirstShred)>,
    slot: Slot,
    data_set_start: DataSetFirstShred,
) -> AccumulationDecision {
    // A small optimization, for the case when the new request should have no impact.
    match client_waiting_for {
        Some((prev_slot, prev_data_set_start))
            if slot == *prev_slot && data_set_start <= *prev_data_set_start =>
        {
            return AccumulationDecision::CanWaitMore
        }
        _ => (),
    }

    *client_waiting_for = Some((slot, data_set_start));

    if todo.is_empty() {
        AccumulationDecision::CanWaitMore
    } else {
        // As `todo` could be long, it is safer to just run a verification pass, if there is
        // anything that we can do right now.
        //
        // We also, currently, do not record the shard indices in the `todo`, so in order to be more
        // precise here, we would need to extend the amount of data we store in `Todo`.
        //
        // It does not seem worth it.
        AccumulationDecision::RunVerification
    }
}

pub(super) fn cleanup(
    processing_slots: &mut HashMap<Slot, SlotStatus>,
    waiting_for_parent_hash: &mut HashMap<Slot, Vec<Slot>>,
    client_waiting_for: &mut Option<(Slot, DataSetFirstShred)>,
    results: &SharedResults,
    earlier_than: Slot,
) {
    processing_slots.retain(|slot, _status| *slot >= earlier_than);

    waiting_for_parent_hash.retain(|slot, _children| *slot >= earlier_than);
    // Parent slots should always have a smaller index than the children slots.  So if everything is
    // correct, there is no need to check the child slot lists.

    match client_waiting_for {
        Some((slot, _data_set_start)) if *slot < earlier_than => *client_waiting_for = None,
        _ => (),
    }

    results.update_notify_all(|results| {
        if results.starting_at >= earlier_than {
            return;
        }

        results.starting_at = earlier_than;
        results.slots.retain(|slot, _result| *slot >= earlier_than);
    });
}

/// Looks for an existing entry for a slot, or inserts a new entry and returns it.
///
/// Due to the borrow rules, in case the inserted data set is the first one in the slot, this
/// function also returns the parent slot last entry hash.  As this function returns an exclusive
/// reference to an element of the `processing_slots`, it also blocks any further accesses to the
/// `processing_slots`, and the caller will not be able to perform the parent slot lookup, until
/// they release the returned reference.
///
/// `last_entry_hash` should only be set when `data_set_start` references the last data set in the
/// `slot`.
fn get_or_insert_slot_for<'slots>(
    blockstore: &Blockstore,
    processing_slots: &'slots mut HashMap<Slot, SlotStatus>,
    slot: Slot,
    data_set_start: DataSetFirstShred,
    slot_last_entry_hash: Option<Hash>,
) -> (Option<&'slots mut SlotStatus>, Option<Hash>) {
    if data_set_start.is_first_in_slot() {
        get_or_insert_slot_for_first_slot_data_set(
            blockstore,
            processing_slots,
            slot,
            slot_last_entry_hash,
        )
    } else {
        let slot_status = get_or_insert_slot_for_non_first_slot_data_set(
            blockstore,
            processing_slots,
            slot,
            data_set_start,
            slot_last_entry_hash,
        );
        (slot_status, None)
    }
}

fn get_or_insert_slot_for_first_slot_data_set<'slots>(
    blockstore: &Blockstore,
    processing_slots: &'slots mut HashMap<Slot, SlotStatus>,
    slot: Slot,
    slot_last_entry_hash: Option<Hash>,
) -> (Option<&'slots mut SlotStatus>, Option<Hash>) {
    // We are trying to avoid a blockstore call, if we have already stored the parent slot index in
    // the slots.
    //
    // We also need to release the slot entry reference, in order to access the parent slot entry.
    let parent_slot = match processing_slots.get_mut(&slot) {
        Some(SlotStatus::Processing { parent_slot, .. }) => Some(*parent_slot),

        // It should be this:
        //
        // Some(slot_status @ (SlotStatus::Verified { .. } | SlotStatus::Failed)) => {
        //     // For an already verified or failed slots we are not going to do any more work,
        //     // so we can exit early, and not look for the PoH hash.
        //     //
        //     return (Some(slot_status), None);
        //
        // Unfortunately, the above does not work.  Borrow checker is unhappy.
        // These bugs seem relevant:
        //
        //   https://github.com/rust-lang/rust/issues/51545
        //   https://github.com/rust-lang/rust/issues/58910
        //
        // This removes a potential optimizations, as we know that we do not need to get the parent
        // slot, nor do we need to look for the slot entry in `processing_slots` again.
        // The optimizer might or might not be able to fix this.  And this is not a very common
        // case, so, not a big deal.
        Some(SlotStatus::Verified { .. } | SlotStatus::Failed) => None,

        None => match get_parent_slot(blockstore, slot, 0u32.into()) {
            Some(slot) => Some(slot),
            None => {
                return (None, None);
            }
        },
    };

    let parent_slot_last_entry_hash = parent_slot
        .map(|parent_slot| processing_slots.get(&parent_slot))
        .flatten()
        .map(|slot_status| slot_status.last_entry_hash())
        .flatten();

    let slot_status = processing_slots.entry(slot).or_insert_with(|| {
        let parent_slot = parent_slot.expect("Is set when slot was not initialized");
        let last_data_set = slot_last_entry_hash.map(|last_entry_hash| LastSlotDataSet {
            index: DataSetFirstShred::from(0),
            last_entry_hash,
        });

        SlotStatus::Processing {
            parent_slot,
            verified_up_to: 0.into(),
            last_data_set,
            data_sets: HashMap::new(),
        }
    });

    (Some(slot_status), parent_slot_last_entry_hash)
}

fn get_or_insert_slot_for_non_first_slot_data_set<'slots>(
    blockstore: &Blockstore,
    processing_slots: &'slots mut HashMap<Slot, SlotStatus>,
    slot: Slot,
    data_set_start: DataSetFirstShred,
    slot_last_entry_hash: Option<Hash>,
) -> Option<&'slots mut SlotStatus> {
    match processing_slots.entry(slot) {
        hash_map::Entry::Occupied(slot_status) => Some(slot_status.into_mut()),
        hash_map::Entry::Vacant(empty_slot_status) => {
            let parent_slot = match get_parent_slot(blockstore, slot, data_set_start.into()) {
                Some(slot) => slot,
                None => {
                    return None;
                }
            };

            let last_data_set = slot_last_entry_hash.map(|last_entry_hash| LastSlotDataSet {
                index: data_set_start,
                last_entry_hash,
            });

            let slot_status = empty_slot_status.insert(SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set,
                data_sets: HashMap::new(),
            });

            Some(slot_status)
        }
    }
}

/// Inserts a PoH start hash for the specified data set.  This operation only makes sense if the
/// target data set does not have any data associated with it, or if it is in the
/// `DataSetOptimisticStatus::AllButFirst` state.
fn insert_poh_start_hash(
    todo: &mut Todo,
    slot: Slot,
    slot_status: &mut SlotStatus,
    data_set_idx: DataSetFirstShred,
    poh_start_hash: Hash,
) {
    let data_set = match slot_status {
        SlotStatus::Processing { data_sets, .. } => match data_sets.get_mut(&data_set_idx) {
            None => {
                data_sets.insert(
                    data_set_idx,
                    DataSetOptimisticStatus::PohStartHash(poh_start_hash),
                );
                return;
            }
            Some(data_set) => data_set,
        },
        SlotStatus::Verified { .. } => {
            unexpected_error!(
                "insert_poh_start_hash",
                "slot_already_verified",
                "Target data set is in a slot that is already marked verified",
                "slot: {}, shred: {}",
                slot,
                data_set_idx,
            );
            return;
        }
        SlotStatus::Failed => return,
    };

    match data_set {
        DataSetOptimisticStatus::PohStartHash(_) => {
            unexpected_error!(
                "insert_poh_start_hash",
                "already_has_hash",
                "Target data set already has a starting hash",
                "slot: {}, shred: {}",
                slot,
                data_set_idx,
            );
        }
        DataSetOptimisticStatus::AllButFirst {
            next_data_set,
            first_entry,
        } => {
            let first_entry = first_entry.clone();

            *data_set = DataSetOptimisticStatus::Verified {
                next_data_set: *next_data_set,
            };

            todo.push(TodoItem {
                slot,
                op: TodoOp::OnlyFirstEntryPoh {
                    poh_start_hash,
                    entry: first_entry,
                },
            });
        }
        DataSetOptimisticStatus::Verified { .. } => {
            unexpected_error!(
                "insert_poh_start_hash",
                "already_verified",
                "Target data set is already marked verified",
                "slot: {}, shred: {}",
                slot,
                data_set_idx,
            );
        }
    }
}

/// Inserts a PoH start hash for a data set that follows the one specified.
///
/// When the current data set is the last in the slot, this function will proceed to update all the
/// child slots that we've seen so far.
fn insert_poh_start_hash_into_child_slots(
    todo: &mut Todo,
    processing_slots: &mut HashMap<Slot, SlotStatus>,
    waiting_for_parent_hash: &mut HashMap<Slot, Vec<Slot>>,
    slot: Slot,
    last_entry_hash: Hash,
) {
    let children = match waiting_for_parent_hash.remove(&slot) {
        Some(children) => children,
        None => return,
    };

    for child_slot in children.iter() {
        match processing_slots.entry(*child_slot) {
            hash_map::Entry::Vacant(empty_slot_status) => {
                unexpected_error!(
                    "insert_poh_start_hash_into_child_slots",
                    "vacant_child_slot",
                    "Child slot should always have state, otherwise it must not be in \
                     `waiting_for_parent_hash`",
                    "parent_slot: {}, child_slot: {}",
                    slot,
                    child_slot,
                );
                empty_slot_status.insert(SlotStatus::Processing {
                    parent_slot: slot,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        0.into(),
                        DataSetOptimisticStatus::PohStartHash(last_entry_hash),
                    )]),
                });
            }
            hash_map::Entry::Occupied(slot_status) => insert_poh_start_hash(
                todo,
                *child_slot,
                slot_status.into_mut(),
                0.into(),
                last_entry_hash,
            ),
        }
    }
}

/// Add entries for the specified data set.
///
/// This function does not update the subsequent data set with the PoH starting hash.  Use
/// `insert_poh_start_hash()` for that.
///
/// `parent_slot_last_entry_hash` is only used when `data_set_start.is_first_in_slot()`.
fn insert_data_set_entries(
    todo: &mut Todo,
    waiting_for_parent_hash: &mut HashMap<Slot, Vec<Slot>>,
    data_sets: &mut HashMap<DataSetFirstShred, DataSetOptimisticStatus>,
    parent_slot: Slot,
    parent_slot_last_entry_hash: Option<Hash>,
    slot: Slot,
    data_set_start: DataSetFirstShred,
    entries: Arc<Vec<Entry>>,
    next_data_set_start: DataSetFirstShred,
) {
    match data_sets.entry(data_set_start) {
        hash_map::Entry::Vacant(empty_entry) if data_set_start.is_first_in_slot() => {
            match parent_slot_last_entry_hash {
                None => {
                    waiting_for_parent_hash
                        .entry(parent_slot)
                        .and_modify(|waiting| waiting.push(slot))
                        .or_insert_with(|| vec![slot]);

                    let first_entry = entries.first().unwrap().clone();
                    empty_entry.insert(DataSetOptimisticStatus::AllButFirst {
                        next_data_set: next_data_set_start,
                        first_entry,
                    });

                    todo.push(TodoItem {
                        slot,
                        op: TodoOp::AllButPohOnFirstEntry { entries },
                    });
                }
                Some(poh_start_hash) => {
                    empty_entry.insert(DataSetOptimisticStatus::Verified {
                        next_data_set: next_data_set_start,
                    });

                    todo.push(TodoItem {
                        slot,
                        op: TodoOp::All {
                            poh_start_hash,
                            entries,
                        },
                    });
                }
            }
        }
        hash_map::Entry::Vacant(empty_entry) => {
            // If this is not the first slot data set, then an absent entry means we have not seen
            // the preceding data set yet.

            let first_entry = entries.first().unwrap().clone();
            empty_entry.insert(DataSetOptimisticStatus::AllButFirst {
                next_data_set: next_data_set_start,
                first_entry,
            });

            todo.push(TodoItem {
                slot,
                op: TodoOp::AllButPohOnFirstEntry { entries },
            });
        }
        hash_map::Entry::Occupied(entry) => {
            let poh_start_hash = match entry.get() {
                DataSetOptimisticStatus::PohStartHash(hash) => *hash,
                DataSetOptimisticStatus::AllButFirst { .. }
                | DataSetOptimisticStatus::Verified { .. } => {
                    unexpected_error!(
                        "command_add_data_set",
                        "data_set_already_verified",
                        "data_sets() already contains verification result",
                        "slot: {}, shred: {}",
                        slot,
                        data_set_start,
                    );
                    return;
                }
            };

            *entry.into_mut() = DataSetOptimisticStatus::Verified {
                next_data_set: next_data_set_start,
            };

            todo.push(TodoItem {
                slot,
                op: TodoOp::All {
                    poh_start_hash,
                    entries,
                },
            });
        }
    }
}

fn mark_slot_failed(
    processing_slots: &mut HashMap<Slot, SlotStatus>,
    waiting_for_parent_hash: &mut HashMap<Slot, Vec<Slot>>,
    results: &SharedResults,
    slot: Slot,
) {
    processing_slots
        .entry(slot)
        .and_modify(|status| *status = SlotStatus::Failed)
        .or_insert(SlotStatus::Failed);

    results
        .update_notify_all(|results| results.set_slot_status(slot, SlotVerificationStatus::Failed));

    if let Some(children) = waiting_for_parent_hash.remove(&slot) {
        for child in children.iter() {
            mark_slot_failed(processing_slots, waiting_for_parent_hash, results, *child);
        }
    }
}

/// Looks for the parent slot index for the given `slot`, assuming that `known_shred_index` has been
/// added into the `blockstore`.
///
/// `None` is only returned in case of an error.  If shred with index `known_shred_index` has been
/// inserted into the `blockstore`, it must contain the parent slot index.
fn get_parent_slot(blockstore: &Blockstore, slot: Slot, known_shred_index: u32) -> Option<Slot> {
    let shred = match blockstore.get_data_shred(slot, known_shred_index.into()) {
        Ok(Some(bytes)) => Shred::new_from_serialized_shred(bytes),
        Ok(None) => {
            unexpected_error!(
                "get_parent_slot",
                "get_data_shred__missing",
                "get_data_shred() returned None",
                "slot: {}, shred: {}",
                slot,
                known_shred_index,
            );
            return None;
        }
        Err(err) => {
            unexpected_error!(
                "get_parent_slot",
                "get_data_shred__error",
                "get_data_shred() failed",
                "slot: {}, shred: {}: {:?}",
                slot,
                known_shred_index,
                err,
            );
            return None;
        }
    };

    let shred = match shred {
        Ok(shred) => shred,
        Err(err) => {
            unexpected_error!(
                "get_parent_slot",
                "shred_new_from_serialized",
                "Shred::new_from_serialized_shred() failed",
                "slot: {}, shred: {}: {:?}",
                slot,
                known_shred_index,
                err
            );
            return None;
        }
    };

    match shred.parent() {
        Ok(parent) => Some(parent),
        Err(err) => {
            unexpected_error!(
                "get_parent_slot",
                "shred_parent",
                "Shred::parent() failed",
                "slot: {}, shred: {}: {:?}",
                slot,
                known_shred_index,
                err
            );
            None
        }
    }
}
