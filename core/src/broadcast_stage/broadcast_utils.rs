use crate::entry::Entry;
use crate::poh_recorder::WorkingBankEntries;
use crate::result::Result;
use crate::shred::{Shred, Shredder};
use solana_runtime::bank::Bank;
use solana_sdk::signature::Keypair;
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

pub(super) fn recv_slot_shreds(receiver: &Receiver<WorkingBankEntries>) -> Result<ReceiveResults> {
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

pub(super) fn entries_to_shreds(
    ventries: Vec<Vec<(Entry, u64)>>,
    slot: u64,
    last_tick: u64,
    bank_max_tick: u64,
    keypair: &Arc<Keypair>,
    mut latest_shred_index: u64,
    parent_slot: u64,
) -> (Vec<Shred>, Vec<Vec<u8>>, u64) {
    let mut all_shred_bufs = vec![];
    let mut all_shreds = vec![];
    let num_ventries = ventries.len();
    ventries
        .into_iter()
        .enumerate()
        .for_each(|(i, entries_tuple)| {
            let (entries, _): (Vec<_>, Vec<_>) = entries_tuple.into_iter().unzip();
            //entries
            let mut shredder =
                Shredder::new(slot, parent_slot, 1.0, keypair, latest_shred_index as u32)
                    .expect("Expected to create a new shredder");

            bincode::serialize_into(&mut shredder, &entries)
                .expect("Expect to write all entries to shreds");

            if i == (num_ventries - 1) && last_tick == bank_max_tick {
                shredder.finalize_slot();
            } else {
                shredder.finalize_data();
            }

            let mut shreds: Vec<Shred> = shredder
                .shreds
                .iter()
                .map(|s| bincode::deserialize(s).unwrap())
                .collect();

            trace!("Inserting {:?} shreds in blocktree", shreds.len());
            latest_shred_index = u64::from(shredder.index);
            all_shreds.append(&mut shreds);
            all_shred_bufs.append(&mut shredder.shreds);
        });
    (all_shreds, all_shred_bufs, latest_shred_index)
}
