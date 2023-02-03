//! [`CompletedDataSetsService`] is a hub, that runs different operations when a "completed data
//! set", also known as a [`Vec<Entry>`], is received by the validator.
//!
//! Currently, `WindowService` sends [`CompletedDataSetInfo`]s via a `completed_sets_receiver`
//! provided to the [`CompletedDataSetsService`].

use {
    self::{max_slot::MaxSlot, notify_rpc_subscriptions::NotifyRpcSubscriptions},
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    solana_entry::entry::Entry,
    solana_ledger::blockstore::{Blockstore, CompletedDataSetInfo},
    solana_rpc::{max_slots::MaxSlots, rpc_subscriptions::RpcSubscriptions},
    solana_sdk::clock::Slot,
    std::{
        iter::once,
        ops::RangeInclusive,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub mod max_slot;
pub mod notify_rpc_subscriptions;

pub type CompletedDataSetsReceiver = Receiver<Vec<CompletedDataSetInfo>>;
pub type CompletedDataSetsSender = Sender<Vec<CompletedDataSetInfo>>;

/// Holds a [`Vec<Entry>`] deserialized from a given [`CompletedDataSetInfo`].
pub struct SlotEntries<'entries> {
    pub slot: Slot,
    /// Shards in `slot` that represent `entries`.
    pub shard_indices: RangeInclusive<u32>,
    pub last_in_slot: bool,
    pub entries: &'entries [Entry],
}

pub type CompletedDataSetHandler = Box<dyn Fn(SlotEntries) + Send>;

pub struct CompletedDataSetsService {
    thread_hdl: JoinHandle<()>,
}

impl CompletedDataSetsService {
    /// Starts a thread that is running a `CompletedDataSetsService` instance, sending received
    /// [`Entry`]es into the provided set of `handlers`.
    pub fn run(
        exit: Arc<AtomicBool>,
        receiver: CompletedDataSetsReceiver,
        blockstore: Arc<Blockstore>,
        handlers: Vec<CompletedDataSetHandler>,
    ) -> Self {
        let process = move || loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            match forward_completed(&receiver, &blockstore, &handlers) {
                Ok(()) => (),
                Err(_) => break,
            }
        };

        let thread_hdl = Builder::new()
            .name("solComplDataSet".to_string())
            .spawn(process)
            .unwrap();
        Self { thread_hdl }
    }

    /// Constructs a list with all handlers defined in submodules of this module.
    pub fn construct_handlers(
        rpc_subscriptions: Arc<RpcSubscriptions>,
        max_slots: Arc<MaxSlots>,
    ) -> Vec<CompletedDataSetHandler> {
        vec![
            NotifyRpcSubscriptions::handler(rpc_subscriptions),
            MaxSlot::handler(max_slots),
        ]
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

fn forward_completed(
    receiver: &CompletedDataSetsReceiver,
    blockstore: &Blockstore,
    handlers: &[CompletedDataSetHandler],
) -> Result<(), RecvTimeoutError> {
    for CompletedDataSetInfo {
        slot,
        start_index,
        end_index,
        last_in_slot,
    } in once(receiver.recv_timeout(Duration::from_secs(1))?)
        .chain(receiver.try_iter())
        .flatten()
    {
        match blockstore.get_entries_in_data_block(slot, start_index, end_index, None) {
            Ok(entries) if !entries.is_empty() => {
                for handler in handlers.iter() {
                    handler(SlotEntries {
                        slot,
                        shard_indices: start_index..=end_index,
                        last_in_slot,
                        entries: &entries,
                    });
                }
            }
            Ok(_) => (),
            Err(err) => {
                warn!(
                    "completed-data-set-service deserialize error:\n\
                     slot: {slot}, shreds: {start_index}..={end_index}\n\
                     {err:?}",
                );
                datapoint_error!(
                    "completed_data_set_service_deserialize_error",
                    (
                        "error",
                        format!("slot: {slot}, shreds: {start_index}..={end_index}: {err:?}"),
                        String
                    )
                );
            }
        }
    }

    Ok(())
}
