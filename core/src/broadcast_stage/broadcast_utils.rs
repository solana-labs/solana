use crate::entry::Entry;
use crate::entry::EntrySlice;
use crate::erasure::CodingGenerator;
use crate::packet::{self, SharedBlob};
use crate::poh_recorder::WorkingBankEntries;
use crate::result::Result;
use rayon::prelude::*;
use rayon::ThreadPool;
use solana_runtime::bank::Bank;
use solana_sdk::signature::{Keypair, KeypairUtil, Signable};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct ReceiveResults {
    pub ventries: Vec<Vec<(Entry, u64)>>,
    pub num_entries: usize,
    pub time_elapsed: Duration,
    pub bank: Arc<Bank>,
    pub last_tick: u64,
}

impl ReceiveResults {
    pub fn new(
        ventries: Vec<Vec<(Entry, u64)>>,
        num_entries: usize,
        time_elapsed: Duration,
        bank: Arc<Bank>,
        last_tick: u64,
    ) -> Self {
        Self {
            ventries,
            num_entries,
            time_elapsed,
            bank,
            last_tick,
        }
    }
}

pub(super) fn recv_slot_blobs(receiver: &Receiver<WorkingBankEntries>) -> Result<ReceiveResults> {
    let timer = Duration::new(1, 0);
    let (mut bank, entries) = receiver.recv_timeout(timer)?;
    let recv_start = Instant::now();
    let mut max_tick_height = bank.max_tick_height();
    let mut num_entries = entries.len();
    let mut ventries = Vec::new();
    let mut last_tick = entries.last().map(|v| v.1).unwrap_or(0);
    ventries.push(entries);

    assert!(last_tick <= max_tick_height);
    if last_tick != max_tick_height {
        while let Ok((same_bank, entries)) = receiver.try_recv() {
            // If the bank changed, that implies the previous slot was interrupted and we do not have to
            // broadcast its entries.
            if same_bank.slot() != bank.slot() {
                num_entries = 0;
                ventries.clear();
                bank = same_bank.clone();
                max_tick_height = bank.max_tick_height();
            }
            num_entries += entries.len();
            last_tick = entries.last().map(|v| v.1).unwrap_or(0);
            ventries.push(entries);
            assert!(last_tick <= max_tick_height,);
            if last_tick == max_tick_height {
                break;
            }
        }
    }

    let recv_end = recv_start.elapsed();
    let receive_results = ReceiveResults::new(ventries, num_entries, recv_end, bank, last_tick);
    Ok(receive_results)
}

pub(super) fn entries_to_blobs(
    ventries: Vec<Vec<(Entry, u64)>>,
    thread_pool: &ThreadPool,
    latest_blob_index: u64,
    last_tick: u64,
    bank: &Bank,
    keypair: &Keypair,
    coding_generator: &mut CodingGenerator,
) -> (Vec<SharedBlob>, Vec<SharedBlob>) {
    let blobs = generate_data_blobs(
        ventries,
        thread_pool,
        latest_blob_index,
        last_tick,
        &bank,
        &keypair,
    );

    let coding = generate_coding_blobs(&blobs, &thread_pool, coding_generator, &keypair);

    (blobs, coding)
}

pub(super) fn generate_data_blobs(
    ventries: Vec<Vec<(Entry, u64)>>,
    thread_pool: &ThreadPool,
    latest_blob_index: u64,
    last_tick: u64,
    bank: &Bank,
    keypair: &Keypair,
) -> Vec<SharedBlob> {
    let blobs: Vec<SharedBlob> = thread_pool.install(|| {
        ventries
            .into_par_iter()
            .map(|p| {
                let entries: Vec<_> = p.into_iter().map(|e| e.0).collect();
                entries.to_shared_blobs()
            })
            .flatten()
            .collect()
    });

    packet::index_blobs(
        &blobs,
        &keypair.pubkey(),
        latest_blob_index,
        bank.slot(),
        bank.parent().map_or(0, |parent| parent.slot()),
    );

    if last_tick == bank.max_tick_height() {
        blobs.last().unwrap().write().unwrap().set_is_last_in_slot();
    }

    // Make sure not to modify the blob header or data after signing it here
    thread_pool.install(|| {
        blobs.par_iter().for_each(|b| {
            b.write().unwrap().sign(keypair);
        })
    });

    blobs
}

pub(super) fn generate_coding_blobs(
    blobs: &[SharedBlob],
    thread_pool: &ThreadPool,
    coding_generator: &mut CodingGenerator,
    keypair: &Keypair,
) -> Vec<SharedBlob> {
    let coding = coding_generator.next(&blobs);

    thread_pool.install(|| {
        coding.par_iter().for_each(|c| {
            c.write().unwrap().sign(keypair);
        })
    });

    coding
}
