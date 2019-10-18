use crate::poh_recorder::WorkingBankEntry;
use crate::result::Result;
use solana_ledger::entry::Entry;
use solana_runtime::bank::Bank;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct ReceiveResults {
    pub entries: Vec<Entry>,
    pub time_elapsed: Duration,
    pub bank: Arc<Bank>,
    pub last_tick_height: u64,
}

#[derive(Copy, Clone)]
pub struct UnfinishedSlotInfo {
    pub next_shred_index: u32,
    pub slot: u64,
    pub parent: u64,
}

/// This parameter tunes how many entries are received in one iteration of recv loop
/// This will prevent broadcast stage from consuming more entries, that could have led
/// to delays in shredding, and broadcasting shreds to peer validators
const RECEIVE_ENTRY_COUNT_THRESHOLD: usize = 8;

pub(super) fn recv_slot_entries(receiver: &Receiver<WorkingBankEntry>) -> Result<ReceiveResults> {
    let timer = Duration::new(1, 0);
    let recv_start = Instant::now();
    let (mut bank, (entry, mut last_tick_height)) = receiver.recv_timeout(timer)?;

    let mut entries = vec![entry];
    let mut slot = bank.slot();
    let mut max_tick_height = bank.max_tick_height();

    assert!(last_tick_height <= max_tick_height);

    if last_tick_height != max_tick_height {
        while let Ok((try_bank, (entry, tick_height))) = receiver.try_recv() {
            // If the bank changed, that implies the previous slot was interrupted and we do not have to
            // broadcast its entries.
            if try_bank.slot() != slot {
                warn!("Broadcast for slot: {} interrupted", bank.slot());
                entries.clear();
                bank = try_bank;
                slot = bank.slot();
                max_tick_height = bank.max_tick_height();
            }
            last_tick_height = tick_height;
            entries.push(entry);

            if entries.len() >= RECEIVE_ENTRY_COUNT_THRESHOLD {
                break;
            }

            assert!(last_tick_height <= max_tick_height);
            if last_tick_height == max_tick_height {
                break;
            }
        }
    }

    let time_elapsed = recv_start.elapsed();
    Ok(ReceiveResults {
        entries,
        time_elapsed,
        bank,
        last_tick_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::system_transaction;
    use solana_sdk::transaction::Transaction;
    use std::sync::mpsc::channel;

    fn setup_test() -> (GenesisBlock, Arc<Bank>, Transaction) {
        let GenesisBlockInfo {
            genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(2);
        let bank0 = Arc::new(Bank::new(&genesis_block));
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &Pubkey::new_rand(),
            1,
            genesis_block.hash(),
        );

        (genesis_block, bank0, tx)
    }

    #[test]
    fn test_recv_slot_entries_1() {
        let (genesis_block, bank0, tx) = setup_test();

        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        let (s, r) = channel();
        let mut last_hash = genesis_block.hash();

        assert!(bank1.max_tick_height() > 1);
        let entries: Vec<_> = (1..bank1.max_tick_height() + 1)
            .map(|i| {
                let entry = Entry::new(&last_hash, 1, vec![tx.clone()]);
                last_hash = entry.hash;
                s.send((bank1.clone(), (entry.clone(), i))).unwrap();
                entry
            })
            .collect();

        let result = recv_slot_entries(&r).unwrap();

        assert_eq!(result.bank.slot(), bank1.slot());
        assert_eq!(result.last_tick_height, bank1.max_tick_height());
        assert_eq!(result.entries, entries);
    }

    #[test]
    fn test_recv_slot_entries_2() {
        let (genesis_block, bank0, tx) = setup_test();

        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &Pubkey::default(), 2));
        let (s, r) = channel();

        let mut last_hash = genesis_block.hash();
        assert!(bank1.max_tick_height() > 1);
        // Simulate slot 2 interrupting slot 1's transmission
        let expected_last_height = bank1.max_tick_height();
        let last_entry = (1..=bank1.max_tick_height())
            .map(|tick_height| {
                let entry = Entry::new(&last_hash, 1, vec![tx.clone()]);
                last_hash = entry.hash;
                // Interrupt slot 1 right before the last tick
                if tick_height == expected_last_height {
                    s.send((bank2.clone(), (entry.clone(), tick_height)))
                        .unwrap();
                    Some(entry)
                } else {
                    s.send((bank1.clone(), (entry, tick_height))).unwrap();
                    None
                }
            })
            .last()
            .unwrap()
            .unwrap();

        let result = recv_slot_entries(&r).unwrap();
        assert_eq!(result.bank.slot(), bank2.slot());
        assert_eq!(result.last_tick_height, expected_last_height);
        assert_eq!(result.entries, vec![last_entry]);
    }
}
