use crate::{result::Result, window_service::DuplicateSlotSender};
use crossbeam_channel::{unbounded, Receiver as CrossbeamReceiver, Sender as CrossbeamSender};
use log::*;
use solana_gossip::{
    cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS},
    crds::Cursor,
    crds_value::CrdsData,
    duplicate_shred::{self, DuplicateShred},
};
use solana_ledger::blockstore::Blockstore;
use solana_ledger::leader_schedule_utils::slot_leader_at;
use solana_metrics::inc_new_counter_debug;
use solana_poh::poh_recorder::PohRecorder;
use solana_sdk::{clock::Slot, pubkey::Pubkey};
use std::{
    cmp::Ordering::{Equal, Greater},
    collections::{hash_map::Entry, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        {Arc, Mutex},
    },
    thread::{self, sleep, Builder, JoinHandle},
    time::Duration,
};

pub type DuplicateShredSender = CrossbeamSender<Pubkey>;
pub type DuplicateShredReceiver = CrossbeamReceiver<Pubkey>;

pub struct ClusterInfoEntriesListener {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ClusterInfoEntriesListener {
    pub fn new(
        exit: &Arc<AtomicBool>,
        cluster_info: Arc<ClusterInfo>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        blockstore: Arc<Blockstore>,
        duplicate_slot_sender: DuplicateSlotSender,
    ) -> Self {
        let (duplicate_shred_sender, duplicate_shred_receiver) = unbounded();
        let exit_ = exit.clone();
        let cluster_info_ = cluster_info.clone();

        let listen_thread = Builder::new()
            .name("solana-cluster_info_entries_listener".to_string())
            .spawn(move || {
                let _ = Self::recv_loop(exit_, &cluster_info_, duplicate_shred_sender);
            })
            .unwrap();

        let exit_ = exit.clone();
        let poh_recorder = poh_recorder.clone();
        let duplicate_shreds_thread = Builder::new()
            .name("solana-cluster_info_process_duplicate_shreds".to_string())
            .spawn(move || {
                let _ = Self::process_duplicate_shreds_loop(
                    exit_,
                    &cluster_info,
                    duplicate_shred_receiver,
                    poh_recorder,
                    blockstore,
                    duplicate_slot_sender,
                );
            })
            .unwrap();

        Self {
            thread_hdls: vec![listen_thread, duplicate_shreds_thread],
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }

    fn recv_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &ClusterInfo,
        duplicate_shred_sender: DuplicateShredSender,
    ) -> Result<()> {
        let mut cursor = Cursor::default();
        while !exit.load(Ordering::Relaxed) {
            let entries = cluster_info.get_entries(&mut cursor);
            inc_new_counter_debug!("cluster_info_entries_listener-recv_count", entries.len());
            if !entries.is_empty() {
                entries.iter().for_each(|value| {
                    if let CrdsData::DuplicateShred(_, _) = value.data {
                        if let Err(e) = duplicate_shred_sender.send(value.pubkey()) {
                            error!("Couldn't send duplicate shred received from {} to process loop: {}", value.pubkey(), e);
                        }
                    }
                });
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
        Ok(())
    }

    fn process_duplicate_shreds_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &ClusterInfo,
        duplicate_shred_receiver: DuplicateShredReceiver,
        poh_recorder: Arc<Mutex<PohRecorder>>,
        blockstore: Arc<Blockstore>,
        duplicate_slot_sender: DuplicateSlotSender,
    ) -> Result<()> {
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let pubkey = match duplicate_shred_receiver.recv() {
                Ok(p) => p,
                Err(_) => continue,
            };

            duplicate_shred_receiver
                .try_iter()
                .chain(vec![pubkey].into_iter())
                .for_each(|pubkey| {
                    // Keep a map in case multiple gossip proofs come at once
                    let mut chunks_per_slot: HashMap::<Slot, (Vec<DuplicateShred>, usize)> = HashMap::new();
                    cluster_info
                        .get_duplicate_shreds_from(&pubkey)
                        .filter(|chunk| blockstore.get_duplicate_slot(chunk.slot).is_none()) // Filter out slots we already know are duplicate
                        .for_each(|chunk| match chunks_per_slot.entry(chunk.slot) {
                            Entry::Vacant(entry) => {
                                let mut chunks = Vec::new();
                                let num_chunks = chunk.num_chunks.into();
                                chunks.push(chunk);
                                entry.insert((chunks, num_chunks));
                            }
                            Entry::Occupied(mut entry) => {
                                let (chunks, _) = entry.get_mut();
                                chunks.push(chunk);
                            }
                        });

                    chunks_per_slot.into_iter().for_each(|(slot, (chunks, num_chunks)): (Slot, (Vec<DuplicateShred>, usize))| {
                        let leader = |slot : Slot| {
                            poh_recorder.lock().unwrap().bank().as_ref().map(|bank| slot_leader_at(slot, bank)).flatten()
                        };
                        // See if we have all of the shreds needed for the proof yet
                        match chunks.len().cmp(&num_chunks) {
                            Equal => match duplicate_shred::into_shreds(chunks.into_iter(), leader) {
                                Ok((shred1, shred2)) => {
                                    if let Err(e) = blockstore.store_duplicate_slot(
                                        slot,
                                        shred1.payload,
                                        shred2.payload,
                                    ) {
                                        error!("Unable to store duplicate slot {} in blockstore: {}", slot, e)
                                    }
                                    if let Err(e) = duplicate_slot_sender.send(slot) {
                                        error!("Unable to notify replay stage of duplicate slot {} from gossip proof: {}", slot, e)
                                    }
                                }
                                Err(e) => {
                                    warn!("Unable to ingest duplicate slot proof for {}: {}", slot, e)
                                }
                            },
                            Greater => error!("Duplicate slot proof for {} is corrupt, expected {} chunks but got {} chunks", slot, num_chunks, chunks.len()),
                            _ => (),
                        }
                    });
                });
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }
}
