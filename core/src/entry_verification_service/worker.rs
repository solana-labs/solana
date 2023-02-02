//! Execution part of the [`super::EntryVerificationService`].

use {
    super::{DataSetFirstShred, SharedResults, SlotVerificationStatus},
    crossbeam_channel::{self, RecvError, RecvTimeoutError},
    solana_entry::entry::Entry,
    solana_ledger::blockstore::Blockstore,
    solana_sdk::{clock::Slot, hash::Hash},
    std::{
        collections::HashMap,
        io::Error,
        ops::RangeInclusive,
        result::Result,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    },
};

#[cfg(test)]
#[macro_use]
mod test;

mod commands;

/// API of the worker thread.
pub(super) enum Command {
    /// Add new data set into the specified slot.
    AddDataSet {
        slot: Slot,
        shred_indices: RangeInclusive<u32>,
        last_in_slot: bool,
        entries: Arc<Vec<Entry>>,
    },
    /// Indicates that someone is waiting for a validation result for this slot.  This is just a
    /// hint to start the validation ASAP.
    ///
    /// There is a chance for this command delivery to be delayed.  If the `commands` channel has
    /// some unfinished work and `todo` has a lot of accumulated work, then we might not start the
    /// verification work for things that the replay service might need us to verify right now.
    ///
    /// Ideally, this should not happen during normal operation.  And should only happen during very
    /// high load, when there is not enough CPU to process all the requests.  In which case, it
    /// seems fine.  Accumulation delay might trigger processing then.
    ///
    /// We need to add metrics to make sure that neither the `commands` channel, nor the `todo` list
    /// do grow too long under normal operation.
    ///
    /// An alternative design would be to send `WaitingFor` though a separate channel.  But it seems
    /// that if it really matters, then the verification is not working fast enough in the first
    /// place.  Also we do not currently track which pending work is needed for `slot`/`data_set` to
    /// be verified, and which work might potentially be delayed for later.  So it is not as simple
    /// as just adding another channel.
    WaitingFor {
        slot: Slot,
        data_set: DataSetFirstShred,
    },
    /// Remove state for slots before the specified one.
    Cleanup { earlier_than: Slot },
}

pub(super) fn start(
    blockstore: Arc<Blockstore>,
    results: Arc<SharedResults>,
) -> Result<crossbeam_channel::Sender<Command>, Error> {
    let (commands_sender, commands_receiver) = crossbeam_channel::unbounded();

    let state = State {
        commands: commands_receiver,
        blockstore,
        processing: HashMap::new(),
        waiting_for_parent_hash: HashMap::new(),
        client_waiting_for: None,
        results,
    };

    thread::Builder::new()
        .name("solVerifyEntries".to_string())
        .spawn(move || state.run())?;

    Ok(commands_sender)
}

/// Additional information stored about the very last data set in a slot.
#[derive(PartialEq, Eq, Debug, Clone)]
struct LastSlotDataSet {
    /// Index of the first shard holding this data set entries.
    index: DataSetFirstShred,

    /// Hash of the very last entry in the data set.  Used as a starting point for PoH verification
    /// for child slots.
    last_entry_hash: Hash,
}

/// Verification status for a single slot.
#[derive(PartialEq, Eq, Debug, Clone)]
enum SlotStatus {
    /// Some entries int he slot are not fully verified yet.
    Processing {
        /// Parent slot index.  It is used to get a hash for the PoH verification of the very first
        /// [`Entry`] in this slot.
        parent_slot: Slot,

        /// Index of the first data set, starting from the beginning of the slot, that has not been
        /// fully verified yet, with no gaps.  When this value would have exceeded
        /// `last_data_set.index`, slot is considered verified, and the state changes to `Verified`.
        verified_up_to: DataSetFirstShred,

        /// Index and last entry hash for the very last data set in the slot.  We record this
        /// information when we receive the last data set in the slot.
        last_data_set: Option<LastSlotDataSet>,

        /// Holds optimistic status for an individual data set.
        ///
        /// If any of the pending verification fails, the whole slot is marked as [`Failed`].
        ///
        /// Entries are delivered in chunks, called "data sets".  Each data set is contained in
        /// a consecutive range of shreds, that were used to deliver those entries.
        ///
        /// This map only contains entries at or after `verified_up_to`.
        data_sets: HashMap<DataSetFirstShred, DataSetOptimisticStatus>,
    },

    /// All entries in the slot have been successfully verified.  We still need to remember the very
    /// last entry hash, as it is needed for verification of any children of this slot.
    Verified { last_entry_hash: Hash },

    /// Any part of the verification process failed for this slot.
    ///
    /// It is no useful to verify child slots that use this slot as a parent.  As this service API
    /// only support checks for uninterrupted verification.  So we do not store the
    /// `last_entry_hash` here.  And any slot that uses this slot as a parent can safely be marked
    /// `Failed` as well.
    Failed,
}

impl SlotStatus {
    fn last_entry_hash(&self) -> Option<Hash> {
        match self {
            Self::Processing { last_data_set, .. } => last_data_set
                .as_ref()
                .map(|data_set| data_set.last_entry_hash),
            Self::Verified { last_entry_hash } => Some(*last_entry_hash),
            Self::Failed => None,
        }
    }
}

/// Verification status for a data set.
///
/// "Optimistic" here means that the status is set to include all the past successful verification,
/// as well as pretend that all the pending verification work that is been accumulated by the
/// [`accumulate()`] function results in successful verification.  Due to this, there is no state to
/// represent pending verification or a failure.
///
/// We always run both PoH and transaction history verification for a data set, whenever we are
/// processing one.  But for the PoH verification we might be lacking the starting hash.  In which
/// case we can not verify the very first entry.
///
/// An [`Entry`] needs the hash of the preceding entry for PoH verifications.  So in a given data
/// set, all entries can be verified with just the data in the data set, except for the very first
/// one.  Which would require hash of the last entry in the preceding data set.  And the first entry
/// in the slot requires a hash of the very last entry from the parent slot.
///
/// As we receive data sets in arbitrary order, we may not have access to the preceding data set
/// yet.  In which case we can only verify everything, except PoH for the very first entry.
#[derive(PartialEq, Eq, Debug, Clone)]
enum DataSetOptimisticStatus {
    /// This data set does not have entries yet.  But the preceding data set was available and we
    /// record the last entry hash here.
    ///
    /// Storing the starting hash here saves a hash lookup, when we do received data for this data
    /// set.  An alternative would be to look for a preceding data set, at that point.  But, as we
    /// index data sets with the starting shred indices, we do not know what is the starting shred
    /// index for the preceding data set.  Should we use this alternative, we would need an
    /// additional map to track ending points of data sets.
    ///
    /// Other than the need for an extra data set, both variants produce the same amount of hash
    /// lookups.  And as we are storing the starting hash in the same location that will be used for
    /// the data set state, we are, most likely, do not create any additional memory load.
    /// Allocation size is also less than the size of the rest of the enum variants, so so there is
    /// no extra allocation.
    ///
    /// Note that for the very first data set of a slot, we still fetch the hash from the parent
    /// slot.  See [`State::waiting_for_parent_hash`].  In this case we can compute the right
    /// location to check without additional lookups, and as a single parent may have multiple
    /// children.  Combined these arguments made me break consistency between in how this part forks
    /// for the first slot data set vs the rest of them.
    PohStartHash(Hash),

    /// We did not have access to the last hash from the preceding data set, so we could only verify
    /// all but the first entry.
    AllButFirst {
        /// Index of the next data set in the slot.  Necessary when moving from one data set to the
        /// next, when updating [`SlotStatus::verified_up_to`].
        next_data_set: DataSetFirstShred,
        /// Just the first entry, as we have already verified all the rest.
        first_entry: Entry,
    },

    /// All [`Entry`]es have been successfully verified.
    Verified {
        /// Index of the next data set in the slot.  Necessary when moving from one data set to the
        /// next, when updating [`SlotStatus::verified_up_to`].
        next_data_set: DataSetFirstShred,
    },
}

struct State {
    commands: crossbeam_channel::Receiver<Command>,

    blockstore: Arc<Blockstore>,

    processing: HashMap<Slot, SlotStatus>,

    /// Slots that are waiting for the parent slot last entry hash.
    waiting_for_parent_hash: HashMap<Slot, Vec<Slot>>,

    /// When a client is actively waiting for a certain data set that we did not process yet, we are
    /// going to skip accumulation in order to provide better latency.
    ///
    /// We skip accumulation if we see any work for a data set in this slot, for any of the data
    /// sets up to and including the specified one.  We also skip accumulation, if we see any work
    /// for a parent slot.
    ///
    /// As slots are relatively big compared to datasets, it is unlikely that the validation service
    /// will be waiting accumulating data for a grand-parent of the slot the clients are waiting
    /// for.
    client_waiting_for: Option<(Slot, DataSetFirstShred)>,

    results: Arc<SharedResults>,
}

/// Indicates which validation are we planning to do for a data set.
///
/// As we are going to run verification for PoH and for signature verification in different threads,
/// and, potentially, in parallel, holding `entries` as an `Arc<Vec<Entry>>` saves an extra copy.
#[derive(PartialEq, Eq, Debug, Clone)]
enum TodoOp {
    /// Verify PoH for entries, and signature verification for all transactions.
    All {
        poh_start_hash: Hash,
        entries: Arc<Vec<Entry>>,
    },
    /// PoH for all but the first entry, and signature verification for all transactions.
    AllButPohOnFirstEntry { entries: Arc<Vec<Entry>> },
    /// PoH for the first entry, only.
    OnlyFirstEntryPoh { poh_start_hash: Hash, entry: Entry },
}

#[derive(PartialEq, Eq, Debug, Clone)]
struct TodoItem {
    /// Slot to mark as failed if the verification specified in `op` fails.
    slot: Slot,
    /// Pending verification operation.
    op: TodoOp,
}

type Todo = Vec<TodoItem>;

impl State {
    fn run(mut self) {
        loop {
            let todo = match self.accumulate_work() {
                Some(todo) => todo,
                None => return,
            };

            self.verify(todo);
        }
    }

    fn accumulate_work(&mut self) -> Option<Todo> {
        let Self {
            commands,
            blockstore,
            processing,
            waiting_for_parent_hash,
            client_waiting_for,
            results,
        } = self;

        // We accumulate verification requests for some time before actually running them.  As we
        // combine multiple requests together, it makes sense to wait a bit, in case more raw data
        // will be available in the near future.
        const ACCUMULATION_DELAY: Duration = Duration::from_millis(10);
        let mut todo = vec![];

        let mut deadline = None;

        loop {
            let command = if todo.is_empty() {
                commands
                    .recv()
                    .map_err(|_err: RecvError| RecvTimeoutError::Disconnected)
            } else {
                let deadline = deadline.expect("Deadline is set when we have any work pending");
                commands.recv_deadline(deadline)
            };

            let command = match command {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => return Some(todo),
                Err(RecvTimeoutError::Disconnected) => return None,
            };

            match command {
                Command::AddDataSet {
                    slot,
                    shred_indices,
                    last_in_slot,
                    entries,
                } => {
                    if deadline.is_none() {
                        deadline = Some(Instant::now() + ACCUMULATION_DELAY);
                    }

                    let data_set_start = DataSetFirstShred::from(*shred_indices.start());

                    let parent_slot = commands::add_data_set(
                        &mut todo,
                        blockstore,
                        processing,
                        waiting_for_parent_hash,
                        results,
                        slot,
                        shred_indices,
                        last_in_slot,
                        entries,
                    );

                    // If the client is waiting on the slot and data set that we have work for,
                    // stop the work accumulation.
                    //
                    // Same for any work in the last data set of the parent slot, as the first child
                    // slot is blocked by the parent slot work.
                    if let Some((waiting_slot, waiting_data_set_start)) = client_waiting_for {
                        let last_data_set_in_parent = parent_slot
                            .map(|parent_slot| last_in_slot && parent_slot == *waiting_slot)
                            .unwrap_or(false);

                        if last_data_set_in_parent
                            || (slot == *waiting_slot && data_set_start <= *waiting_data_set_start)
                        {
                            return Some(todo);
                        }
                    }
                }
                Command::WaitingFor { slot, data_set } => {
                    match commands::waiting_for(&todo, client_waiting_for, slot, data_set) {
                        commands::AccumulationDecision::RunVerification => return Some(todo),
                        commands::AccumulationDecision::CanWaitMore => (),
                    }
                }
                Command::Cleanup { earlier_than } => commands::cleanup(
                    processing,
                    waiting_for_parent_hash,
                    client_waiting_for,
                    results,
                    earlier_than,
                ),
            }
        }
    }

    fn verify(&mut self, todo: Todo) {
        let Self {
            processing,
            results,
            ..
        } = self;

        // As we do not expect to process more than 1 or 2 slots at a time, we use a vector, rather
        // than a set to store slot indices.
        let mut verified = vec![];
        for TodoItem { slot, .. } in todo.iter() {
            if !verified.contains(slot) {
                verified.push(*slot);
            }
        }

        // TODO Send request to the GPU thread.

        // TODO Run signature verification.

        let failed = vec![];
        // TODO Check results and mark any failed slots.

        for slot in failed.iter() {
            if let Some(i) = verified.iter().position(|v| *v == *slot) {
                verified.swap_remove(i);
            }
        }

        let updated = advance_verified(processing, verified);

        update_results(processing, results, updated, failed);
    }
}

fn advance_verified(
    processing_slots: &mut HashMap<Slot, SlotStatus>,
    verified: Vec<Slot>,
) -> Vec<Slot> {
    let mut updated = vec![];

    for slot in verified.into_iter() {
        let mut advanced = false;

        let slot_status = match processing_slots.get_mut(&slot) {
            Some(slot_status) => slot_status,
            None => {
                unexpected_error!(
                    "advance_verified",
                    "slot_state_absent",
                    "Target slot does not have any state",
                    "slot: {}",
                    slot
                );
                continue;
            }
        };

        match slot_status {
            SlotStatus::Verified { .. } => unexpected_error!(
                "advance_verified",
                "slot_state_verified",
                "Target slot is already marked `Verified`",
                "slot: {}",
                slot
            ),
            SlotStatus::Failed { .. } => unexpected_error!(
                "advance_verified",
                "slot_state_failed",
                "Target slot is already marked `Failed`",
                "slot: {}",
                slot
            ),
            SlotStatus::Processing {
                verified_up_to,
                data_sets,
                last_data_set,
                ..
            } => {
                let mut current = *verified_up_to;
                loop {
                    let data_set = match data_sets.get(&current) {
                        Some(data_set) => data_set,
                        None => break,
                    };

                    let next_data_set = match data_set {
                        DataSetOptimisticStatus::Verified { next_data_set } => *next_data_set,
                        _ => break,
                    };

                    *verified_up_to = next_data_set;
                    advanced = true;

                    if let Some(last_data_set) = last_data_set {
                        if current == last_data_set.index {
                            *slot_status = SlotStatus::Verified {
                                last_entry_hash: last_data_set.last_entry_hash,
                            };
                            break;
                        }
                    }

                    current = next_data_set;
                }
            }
        }

        if advanced {
            updated.push(slot);
        }
    }

    updated
}

fn update_results(
    processing_slots: &mut HashMap<Slot, SlotStatus>,
    results: &SharedResults,
    updated: Vec<Slot>,
    failed: Vec<Slot>,
) {
    // Avoid triggering the waiting thread(s), if there are really no updates.
    if updated.is_empty() && failed.is_empty() {
        return;
    }

    for slot in failed.iter() {
        processing_slots
            .entry(*slot)
            .and_modify(|slot_status| *slot_status = SlotStatus::Failed);
    }

    results.update_notify_all(|results| {
        for slot in updated.into_iter().chain(failed.into_iter()) {
            let slot_status = match processing_slots.get(&slot) {
                Some(slot_status) => slot_status,
                None => {
                    unexpected_error!(
                        "update_results",
                        "slot_state_absent",
                        "Target slot does not have any state",
                        "slot: {}",
                        slot
                    );
                    continue;
                }
            };

            let result = match slot_status {
                SlotStatus::Processing { verified_up_to, .. } => SlotVerificationStatus::Partial {
                    verified_up_to: *verified_up_to,
                },
                SlotStatus::Verified { .. } => SlotVerificationStatus::Valid,
                SlotStatus::Failed { .. } => SlotVerificationStatus::Failed,
            };

            results
                .slots
                .entry(slot)
                .and_modify(|slot_result| *slot_result = result);
        }
    });
}
