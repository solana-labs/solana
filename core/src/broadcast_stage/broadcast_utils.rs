use crate::entry::Entry;
use crate::poh_recorder::WorkingBankEntry;
use crate::result::Result;
use crate::shred::{Shred, Shredder, RECOMMENDED_FEC_RATE};
use core::cell::RefCell;
use rayon::prelude::*;
use rayon::ThreadPool;
use solana_rayon_threadlimit::get_thread_count;
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

#[derive(Copy, Clone)]
pub struct UnfinishedSlotInfo {
    pub next_index: u64,
    pub slot: u64,
    pub parent: u64,
}

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .build()
                    .unwrap()));

/// Theis parameter tunes how many entries are received in one iteration of recv loop
/// This will prevent broadcast stage from consuming more entries, that could have led
/// to delays in shredding, and broadcasting shreds to peer validators
const RECEIVE_ENTRY_COUNT_THRESHOLD: usize = 8;

pub(super) fn recv_slot_entries(receiver: &Receiver<WorkingBankEntry>) -> Result<ReceiveResults> {
    let timer = Duration::new(1, 0);
    let (mut bank, (entry, mut last_tick)) = receiver.recv_timeout(timer)?;
    let recv_start = Instant::now();

    let mut entries = vec![entry];
    let mut slot = bank.slot();
    let mut max_tick_height = bank.max_tick_height();

    assert!(last_tick <= max_tick_height);

    if last_tick != max_tick_height {
        while let Ok((try_bank, (entry, tick_height))) = receiver.try_recv() {
            // If the bank changed, that implies the previous slot was interrupted and we do not have to
            // broadcast its entries.
            if try_bank.slot() != slot {
                entries.clear();
                bank = try_bank;
                slot = bank.slot();
                max_tick_height = bank.max_tick_height();
            }
            last_tick = tick_height;
            entries.push(entry);

            if entries.len() >= RECEIVE_ENTRY_COUNT_THRESHOLD {
                break;
            }

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
    mut latest_shred_index: u64,
    parent_slot: u64,
    last_unfinished_slot: Option<UnfinishedSlotInfo>,
) -> (Vec<Shred>, Option<UnfinishedSlotInfo>) {
    let mut past_shreds = if let Some(unfinished_slot) = last_unfinished_slot {
        if unfinished_slot.slot != slot {
            let mut shredder = Shredder::new(
                unfinished_slot.slot,
                unfinished_slot.parent,
                RECOMMENDED_FEC_RATE,
                keypair,
                unfinished_slot.next_index as u32,
            )
            .expect("Expected to create a new shredder");
            shredder.finalize_slot();
            shredder.shreds.drain(..).collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    // Shred entries in parallel, with index starting at 0 for each entry
    let shredders: Vec<Shredder> = PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            entries
                .par_iter()
                .map(|e| {
                    let mut shredder =
                        Shredder::new(slot, parent_slot, RECOMMENDED_FEC_RATE, keypair, 0)
                            .expect("Expected to create a new shredder");

                    bincode::serialize_into(&mut shredder, &e)
                        .expect("Expect to write all entries to shreds");

                    shredder.finalize_data();
                    shredder
                })
                .collect()
        })
    });

    let mut shredders_and_new_base: Vec<(u64, Shredder)> = shredders
        .into_iter()
        .map(|s| {
            let base = latest_shred_index;
            latest_shred_index += s.num_data_shreds as u64;
            (base, s)
        })
        .collect();

    let unfinished_slot = if last_tick == bank_max_tick {
        shredders_and_new_base.last_mut().unwrap().1.finalize_slot();
        None
    } else {
        let last_shredder = shredders_and_new_base.last().unwrap();
        let last_index = last_shredder.0 + last_shredder.1.num_data_shreds as u64;
        Some(UnfinishedSlotInfo {
            next_index: last_index + 1,
            slot,
            parent: parent_slot,
        })
    };

    // Rebase entries in shredder in parallel
    let mut shreds: Vec<Shred> = PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            shredders_and_new_base
                .into_par_iter()
                .flat_map(|(b, mut s)| {
                    s.shreds
                        .iter_mut()
                        .for_each(|shred| shred.rebase_index(b as u32));
                    s.shreds
                })
                .collect()
        })
    });

    shreds.append(&mut past_shreds);

    trace!("Inserting {:?} shreds in blocktree", shreds.len());

    (shreds, unfinished_slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use crate::test_tx::test_tx;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::KeypairUtil;
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
    fn test_entries_to_shred() {
        let kp = Arc::new(Keypair::new());
        let entries: Vec<Entry> = (0..100)
            .map(|_| Entry {
                num_hashes: 100_000,
                hash: Hash::default(),
                transactions: vec![test_tx(); 128],
            })
            .collect();

        let mut expected_data_index = 123;
        let mut expected_code_index = 123;
        let (shreds, _) = entries_to_shreds(entries, 0, 0, 1, &kp, expected_data_index, 0, None);

        let mut reset_code_index = false;
        let mut found_last_in_slot = false;
        shreds.iter().for_each(|s| {
            if s.is_data() {
                if reset_code_index {
                    expected_code_index = expected_data_index;
                    reset_code_index = false;
                }
                assert_eq!(s.index(), expected_data_index as u32);
                expected_data_index += 1;
                found_last_in_slot = found_last_in_slot || s.last_in_slot();
            } else {
                assert_eq!(s.index(), expected_code_index as u32);
                expected_code_index += 1;
                reset_code_index = true;
            }
        });

        assert!(!found_last_in_slot);
    }

    #[test]
    fn test_entries_to_shred_slot_finish() {
        let kp = Arc::new(Keypair::new());
        let entries: Vec<Entry> = (0..100)
            .map(|_| Entry {
                num_hashes: 100_000,
                hash: Hash::default(),
                transactions: vec![test_tx(); 128],
            })
            .collect();

        let mut expected_data_index = 123;
        let mut expected_code_index = 123;
        let (shreds, _) = entries_to_shreds(entries, 0, 0, 0, &kp, expected_data_index, 0, None);

        let mut reset_code_index = false;
        let mut found_last_in_slot = false;
        shreds.iter().for_each(|s| {
            if s.is_data() {
                if reset_code_index {
                    expected_code_index = expected_data_index;
                    reset_code_index = false;
                }
                assert_eq!(s.index(), expected_data_index as u32);
                expected_data_index += 1;
                found_last_in_slot = found_last_in_slot || s.last_in_slot();
            } else {
                assert_eq!(s.index(), expected_code_index as u32);
                expected_code_index += 1;
                reset_code_index = true;
            }
        });

        assert!(found_last_in_slot);
    }

    #[test]
    fn test_recv_slot_entries_1() {
        let (genesis_block, bank0, tx) = setup_test();

        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        let (s, r) = channel();
        let mut last_hash = genesis_block.hash();

        assert!(bank1.max_tick_height() > 1);
        let entries: Vec<_> = (0..bank1.max_tick_height() + 1)
            .map(|i| {
                let entry = Entry::new(&last_hash, 1, vec![tx.clone()]);
                last_hash = entry.hash;
                s.send((bank1.clone(), (entry.clone(), i))).unwrap();
                entry
            })
            .collect();

        let result = recv_slot_entries(&r).unwrap();

        assert_eq!(result.bank.slot(), bank1.slot());
        assert_eq!(result.last_tick, bank1.max_tick_height());
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
        let expected_last_index = bank1.max_tick_height() - 1;
        let last_entry = (0..bank1.max_tick_height())
            .map(|i| {
                let entry = Entry::new(&last_hash, 1, vec![tx.clone()]);
                last_hash = entry.hash;
                // Interrupt slot 1 right before the last tick
                if i == expected_last_index {
                    s.send((bank2.clone(), (entry.clone(), i))).unwrap();
                    Some(entry)
                } else {
                    s.send((bank1.clone(), (entry, i))).unwrap();
                    None
                }
            })
            .last()
            .unwrap()
            .unwrap();

        let result = recv_slot_entries(&r).unwrap();
        assert_eq!(result.bank.slot(), bank2.slot());
        assert_eq!(result.last_tick, expected_last_index);
        assert_eq!(result.entries, vec![last_entry]);
    }
}
