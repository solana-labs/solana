use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::entry::{Entry, EntrySlice};
use crate::leader_scheduler::LeaderScheduler;
use log::Level;
use rayon::prelude::*;
use solana_metrics::counter::Counter;
use solana_runtime::bank::{Bank, BankError, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::timing::duration_as_ms;
use solana_sdk::timing::MAX_ENTRY_IDS;
use std::sync::{Arc, RwLock};
use std::time::Instant;

pub fn process_entry(bank: &Bank, entry: &Entry) -> Result<()> {
    if !entry.is_tick() {
        for result in bank.process_transactions(&entry.transactions) {
            match result {
                // Entries that result in a ProgramError are still valid and are written in the
                // ledger so map them to an ok return value
                Err(BankError::ProgramError(_, _)) => Ok(()),
                _ => result,
            }?;
        }
    } else {
        bank.register_tick(&entry.id);
    }
    Ok(())
}

fn first_err(results: &[Result<()>]) -> Result<()> {
    for r in results {
        r.clone()?;
    }
    Ok(())
}

fn ignore_program_errors(results: Vec<Result<()>>) -> Vec<Result<()>> {
    results
        .into_iter()
        .map(|result| match result {
            // Entries that result in a ProgramError are still valid and are written in the
            // ledger so map them to an ok return value
            Err(BankError::ProgramError(index, err)) => {
                info!("program error {:?}, {:?}", index, err);
                inc_new_counter_info!("bank-ignore_program_err", 1);
                Ok(())
            }
            _ => result,
        })
        .collect()
}

fn par_execute_entries(bank: &Bank, entries: &[(&Entry, Vec<Result<()>>)]) -> Result<()> {
    inc_new_counter_info!("bank-par_execute_entries-count", entries.len());
    let results: Vec<Result<()>> = entries
        .into_par_iter()
        .map(|(e, lock_results)| {
            let old_results = bank.load_execute_and_commit_transactions(
                &e.transactions,
                lock_results.to_vec(),
                MAX_ENTRY_IDS,
            );
            let results = ignore_program_errors(old_results);
            bank.unlock_accounts(&e.transactions, &results);
            first_err(&results)
        })
        .collect();

    first_err(&results)
}

/// process entries in parallel
/// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
/// 2. Process the locked group in parallel
/// 3. Register the `Tick` if it's available
/// 4. Update the leader scheduler, goto 1
fn par_process_entries_with_scheduler(
    bank: &Bank,
    entries: &[Entry],
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<()> {
    // accumulator for entries that can be processed in parallel
    let mut mt_group = vec![];
    for entry in entries {
        if entry.is_tick() {
            // if its a tick, execute the group and register the tick
            par_execute_entries(bank, &mt_group)?;
            bank.register_tick(&entry.id);
            leader_scheduler
                .write()
                .unwrap()
                .update_tick_height(bank.tick_height(), bank);
            mt_group = vec![];
            continue;
        }
        // try to lock the accounts
        let lock_results = bank.lock_accounts(&entry.transactions);
        // if any of the locks error out
        // execute the current group
        if first_err(&lock_results).is_err() {
            par_execute_entries(bank, &mt_group)?;
            mt_group = vec![];
            //reset the lock and push the entry
            bank.unlock_accounts(&entry.transactions, &lock_results);
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

/// Process an ordered list of entries.
pub fn process_entries(
    bank: &Bank,
    entries: &[Entry],
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<()> {
    par_process_entries_with_scheduler(bank, entries, leader_scheduler)
}

/// Process an ordered list of entries, populating a circular buffer "tail"
/// as we go.
fn process_block(
    bank: &Bank,
    entries: &[Entry],
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<()> {
    for entry in entries {
        process_entry(bank, entry)?;
        if entry.is_tick() {
            let mut leader_scheduler = leader_scheduler.write().unwrap();
            leader_scheduler.update_tick_height(bank.tick_height(), bank);
        }
    }

    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct BankForksInfo {
    pub bank_id: u64,
    pub entry_height: u64,
    pub last_entry_id: Hash,
}

pub fn process_blocktree(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<(BankForks, Vec<BankForksInfo>)> {
    let now = Instant::now();
    info!("processing ledger...");

    // Setup bank for slot 0
    let (mut bank_forks, mut pending_slots) = {
        let bank0 = Bank::new(&genesis_block);
        let bank_id = 0;
        let slot = 0;
        let entry_height = 0;
        leader_scheduler
            .write()
            .unwrap()
            .update_tick_height(slot, &bank0);
        let last_entry_id = bank0.last_id();

        (
            BankForks::new(bank_id, bank0),
            vec![(slot, bank_id, entry_height, last_entry_id)],
        )
    };

    let mut bank_forks_info = vec![];
    while !pending_slots.is_empty() {
        let (slot, bank_id, mut entry_height, mut last_entry_id) = pending_slots.pop().unwrap();

        bank_forks.set_working_bank_id(bank_id);
        let bank = bank_forks.working_bank();

        // Load the metadata for this slot
        let meta = blocktree
            .meta(slot)
            .map_err(|err| {
                warn!("Failed to load meta for slot {}: {:?}", slot, err);
                BankError::LedgerVerificationFailed
            })?
            .unwrap();
        trace!("processing slot {:?}, meta={:?}", slot, meta);

        // Fetch all entries for this slot
        let mut entries = blocktree.get_slot_entries(slot, 0, None).map_err(|err| {
            warn!("Failed to load entries for slot {}: {:?}", slot, err);
            BankError::LedgerVerificationFailed
        })?;

        if slot == 0 {
            // The first entry in the ledger is a pseudo-tick used only to ensure the number of ticks
            // in slot 0 is the same as the number of ticks in all subsequent slots.  It is not
            // registered as a tick and thus cannot be used as a last_id
            if entries.is_empty() {
                warn!("entry0 not present");
                return Err(BankError::LedgerVerificationFailed);
            }
            let entry0 = &entries[0];
            if !(entry0.is_tick() && entry0.verify(&last_entry_id)) {
                warn!("Ledger proof of history failed at entry0");
                return Err(BankError::LedgerVerificationFailed);
            }
            last_entry_id = entry0.id;
            entry_height += 1;
            entries = entries.drain(1..).collect();
        }

        // Feed the entries into the bank for this slot
        if !entries.is_empty() {
            if !entries.verify(&last_entry_id) {
                warn!("Ledger proof of history failed at entry: {}", entry_height);
                return Err(BankError::LedgerVerificationFailed);
            }

            process_block(&bank, &entries, &leader_scheduler).map_err(|err| {
                warn!("Failed to process entries for slot {}: {:?}", slot, err);
                BankError::LedgerVerificationFailed
            })?;

            last_entry_id = entries.last().unwrap().id;
            entry_height += entries.len() as u64;
        }

        match meta.next_slots.len() {
            0 => {
                // Reached the end of this fork.  Record the final entry height and last entry id
                bank_forks_info.push(BankForksInfo {
                    bank_id,
                    entry_height,
                    last_entry_id,
                })
            }
            1 => pending_slots.push((meta.next_slots[0], bank_id, entry_height, last_entry_id)),
            _ => {
                // This is a fork point, create a new child bank for each fork
                pending_slots.extend(meta.next_slots.iter().map(|next_slot| {
                    let leader = leader_scheduler
                        .read()
                        .unwrap()
                        .get_leader_for_slot(*next_slot)
                        .unwrap();
                    let child_bank = Bank::new_from_parent(&bank, &leader);
                    trace!("Add child bank for slot={}", next_slot);
                    let child_bank_id = *next_slot;
                    bank_forks.insert(child_bank_id, child_bank);
                    (*next_slot, child_bank_id, entry_height, last_entry_id)
                }));
            }
        }
        // reverse sort by slot, so the next slot to be processed can be pop()ed
        pending_slots.sort_by(|a, b| b.0.cmp(&a.0));
    }

    info!(
        "processed ledger in {}ms, forks={}...",
        duration_as_ms(&now.elapsed()),
        bank_forks_info.len(),
    );

    Ok((bank_forks, bank_forks_info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::tests::entries_to_blobs;
    use crate::blocktree::{create_tmp_sample_ledger, BlocktreeConfig};
    use crate::entry::{create_ticks, next_entry, Entry};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::native_program::ProgramError;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;

    fn fill_blocktree_slot_with_ticks(
        blocktree: &Blocktree,
        blocktree_config: &BlocktreeConfig,
        slot: u64,
        parent_slot: u64,
        last_entry_id: Hash,
    ) -> Hash {
        let entries = create_ticks(blocktree_config.ticks_per_slot, last_entry_id);
        let last_entry_id = entries.last().unwrap().id;

        let blobs = entries_to_blobs(&entries, slot, parent_slot);
        blocktree.insert_data_blobs(blobs.iter()).unwrap();

        last_entry_id
    }

    #[test]
    fn test_process_blocktree_with_two_forks() {
        solana_logger::setup();

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));
        let blocktree_config = &BlocktreeConfig::default();

        // Create a new ledger with slot 0 full of ticks
        let (_mint_keypair, ledger_path, tick_height, _entry_height, _last_id, mut last_entry_id) =
            create_tmp_sample_ledger(
                "blocktree_with_two_forks",
                10_000,
                blocktree_config.ticks_per_slot - 1,
                Keypair::new().pubkey(),
                123,
                &blocktree_config,
            );
        debug!("ledger_path: {:?}", ledger_path);
        assert_eq!(tick_height, blocktree_config.ticks_per_slot);

        /*
            Build a blocktree in the ledger with the following fork structure:

                 slot 0
                   |
                 slot 1
                 /   \
            slot 2   |
               /     |
            slot 3   |
                     |
                   slot 4

        */
        let genesis_block =
            GenesisBlock::load(&ledger_path).expect("Expected to successfully open genesis block");
        let blocktree = Blocktree::open_config(&ledger_path, &blocktree_config)
            .expect("Expected to successfully open database ledger");

        // Fork 1, ending at slot 3
        let last_slot1_entry_id =
            fill_blocktree_slot_with_ticks(&blocktree, &blocktree_config, 1, 0, last_entry_id);
        last_entry_id = fill_blocktree_slot_with_ticks(
            &blocktree,
            &blocktree_config,
            2,
            1,
            last_slot1_entry_id,
        );
        let last_fork1_entry_id =
            fill_blocktree_slot_with_ticks(&blocktree, &blocktree_config, 3, 2, last_entry_id);

        // Fork 2, ending at slot 4
        let last_fork2_entry_id = fill_blocktree_slot_with_ticks(
            &blocktree,
            &blocktree_config,
            4,
            1,
            last_slot1_entry_id,
        );

        info!("last_fork1_entry_id: {:?}", last_fork1_entry_id);
        info!("last_fork2_entry_id: {:?}", last_fork2_entry_id);

        let (mut bank_forks, bank_forks_info) =
            process_blocktree(&genesis_block, &blocktree, &leader_scheduler).unwrap();

        assert_eq!(bank_forks_info.len(), 2); // There are two forks
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_id: 2, // Fork 1 diverged with slot 2
                entry_height: blocktree_config.ticks_per_slot * 4,
                last_entry_id: last_fork1_entry_id,
            }
        );
        assert_eq!(
            bank_forks_info[1],
            BankForksInfo {
                bank_id: 4, // Fork 2 diverged with slot 4
                entry_height: blocktree_config.ticks_per_slot * 3,
                last_entry_id: last_fork2_entry_id,
            }
        );

        // Ensure bank_forks holds the right banks
        for info in bank_forks_info {
            bank_forks.set_working_bank_id(info.bank_id);
            assert_eq!(bank_forks.working_bank().last_id(), info.last_entry_id)
        }
    }

    #[test]
    fn test_first_err() {
        assert_eq!(first_err(&[Ok(())]), Ok(()));
        assert_eq!(
            first_err(&[Ok(()), Err(BankError::DuplicateSignature)]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(BankError::DuplicateSignature),
                Err(BankError::AccountInUse)
            ]),
            Err(BankError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(BankError::AccountInUse),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
        assert_eq!(
            first_err(&[
                Err(BankError::AccountInUse),
                Ok(()),
                Err(BankError::DuplicateSignature)
            ]),
            Err(BankError::AccountInUse)
        );
    }

    #[test]
    fn test_bank_ignore_program_errors() {
        let expected_results = vec![Ok(()), Ok(())];
        let results = vec![Ok(()), Ok(())];
        let updated_results = ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        let results = vec![
            Err(BankError::ProgramError(
                1,
                ProgramError::ResultWithNegativeTokens,
            )),
            Ok(()),
        ];
        let updated_results = ignore_program_errors(results);
        assert_eq!(updated_results, expected_results);

        // Other BankErrors should not be ignored
        let results = vec![Err(BankError::AccountNotFound), Ok(())];
        let updated_results = ignore_program_errors(results);
        assert_ne!(updated_results, expected_results);
    }

    fn par_process_entries(bank: &Bank, entries: &[Entry]) -> Result<()> {
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));
        par_process_entries_with_scheduler(bank, entries, &leader_scheduler)
    }

    #[test]
    fn test_process_empty_entry_is_registered() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2);
        let bank = Bank::new(&genesis_block);
        let keypair = Keypair::new();
        let entry = next_entry(&genesis_block.last_id(), 1, vec![]);
        let tx = SystemTransaction::new_account(&mint_keypair, keypair.pubkey(), 1, entry.id, 0);

        // First, ensure the TX is rejected because of the unregistered last ID
        assert_eq!(
            bank.process_transaction(&tx),
            Err(BankError::LastIdNotFound)
        );

        // Now ensure the TX is accepted despite pointing to the ID of an empty entry.
        par_process_entries(&bank, &[entry]).unwrap();
        assert_eq!(bank.process_transaction(&tx), Ok(()));
    }

    #[test]
    fn test_process_ledger_simple() {
        let blocktree_config = BlocktreeConfig::default();
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));

        let (
            mint_keypair,
            ledger_path,
            tick_height,
            mut entry_height,
            mut last_id,
            mut last_entry_id,
        ) = create_tmp_sample_ledger(
            "process_ledger_simple",
            100,
            0,
            Keypair::new().pubkey(),
            50,
            &blocktree_config,
        );
        debug!("ledger_path: {:?}", ledger_path);
        let genesis_block =
            GenesisBlock::load(&ledger_path).expect("Expected to successfully open genesis block");

        let mut entries = vec![];
        for _ in 0..3 {
            // Transfer one token from the mint to a random account
            let keypair = Keypair::new();
            let tx = SystemTransaction::new_account(&mint_keypair, keypair.pubkey(), 1, last_id, 0);
            let entry = Entry::new(&last_entry_id, 1, vec![tx]);
            last_entry_id = entry.id;
            entries.push(entry);

            // Add a second Transaction that will produce a
            // ProgramError<0, ResultWithNegativeTokens> error when processed
            let keypair2 = Keypair::new();
            let tx = SystemTransaction::new_account(&keypair, keypair2.pubkey(), 42, last_id, 0);
            let entry = Entry::new(&last_entry_id, 1, vec![tx]);
            last_entry_id = entry.id;
            entries.push(entry);
        }

        // Add a tick for good measure
        let tick = Entry::new(&last_entry_id, 1, vec![]);
        last_entry_id = tick.id;
        last_id = last_entry_id;
        entries.push(tick);

        let blocktree = Blocktree::open_config(&ledger_path, &blocktree_config)
            .expect("Expected to successfully open database ledger");

        blocktree
            .write_entries(0, tick_height, entry_height, &entries)
            .unwrap();
        entry_height += entries.len() as u64;

        let (bank_forks, bank_forks_info) =
            process_blocktree(&genesis_block, &blocktree, &leader_scheduler).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_id: 0,
                entry_height,
                last_entry_id,
            }
        );

        let bank = bank_forks.working_bank();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 50 - 3);
        assert_eq!(bank.tick_height(), 1);
        assert_eq!(bank.last_id(), last_id);
    }

    #[test]
    fn test_par_process_entries_tick() {
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);

        // ensure bank can process a tick
        let tick = next_entry(&genesis_block.last_id(), 1, vec![]);
        assert_eq!(par_process_entries(&bank, &[tick.clone()]), Ok(()));
        assert_eq!(bank.last_id(), tick.id);
    }

    #[test]
    fn test_par_process_entries_2_entries_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        let last_id = bank.last_id();

        // ensure bank can process 2 entries that have a common account and no tick is registered
        let tx =
            SystemTransaction::new_account(&mint_keypair, keypair1.pubkey(), 2, bank.last_id(), 0);
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tx =
            SystemTransaction::new_account(&mint_keypair, keypair2.pubkey(), 2, bank.last_id(), 0);
        let entry_2 = next_entry(&entry_1.id, 1, vec![tx]);
        assert_eq!(par_process_entries(&bank, &[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.last_id(), last_id);
    }

    #[test]
    fn test_par_process_entries_2_txes_collision() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();

        // fund: put 4 in each of 1 and 2
        assert_matches!(
            bank.transfer(4, &mint_keypair, keypair1.pubkey(), bank.last_id()),
            Ok(_)
        );
        assert_matches!(
            bank.transfer(4, &mint_keypair, keypair2.pubkey(), bank.last_id()),
            Ok(_)
        );

        // construct an Entry whose 2nd transaction would cause a lock conflict with previous entry
        let entry_1_to_mint = next_entry(
            &bank.last_id(),
            1,
            vec![SystemTransaction::new_account(
                &keypair1,
                mint_keypair.pubkey(),
                1,
                bank.last_id(),
                0,
            )],
        );

        let entry_2_to_3_mint_to_1 = next_entry(
            &entry_1_to_mint.id,
            1,
            vec![
                SystemTransaction::new_account(&keypair2, keypair3.pubkey(), 2, bank.last_id(), 0), // should be fine
                SystemTransaction::new_account(
                    &keypair1,
                    mint_keypair.pubkey(),
                    2,
                    bank.last_id(),
                    0,
                ), // will collide
            ],
        );

        assert_eq!(
            par_process_entries(&bank, &[entry_1_to_mint, entry_2_to_3_mint_to_1]),
            Ok(())
        );

        assert_eq!(bank.get_balance(&keypair1.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 2);
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 2);
    }

    #[test]
    fn test_par_process_entries_2_entries_par() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx =
            SystemTransaction::new_account(&mint_keypair, keypair1.pubkey(), 1, bank.last_id(), 0);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx =
            SystemTransaction::new_account(&mint_keypair, keypair2.pubkey(), 1, bank.last_id(), 0);
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        // ensure bank can process 2 entries that do not have a common account and no tick is registered
        let last_id = bank.last_id();
        let tx = SystemTransaction::new_account(&keypair1, keypair3.pubkey(), 1, bank.last_id(), 0);
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tx = SystemTransaction::new_account(&keypair2, keypair4.pubkey(), 1, bank.last_id(), 0);
        let entry_2 = next_entry(&entry_1.id, 1, vec![tx]);
        assert_eq!(par_process_entries(&bank, &[entry_1, entry_2]), Ok(()));
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_id(), last_id);
    }

    #[test]
    fn test_par_process_entries_2_entries_tick() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let bank = Bank::new(&genesis_block);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();

        //load accounts
        let tx =
            SystemTransaction::new_account(&mint_keypair, keypair1.pubkey(), 1, bank.last_id(), 0);
        assert_eq!(bank.process_transaction(&tx), Ok(()));
        let tx =
            SystemTransaction::new_account(&mint_keypair, keypair2.pubkey(), 1, bank.last_id(), 0);
        assert_eq!(bank.process_transaction(&tx), Ok(()));

        let last_id = bank.last_id();

        // ensure bank can process 2 entries that do not have a common account and tick is registered
        let tx = SystemTransaction::new_account(&keypair2, keypair3.pubkey(), 1, bank.last_id(), 0);
        let entry_1 = next_entry(&last_id, 1, vec![tx]);
        let tick = next_entry(&entry_1.id, 1, vec![]);
        let tx = SystemTransaction::new_account(&keypair1, keypair4.pubkey(), 1, tick.id, 0);
        let entry_2 = next_entry(&tick.id, 1, vec![tx]);
        assert_eq!(
            par_process_entries(&bank, &[entry_1.clone(), tick.clone(), entry_2.clone()]),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);
        assert_eq!(bank.last_id(), tick.id);
        // ensure that an error is returned for an empty account (keypair2)
        let tx = SystemTransaction::new_account(&keypair2, keypair3.pubkey(), 1, tick.id, 0);
        let entry_3 = next_entry(&entry_2.id, 1, vec![tx]);
        assert_eq!(
            par_process_entries(&bank, &[entry_3]),
            Err(BankError::AccountNotFound)
        );
    }
}
