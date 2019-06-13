use crate::entry::Entry;
use crate::poh_recorder::WorkingBankEntries;
use crate::result::Result;
use solana_runtime::bank::Bank;
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
