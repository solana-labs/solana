use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::entry::{Entry, EntrySlice};
use crate::leader_schedule_cache::LeaderScheduleCache;
use rayon::prelude::*;
use solana_metrics::counter::Counter;
use solana_runtime::bank::Bank;
use solana_runtime::locked_accounts_results::LockedAccountsResults;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::timing::MAX_RECENT_BLOCKHASHES;
use solana_sdk::transaction::{Result, TransactionError};
use std::result;
use std::sync::Arc;
use std::time::Instant;

fn first_err(results: &[Result<()>]) -> Result<()> {
    for r in results {
        r.clone()?;
    }
    Ok(())
}

fn is_unexpected_validator_error(r: &Result<()>) -> bool {
    match r {
        Err(TransactionError::DuplicateSignature) => true,
        _ => false,
    }
}

fn par_execute_entries(bank: &Bank, entries: &[(&Entry, LockedAccountsResults)]) -> Result<()> {
    inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
    let results: Vec<Result<()>> = entries
        .into_par_iter()
        .map(|(e, locked_accounts)| {
            let results = bank.load_execute_and_commit_transactions(
                &e.transactions,
                locked_accounts,
                MAX_RECENT_BLOCKHASHES,
            );
            let mut first_err = None;
            for r in results {
                if let Err(ref e) = r {
                    if first_err.is_none() {
                        first_err = Some(r.clone());
                    }
                    if is_unexpected_validator_error(&r) {
                        warn!("Unexpected validator error: {:?}", e);
                        solana_metrics::submit(
                            solana_metrics::influxdb::Point::new("validator_process_entry_error")
                                .add_field(
                                    "error",
                                    solana_metrics::influxdb::Value::String(format!("{:?}", e)),
                                )
                                .to_owned(),
                        )
                    }
                }
            }
            first_err.unwrap_or(Ok(()))
        })
        .collect();

    first_err(&results)
}

/// Process an ordered list of entries in parallel
/// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
/// 2. Process the locked group in parallel
/// 3. Register the `Tick` if it's available
/// 4. Update the leader scheduler, goto 1
pub fn process_entries(bank: &Bank, entries: &[Entry]) -> Result<()> {
    // accumulator for entries that can be processed in parallel
    let mut mt_group = vec![];
    for entry in entries {
        if entry.is_tick() {
            // if its a tick, execute the group and register the tick
            par_execute_entries(bank, &mt_group)?;
            bank.register_tick(&entry.hash);
            mt_group = vec![];
            continue;
        }
        // try to lock the accounts
        let lock_results = bank.lock_accounts(&entry.transactions);
        // if any of the locks error out
        // execute the current group
        if first_err(lock_results.locked_accounts_results()).is_err() {
            par_execute_entries(bank, &mt_group)?;
            // Drop all the locks on accounts by clearing the LockedAccountsFinalizer's in the
            // mt_group
            mt_group = vec![];
            drop(lock_results);
            let lock_results = bank.lock_accounts(&entry.transactions);
            mt_group.push((entry, lock_results));
        } else {
            // push the entry to the mt_group
            mt_group.push((entry, lock_results));
        }
    }
    par_execute_entries(bank, &mt_group)?;
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct BankForksInfo {
    pub bank_slot: u64,
    pub entry_height: u64,
}

#[derive(Debug)]
pub enum BlocktreeProcessorError {
    LedgerVerificationFailed,
}

pub fn process_blocktree(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    account_paths: Option<String>,
) -> result::Result<(BankForks, Vec<BankForksInfo>, LeaderScheduleCache), BlocktreeProcessorError> {
    let now = Instant::now();
    info!("processing ledger...");
    // Setup bank for slot 0
    let mut pending_slots = {
        let slot = 0;
        let bank = Arc::new(Bank::new_with_paths(&genesis_block, account_paths));
        let entry_height = 0;
        let last_entry_hash = bank.last_blockhash();

        // Load the metadata for this slot
        let meta = blocktree
            .meta(slot)
            .map_err(|err| {
                warn!("Failed to load meta for slot {}: {:?}", slot, err);
                BlocktreeProcessorError::LedgerVerificationFailed
            })?
            .unwrap();

        vec![(slot, meta, bank, entry_height, last_entry_hash)]
    };

    let leader_schedule_cache = LeaderScheduleCache::new(*pending_slots[0].2.epoch_schedule());

    let mut fork_info = vec![];
    while !pending_slots.is_empty() {
        let (slot, meta, bank, mut entry_height, mut last_entry_hash) =
            pending_slots.pop().unwrap();

        // Fetch all entries for this slot
        let mut entries = blocktree.get_slot_entries(slot, 0, None).map_err(|err| {
            warn!("Failed to load entries for slot {}: {:?}", slot, err);
            BlocktreeProcessorError::LedgerVerificationFailed
        })?;

        if slot == 0 {
            // The first entry in the ledger is a pseudo-tick used only to ensure the number of ticks
            // in slot 0 is the same as the number of ticks in all subsequent slots.  It is not
            // processed by the bank, skip over it.
            if entries.is_empty() {
                warn!("entry0 not present");
                return Err(BlocktreeProcessorError::LedgerVerificationFailed);
            }
            let entry0 = entries.remove(0);
            if !(entry0.is_tick() && entry0.verify(&last_entry_hash)) {
                warn!("Ledger proof of history failed at entry0");
                return Err(BlocktreeProcessorError::LedgerVerificationFailed);
            }
            last_entry_hash = entry0.hash;
            entry_height += 1;
        }

        if !entries.is_empty() {
            if !entries.verify(&last_entry_hash) {
                warn!(
                    "Ledger proof of history failed at slot: {}, entry: {}",
                    slot, entry_height
                );
                return Err(BlocktreeProcessorError::LedgerVerificationFailed);
            }

            process_entries(&bank, &entries).map_err(|err| {
                warn!("Failed to process entries for slot {}: {:?}", slot, err);
                BlocktreeProcessorError::LedgerVerificationFailed
            })?;

            last_entry_hash = entries.last().unwrap().hash;
            entry_height += entries.len() as u64;
        }

        bank.freeze(); // all banks handled by this routine are created from complete slots

        if blocktree.is_root(slot) {
            bank.squash();
        }

        if meta.next_slots.is_empty() {
            // Reached the end of this fork.  Record the final entry height and last entry.hash
            let bfi = BankForksInfo {
                bank_slot: slot,
                entry_height,
            };
            fork_info.push((bank, bfi));
            continue;
        }

        // This is a fork point, create a new child bank for each fork
        for next_slot in meta.next_slots {
            let next_meta = blocktree
                .meta(next_slot)
                .map_err(|err| {
                    warn!("Failed to load meta for slot {}: {:?}", slot, err);
                    BlocktreeProcessorError::LedgerVerificationFailed
                })?
                .unwrap();

            // only process full slots in blocktree_processor, replay_stage
            //  handles any partials
            if next_meta.is_full() {
                let next_bank = Arc::new(Bank::new_from_parent(
                    &bank,
                    &leader_schedule_cache
                        .slot_leader_at_else_compute(next_slot, &bank)
                        .unwrap(),
                    next_slot,
                ));
                trace!("Add child bank for slot={}", next_slot);
                // bank_forks.insert(*next_slot, child_bank);
                pending_slots.push((
                    next_slot,
                    next_meta,
                    next_bank,
                    entry_height,
                    last_entry_hash,
                ));
            } else {
                let bfi = BankForksInfo {
                    bank_slot: slot,
                    entry_height,
                };
                fork_info.push((bank.clone(), bfi));
            }
        }

        // reverse sort by slot, so the next slot to be processed can be pop()ed
        // TODO: remove me once leader_scheduler can hang with out-of-order slots?
        pending_slots.sort_by(|a, b| b.0.cmp(&a.0));
    }

    let (banks, bank_forks_info): (Vec<_>, Vec<_>) = fork_info.into_iter().unzip();
    let bank_forks = BankForks::new_from_banks(&banks);
    info!(
        "processed ledger in {}ms, forks={}...",
        duration_as_ms(&now.elapsed()),
        bank_forks_info.len(),
    );

    Ok((bank_forks, bank_forks_info, leader_schedule_cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::create_new_tmp_ledger;
    use crate::blocktree::tests::entries_to_blobs;
    use crate::entry::{create_ticks, next_entry, Entry};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use solana_sdk::transaction::TransactionError;

    fn fill_blocktree_slot_with_ticks(
        blocktree: &Blocktree,
        ticks_per_slot: u64,
        slot: u64,
        parent_slot: u64,
        last_entry_hash: Hash,
    ) -> Hash {
        let entries = create_ticks(ticks_per_slot, last_entry_hash);
        let last_entry_hash = entries.last().unwrap().hash;

        let blobs = entries_to_blobs(&entries, slot, parent_slot, true);
        blocktree.insert_data_blobs(blobs.iter()).unwrap();

        last_entry_hash
    }

    #[test]
    fn test_process_blocktree_with_incomplete_slot() {
        solana_logger::setup();

        let (genesis_block, _mint_keypair) = GenesisBlock::new(10_000);
        let ticks_per_slot = genesis_block.ticks_per_slot;

        /*
          Build a blocktree in the ledger with the following fork structure:

               slot 0 (all ticks)
                 |
               slot 1 (all ticks but one)
                 |
               slot 2 (all ticks)

           where slot 1 is incomplete (missing 1 tick at the end)
        */

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, mut blockhash) = create_new_tmp_ledger!(&genesis_block);
        debug!("ledger_path: {:?}", ledger_path);

        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

        // Write slot 1
        // slot 1, points at slot 0.  Missing one tick
        {
            let parent_slot = 0;
            let slot = 1;
            let mut entries = create_ticks(ticks_per_slot, blockhash);
            blockhash = entries.last().unwrap().hash;

            // throw away last one
            entries.pop();

            let blobs = entries_to_blobs(&entries, slot, parent_slot, false);
            blocktree.insert_data_blobs(blobs.iter()).unwrap();
        }

        // slot 2, points at slot 1
        fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, blockhash);

        let (mut _bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 0, // slot 1 isn't "full", we stop at slot zero
                entry_height: ticks_per_slot,
            }
        );
    }

    #[test]
    fn test_process_blocktree_with_two_forks() {
        solana_logger::setup();

        let (genesis_block, _mint_keypair) = GenesisBlock::new(10_000);
        let ticks_per_slot = genesis_block.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_block);
        debug!("ledger_path: {:?}", ledger_path);
        let mut last_entry_hash = blockhash;

        /*
            Build a blocktree in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1  <-- set_root(true)
                 /   \
            slot 2   |
               /     |
            slot 3   |
                     |
                   slot 4

        */
        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

        // Fork 1, ending at slot 3
        let last_slot1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 1, 0, last_entry_hash);
        last_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, last_slot1_entry_hash);
        let last_fork1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 3, 2, last_entry_hash);

        // Fork 2, ending at slot 4
        let last_fork2_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 4, 1, last_slot1_entry_hash);

        info!("last_fork1_entry.hash: {:?}", last_fork1_entry_hash);
        info!("last_fork2_entry.hash: {:?}", last_fork2_entry_hash);

        blocktree.set_root(0).unwrap();
        blocktree.set_root(1).unwrap();

        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 2); // There are two forks
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 3, // Fork 1's head is slot 3
                entry_height: ticks_per_slot * 4,
            }
        );
        assert_eq!(
            &bank_forks[3]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[2, 1]
        );
        assert_eq!(
            bank_forks_info[1],
            BankForksInfo {
                bank_slot: 4, // Fork 2's head is slot 4
                entry_height: ticks_per_slot * 3,
            }
        );
        assert_eq!(
            &bank_forks[4]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[1]
        );

        // Ensure bank_forks holds the right banks
        for info in bank_forks_info {
            assert_eq!(bank_forks[info.bank_slot].slot(), info.bank_slot);
            assert!(bank_forks[info.bank_slot].is_frozen());
        }
    }

    #[test]
    fn test_first_err() {
        assert_eq!(first_err(&[Ok(())]), Ok(()));
        assert_eq!(
            first_err(&[Ok(()), Err(TransactionError::DuplicateSignature)]),
            Err(TransactionError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(TransactionError::DuplicateSignature),
                Err(TransactionError::AccountInUse)
            ]),
            Err(TransactionError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(TransactionError::AccountInUse),
                Err(TransactionError::DuplicateSignature)
            ]),
            Err(TransactionError::AccountInUse)
        );
        assert_eq!(
            first_err(&[
                Err(TransactionError::AccountInUse),
                Ok(()),
                Err(TransactionError::DuplicateSignature)
            ]),
            Err(TransactionError::AccountInUse)
        );
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        solana_logger::setup();

        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let slot_entries = create_ticks(genesis_block.ticks_per_slot - 1, genesis_block.hash());
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair.pubkey(),
            1,
            slot_entries.last().unwrap().hash,
            0,
        );

        // First, ensure the TX is rejected because of the unregistered last ID
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::BlockhashNotFound)
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        process_entries(&bank, &slot_entries).unwrap();
        assert_eq!(bank.process_transaction(&tx), Ok(()));
    }

    #[test]
    fn test_process_ledger_simple() {
        solana_logger::setup();
        let leader_pubkey = Pubkey::new_rand();
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(100, &leader_pubkey, 50);
        let (ledger_path, mut last_entry_hash) = create_new_tmp_ledger!(&genesis_block);
        debug!("ledger_path: {:?}", ledger_path);

        let mut entries = vec![];
        let blockhash = genesis_block.hash();
        for _ in 0..3 {
            // Transfer one token from the mint to a random account
            let keypair = Keypair::new();
            let tx = system_transaction::create_user_account(
                &mint_keypair,
                &keypair.pubkey(),
                1,
                blockhash,
                0,
            );
            let entry = Entry::new(&last_entry_hash, 1, vec![tx]);
            last_entry_hash = entry.hash;
            entries.push(entry);

            // Add a second Transaction that will produce a
            // InstructionError<0, ResultWithNegativeLamports> error when processed
            let keypair2 = Keypair::new();
            let tx = system_transaction::create_user_account(
                &keypair,
                &keypair2.pubkey(),
                42,
                blockhash,
                0,
            );
            let entry = Entry::new(&last_entry_hash, 1, vec![tx]);
            last_entry_hash = entry.hash;
            entries.push(entry);
        }

        // Fill up the rest of slot 1 with ticks
        entries.extend(create_ticks(genesis_block.ticks_per_slot, last_entry_hash));

        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");
        blocktree
            .write_entries(1, 0, 0, genesis_block.ticks_per_slot, &entries)
            .unwrap();
        let entry_height = genesis_block.ticks_per_slot + entries.len() as u64;
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 1,
                entry_height,
            }
        );

        let bank = bank_forks[1].clone();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 50 - 3);
        assert_eq!(bank.tick_height(), 2 * genesis_block.ticks_per_slot - 1);
        assert_eq!(bank.last_blockhash(), entries.last().unwrap().hash);
    }

    #[test]
    fn test_process_ledger_with_one_tick_per_slot() {
        let (mut genesis_block, _mint_keypair) = GenesisBlock::new(123);
        genesis_block.ticks_per_slot = 1;
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_block, &blocktree, None).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 0,
                entry_height: 1,
            }
        );
        let bank = bank_forks[0].clone();
        assert_eq!(bank.tick_height(), 0);
    }

    #[test]
    fn test_process_entries_tick() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);

        // ensure bank can process a tick
        assert_eq!(bank.tick_height(), 0);
        let tick = next_entry(&genesis_block.hash(), 1, vec![]);
        assert_eq!(process_entries(&bank, &[tick.clone()]), Ok(()));
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_process_entries_2_entries_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let blockhash = bank.last_blockhash();

        // ensure bank can process 2 entries that have a common account and no tick is registered
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair1.pubkey(),
            2,
            bank.last_blockhash(),
            0,
        );
        let entry_1 = next_entry(&blockhash, 1, vec![tx]);
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair2.pubkey(),
            2,
            bank.last_blockhash(),
            0,
        );
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);
        assert_eq!(process_entries(&bank, &[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.last_blockhash(), blockhash);
    }

    #[test]
    fn test_process_entries_2_txes_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        // fund: put 4 in each of 1 and 2
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair1.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair2.pubkey()), Ok(_));

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![system_transaction::create_user_account(
                &keypair1,
                &mint_keypair.pubkey(),
                1,
                bank.last_blockhash(),
                0,
            )],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.hash,
            1,
            vec![
                system_transaction::create_user_account(
                    &keypair2,
                    &keypair3.pubkey(),
                    2,
                    bank.last_blockhash(),
                    0,
                ), // should be fine
                system_transaction::create_user_account(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    2,
                    bank.last_blockhash(),
                    0,
                ), // will collide
            ],
        );

        assert_eq!(
            process_entries(&bank, &[entry_1_to_mint, entry_2_to_3_mint_to_1]),
            Ok(())
        );

        assert_eq!(bank.get_balance(&keypair1.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 2);
    }

    #[test]
    fn test_process_entries_2_txes_collision_and_error() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        // fund: put 4 in each of 1 and 2
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair1.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair2.pubkey()), Ok(_));
        assert_matches!(bank.transfer(4, &mint_keypair, &keypair4.pubkey()), Ok(_));

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![
                system_transaction::create_user_account(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    1,
                    bank.last_blockhash(),
                    0,
                ),
                system_transaction::transfer(
                    &keypair4,
                    &keypair4.pubkey(),
                    1,
                    Hash::default(), // Should cause a transaction failure with BlockhashNotFound
                    0,
                ),
            ],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.hash,
            1,
            vec![
                system_transaction::create_user_account(
                    &keypair2,
                    &keypair3.pubkey(),
                    2,
                    bank.last_blockhash(),
                    0,
                ), // should be fine
                system_transaction::create_user_account(
                    &keypair1,
                    &mint_keypair.pubkey(),
                    2,
                    bank.last_blockhash(),
                    0,
                ), // will collide
            ],
        );

        assert!(process_entries(
            &bank,
            &[entry_1_to_mint.clone(), entry_2_to_3_mint_to_1.clone()]
        )
        .is_err());

        // First transaction in first entry succeeded, so keypair1 lost 1 lamport
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 3);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 4);

        // Check all accounts are unlocked
        let txs1 = &entry_1_to_mint.transactions[..];
        let txs2 = &entry_2_to_3_mint_to_1.transactions[..];
        let locked_accounts1 = bank.lock_accounts(txs1);
        for result in locked_accounts1.locked_accounts_results() {
            assert!(result.is_ok());
        }
        // txs1 and txs2 have accounts that conflict, so we must drop txs1 first
        drop(locked_accounts1);
        let locked_accounts2 = bank.lock_accounts(txs2);
        for result in locked_accounts2.locked_accounts_results() {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_process_entries_2_entries_par() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair1.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair2.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        // ensure bank can process 2 entries that do not have a common account and no tick is registered
        let blockhash = bank.last_blockhash();
        let tx = system_transaction::create_user_account(
            &keypair1,
            &keypair3.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        let entry_1 = next_entry(&blockhash, 1, vec![tx]);
        let tx = system_transaction::create_user_account(
            &keypair2,
            &keypair4.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        let entry_2 = next_entry(&entry_1.hash, 1, vec![tx]);
        assert_eq!(process_entries(&bank, &[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_blockhash(), blockhash);
    }

    #[test]
    fn test_process_entries_2_entries_tick() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair1.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair2.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        let blockhash = bank.last_blockhash();
        while blockhash == bank.last_blockhash() {
            bank.register_tick(&Hash::default());
        }

        // ensure bank can process 2 entries that do not have a common account and tick is registered
        let tx =
            system_transaction::create_user_account(&keypair2, &keypair3.pubkey(), 1, blockhash, 0);
        let entry_1 = next_entry(&blockhash, 1, vec![tx]);
        let tick = next_entry(&entry_1.hash, 1, vec![]);
        let tx = system_transaction::create_user_account(
            &keypair1,
            &keypair4.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        let entry_2 = next_entry(&tick.hash, 1, vec![tx]);
        assert_eq!(
            process_entries(&bank, &[entry_1.clone(), tick.clone(), entry_2.clone()]),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);

        // ensure that an error is returned for an empty account (keypair2)
        let tx = system_transaction::create_user_account(
            &keypair2,
            &keypair3.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        let entry_3 = next_entry(&entry_2.hash, 1, vec![tx]);
        assert_eq!(
            process_entries(&bank, &[entry_3]),
            Err(TransactionError::AccountNotFound)
        );
    }

    #[test]
    fn test_update_transaction_statuses() {
        // Make sure instruction errors still update the signature cache
        let (genesis_block, mint_keypair) = GenesisBlock::new(11_000);
        let bank = Bank::new(&genesis_block);
        let pubkey = Pubkey::new_rand();
        bank.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::new_result_with_negative_lamports(),
            ))
        );
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::DuplicateSignature)
        );

        // Make sure other errors don't update the signature cache
        let tx = system_transaction::create_user_account(
            &mint_keypair,
            &pubkey,
            1000,
            Hash::default(),
            0,
        );
        let signature = tx.signatures[0];

        // Should fail with blockhash not found
        assert_eq!(
            bank.process_transaction(&tx).map(|_| signature),
            Err(TransactionError::BlockhashNotFound)
        );

        // Should fail again with blockhash not found
        assert_eq!(
            bank.process_transaction(&tx).map(|_| signature),
            Err(TransactionError::BlockhashNotFound)
        );
    }

    #[test]
    fn test_update_transaction_statuses_fail() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(11_000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let success_tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair1.pubkey(),
            1,
            bank.last_blockhash(),
            0,
        );
        let fail_tx = system_transaction::create_user_account(
            &mint_keypair,
            &keypair2.pubkey(),
            2,
            bank.last_blockhash(),
            0,
        );

        let entry_1_to_mint = next_entry(
            &bank.last_blockhash(),
            1,
            vec![
                success_tx,
                fail_tx.clone(), // will collide
            ],
        );

        assert_eq!(
            process_entries(&bank, &[entry_1_to_mint]),
            Err(TransactionError::AccountInUse)
        );

        // Should not see duplicate signature error
        assert_eq!(bank.process_transaction(&fail_tx), Ok(()));
    }
}
