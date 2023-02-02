//! [`EntryVerificationService`] is responsible for running different verifications (signature, PoH)
//! on data sets holding [`Entry`]es as they are received from the network.  Before they are
//! assembled into a [`Bank`] by the [`ReplayStage`].
//!
//! The service API is rather simple:
//!
//! - `add_data_set()` to insert a data set holding [`Entry`]es.
//! - `wait_for_result()` to get verification results.
//! - `cleanup()` to discard old results.
//!
//! Most of the complexity comes from two sources:
//!
//! 1. Data sets can be received in arbitrary order.
//!
//!    At the same time, for the PoH hashes, one needs the preceding data set to check the next.
//!    As data sets contain multiple entries, if the preceding data set is absent, it is still
//!    possible to run PoH verification for all by the first entry in the data set.
//!
//!    Later, when the starting hash for the first entry becomes available, the first entry can be
//!    verified.
//!
//!    There is an additional wrinkle, when the data set in question is the first in slot.  In this
//!    case, we need to check the parent slot for the right hash.  And as slots form a tree, it is
//!    not entirely trivial, especially when some data might be missing.
//!
//! 2. As there is a considerable number of data sets, I was trying to optimize all operations to
//!    have the minimum number of lookup and memory accesses. And to use as less memory as possible,
//!    discarding everything that is no longer unnecessary.
//!
//! [`Bank`]: solana_runtime::bank::Bank
//! [`ReplayStage`]: crate::replay_stage::ReplayStage

use {
    self::{
        error::{AddDataSetError, CleanupError, StartError, WaitForResultError},
        shared_wrapper::SharedWrapper,
        worker::Command,
    },
    crossbeam_channel,
    derive_more::{Display, From, Into},
    solana_entry::entry::Entry,
    solana_ledger::blockstore::Blockstore,
    solana_sdk::clock::Slot,
    std::{
        cmp::{Ord, PartialOrd},
        collections::HashMap,
        ops::RangeInclusive,
        result::Result,
        sync::Arc,
        time::Instant,
    },
};

#[macro_use]
pub mod error;
mod shared_wrapper;
mod worker;

/// Service responsible for running PoH and signature verification on [`Entry`]es as they are
/// received from the network.
///
/// [`ReplayStage`] is the main user of this service.  Allowing the verification process to run in
/// parallel, and, hopefully, ahead of the replay process.
///
/// [`ReplayStage`]: crate::replay_stage::ReplayStage
pub struct EntryVerificationService {
    commands: crossbeam_channel::Sender<Command>,
    results: Arc<SharedResults>,
}

impl EntryVerificationService {
    /// Start a worker thread for the verification service, returning an
    /// [`EntryVerificationService`] instance connected to it.
    ///
    /// To shut down the service, just drop the returned value.
    pub fn start(blockstore: Arc<Blockstore>) -> Result<Self, StartError> {
        let results = SharedResults::new(Results::default());

        let commands = worker::start(blockstore, results.clone())?;
        Ok(Self { commands, results })
    }

    /// Provides a new data set, including it in the verification process.
    ///
    /// This method is expected to be called by the [`window_service`], as soon as the next data set
    /// is available.
    ///
    /// [`window_service`]: crate::window_service
    pub fn add_data_set(
        &self,
        slot: Slot,
        shred_indices: RangeInclusive<u32>,
        last_in_slot: bool,
        entries: &[Entry],
    ) -> Result<(), AddDataSetError> {
        self.commands
            .send(Command::AddDataSet {
                slot,
                shred_indices,
                last_in_slot,
                entries: Arc::new(entries.iter().cloned().collect()),
            })
            .map_err(|_| AddDataSetError::WorkerTerminated)
    }

    /// Checks status of a specific `data_set` in a given `slot`, potentially waiting, if the result
    /// is not immediately available.
    ///
    /// This method is expected to be called by the [`ReplayStage`].
    ///
    /// Note that if `slot` state has been already discarded to to a call to `cleanup()`, the
    /// resulting state will be `false`, with no wait.  If one thread is waiting for a result, and
    /// the target `slot` status is discarded via a parallel call to `cleanup()`, the wait will be
    /// interrupted.
    ///
    /// [`ReplayStage`]: crate::replay_stage::ReplayStage
    pub fn wait_for_result(
        &self,
        deadline: Instant,
        slot: Slot,
        data_set: DataSetFirstShred,
    ) -> Result<bool, WaitForResultError> {
        self.commands
            .send(Command::WaitingFor { slot, data_set })
            .map_err(|_| WaitForResultError::WorkerTerminated)?;

        let wait_start = Instant::now();
        match self.results.check_and_wait_until(deadline, |results| {
            if slot < results.starting_at {
                // TODO We may want to record this case as an error.  See `cleanup()` doc comment
                // for an explanation as to why this branch should not be taken during normal
                // validator operation.
                return Some(false);
            }

            match results.slots.get(&slot) {
                None => None,
                Some(SlotVerificationStatus::Failed) => Some(false),
                Some(SlotVerificationStatus::Partial { verified_up_to })
                    if data_set < *verified_up_to =>
                {
                    Some(true)
                }
                Some(SlotVerificationStatus::Partial { .. }) => None,
                Some(SlotVerificationStatus::Valid) => Some(true),
            }
        }) {
            Some(valid) => Ok(valid),
            None => Err(WaitForResultError::Timeout {
                slot,
                data_set,
                waited_for: Instant::now() - wait_start,
            }),
        }
    }

    /// As the verification service is accumulating slot and data sets state, it requires periodic
    /// invocations of which state can be discarded.  It is expected that `cleanup()` will be called
    /// for root slots.  As no slots before that can be used as a parent, nor should their
    /// verification state be useful.
    ///
    /// Any pending or subsequent calls to [`wait_for_result()`] for any slots before the
    /// `earlier_than` slot will return failed verification results immediately.
    ///
    /// There is a race condition between the [`wait_for_result()`] and `cleanup()` invocations.
    /// But normal replay lo logic should not go back to checking state before the last root.  So
    /// during normal operation, this race condition should never be triggered.
    ///
    /// [`wait_for_result()`]: EntryVerificationService::wait_for_result()
    pub fn cleanup(&self, earlier_than: Slot) -> Result<(), CleanupError> {
        self.commands
            .send(Command::Cleanup { earlier_than })
            .map_err(|_| CleanupError::WorkerTerminated)
    }
}

/// We index data sets using index of the first shred that contains them.  This type represents this
/// kind of index - it identifies a data set in a slot.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Display, Clone, Copy, From, Into)]
pub struct DataSetFirstShred(u32);

impl DataSetFirstShred {
    pub fn is_first_in_slot(&self) -> bool {
        self.0 == 0
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum SlotVerificationStatus {
    /// Slot verification failed.
    Failed,
    /// Data sets up to, but not including the specified one have been verified.
    Partial { verified_up_to: DataSetFirstShred },
    /// All data sets in the slot have been verified.
    Valid,
}

/// Results that are shared between the worker and the API of the service.
/// Wrapping it in [`SharedWrapper`] allows the API to wait for the worker, without actively
/// checking for the state to be updated.
#[derive(PartialEq, Eq, Debug, Clone)]
struct Results {
    /// First slot that we are going to process and, possibly, have any results for.
    ///
    /// `EntryVerificationService::cleanup()` shifts this value forward.  And all the other
    /// operations are only valid, when called for this or later slots.
    starting_at: Slot,

    slots: HashMap<Slot, SlotVerificationStatus>,
}

impl Default for Results {
    fn default() -> Self {
        Self {
            starting_at: 0,
            slots: HashMap::new(),
        }
    }
}

impl Results {
    fn set_slot_status(&mut self, slot: Slot, status: SlotVerificationStatus) {
        if slot < self.starting_at {
            return;
        }

        self.slots
            .entry(slot)
            .and_modify(|v| *v = status)
            .or_insert(status);
    }
}

type SharedResults = SharedWrapper<Results>;
