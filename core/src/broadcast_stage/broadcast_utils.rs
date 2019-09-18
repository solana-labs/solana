use crate::entry::Entry;
use crate::poh_recorder::WorkingBankEntry;
use crate::result::Result;
use crate::shred::{Shred, ShredInfo, Shredder, RECOMMENDED_FEC_RATE};
use solana_runtime::bank::Bank;
use solana_sdk::signature::Keypair;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct ReceiveResults {
    pub entries: Vec<Entry>,
    pub time_elapsed: Duration,
    pub bank: Arc<Bank>,
    pub last_tick: u64,
}

pub(super) fn recv_slot_entries(receiver: &Receiver<WorkingBankEntry>) -> Result<ReceiveResults> {
    let timer = Duration::new(1, 0);
    let (bank, (entry, mut last_tick)) = receiver.recv_timeout(timer)?;
    let recv_start = Instant::now();

    let mut entries = vec![entry];
    let mut slot = bank.slot();
    let mut max_tick_height = bank.max_tick_height();

    assert!(last_tick <= max_tick_height);

    if last_tick != max_tick_height {
        while let Ok((bank, (entry, tick_height))) = receiver.try_recv() {
            // If the bank changed, that implies the previous slot was interrupted and we do not have to
            // broadcast its entries.
            if bank.slot() != slot {
                entries.clear();
                slot = bank.slot();
                max_tick_height = bank.max_tick_height();
            }
            last_tick = tick_height;
            entries.push(entry);

            assert!(last_tick <= max_tick_height);
            if last_tick == max_tick_height {
                break;
            }
        }
    }

    let time_elapsed = recv_start.elapsed();
    Ok(ReceiveResults {
        entries,
        time_elapsed,
        bank,
        last_tick,
    })
}

pub(super) fn entries_to_shreds(
    entries: Vec<Entry>,
    last_tick: u64,
    slot: u64,
    bank_max_tick: u64,
    keypair: &Arc<Keypair>,
    latest_shred_index: u64,
    parent_slot: u64,
) -> (Vec<Shred>, Vec<ShredInfo>, u64) {
    let mut shredder = Shredder::new(
        slot,
        parent_slot,
        RECOMMENDED_FEC_RATE,
        keypair,
        latest_shred_index as u32,
    )
    .expect("Expected to create a new shredder");

    bincode::serialize_into(&mut shredder, &entries)
        .expect("Expect to write all entries to shreds");

    if last_tick == bank_max_tick {
        shredder.finalize_slot();
    } else {
        shredder.finalize_data();
    }

    let (shreds, shred_infos): (Vec<Shred>, Vec<ShredInfo>) =
        shredder.shred_tuples.into_iter().unzip();

    trace!("Inserting {:?} shreds in blocktree", shreds.len());

    (shreds, shred_infos, u64::from(shredder.index))
}
