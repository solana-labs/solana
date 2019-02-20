use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::entry::{Entry, EntrySlice};
use crate::leader_scheduler::LeaderScheduler;
use itertools::Itertools;
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

pub const VERIFY_BLOCK_SIZE: usize = 16;

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

/// Starting from the genesis block, append the provided entries to the ledger verifying them
/// along the way.
fn process_ledger<I>(
    bank: &Bank,
    entries: I,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<(u64, Hash)>
where
    I: IntoIterator<Item = Entry>,
{
    let mut last_entry_id = bank.last_id();
    let mut entries_iter = entries.into_iter();

    trace!("genesis last_id={}", last_entry_id);

    // The first entry in the ledger is a pseudo-tick used only to ensure the number of ticks
    // in slot 0 is the same as the number of ticks in all subsequent slots.  It is not
    // registered as a tick and thus cannot be used as a last_id
    let entry0 = entries_iter
        .next()
        .ok_or(BankError::LedgerVerificationFailed)?;
    if !(entry0.is_tick() && entry0.verify(&last_entry_id)) {
        warn!("Ledger proof of history failed at entry0");
        return Err(BankError::LedgerVerificationFailed);
    }
    last_entry_id = entry0.id;
    let mut entry_height = 1;

    // Ledger verification needs to be parallelized, but we can't pull the whole
    // thing into memory. We therefore chunk it.
    for block in &entries_iter.chunks(VERIFY_BLOCK_SIZE) {
        let block: Vec<_> = block.collect();

        if !block.verify(&last_entry_id) {
            warn!("Ledger proof of history failed at entry: {}", entry_height);
            return Err(BankError::LedgerVerificationFailed);
        }

        process_block(bank, &block, leader_scheduler)?;

        last_entry_id = block.last().unwrap().id;
        entry_height += block.len() as u64;
    }
    Ok((entry_height, last_entry_id))
}

pub fn process_blocktree(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
) -> Result<(BankForks, u64, Hash)> {
    let bank = Bank::new(&genesis_block);
    let slot_height = 0; // Use the Bank's slot_height as its ID.
    let bank_forks = BankForks::new(slot_height, bank);
    leader_scheduler
        .write()
        .unwrap()
        .update_tick_height(0, &bank_forks.finalized_bank());

    let now = Instant::now();
    info!("processing ledger...");
    let entries = blocktree.read_ledger().expect("opening ledger");
    let (entry_height, last_entry_id) =
        process_ledger(&bank_forks.working_bank(), entries, leader_scheduler)?;

    info!(
        "processed {} ledger entries in {}ms, tick_height={}...",
        entry_height,
        duration_as_ms(&now.elapsed()),
        bank_forks.working_bank().tick_height()
    );

    // TODO: probably need to return `entry_height` and `last_entry_id` for *all* banks in
    // `bank_forks` instead of just for the `working_bank`
    Ok((bank_forks, entry_height, last_entry_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::tests::entries_to_blobs;
    use crate::blocktree::{create_tmp_sample_ledger, BlocktreeConfig};
    use crate::entry::{create_ticks, next_entries, next_entry, Entry};
    use crate::gen_keys::GenKeys;
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
        let (
            _mint_keypair,
            ledger_path,
            tick_height,
            _last_entry_height,
            _last_id,
            mut last_entry_id,
        ) = create_tmp_sample_ledger(
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
        let (blocktree, _ledger_signal_receiver) =
            Blocktree::open_with_config_signal(&ledger_path, &blocktree_config)
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

        let (bank_forks, ledger_height, last_entry_id) =
            process_blocktree(&genesis_block, &blocktree, &leader_scheduler).unwrap();

        // The following asserts loosely demonstrate how `process_blocktree()` currently only
        // processes fork1 and ignores fork2.
        assert_eq!(last_entry_id, last_fork1_entry_id);
        assert_eq!(ledger_height, 4 * blocktree_config.ticks_per_slot);
        assert_eq!(bank_forks.working_bank().last_id(), last_entry_id);
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

    fn create_sample_block_with_next_entries_using_keypairs(
        genesis_block: &GenesisBlock,
        mint_keypair: &Keypair,
        keypairs: &[Keypair],
    ) -> Vec<Entry> {
        let mut entries: Vec<Entry> = vec![];

        let mut last_id = genesis_block.last_id();

        // Start off the ledger with the psuedo-tick linked to the genesis block
        // (see entry0 in `process_ledger`)
        let tick = Entry::new(&genesis_block.last_id(), 0, 1, vec![]);
        let mut hash = tick.id;
        entries.push(tick);

        let num_hashes = 1;
        for k in keypairs {
            let tx = SystemTransaction::new_account(mint_keypair, k.pubkey(), 1, last_id, 0);
            let txs = vec![tx];
            let mut e = next_entries(&hash, 0, txs);
            entries.append(&mut e);
            hash = entries.last().unwrap().id;
            let tick = Entry::new(&hash, 0, num_hashes, vec![]);
            hash = tick.id;
            last_id = hash;
            entries.push(tick);
        }
        entries
    }

    #[test]
    fn test_hash_internal_state() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(2_000);
        let seed = [0u8; 32];
        let mut rnd = GenKeys::new(seed);
        let keypairs = rnd.gen_n_keypairs(5);
        let entries0 = create_sample_block_with_next_entries_using_keypairs(
            &genesis_block,
            &mint_keypair,
            &keypairs,
        );
        let entries1 = create_sample_block_with_next_entries_using_keypairs(
            &genesis_block,
            &mint_keypair,
            &keypairs,
        );

        let bank0 = Bank::new(&genesis_block);
        par_process_entries(&bank0, &entries0).unwrap();
        let bank1 = Bank::new(&genesis_block);
        par_process_entries(&bank1, &entries1).unwrap();

        let initial_state = bank0.hash_internal_state();

        assert_eq!(bank1.hash_internal_state(), initial_state);

        let pubkey = keypairs[0].pubkey();
        bank0
            .transfer(1_000, &mint_keypair, pubkey, bank0.last_id())
            .unwrap();
        assert_ne!(bank0.hash_internal_state(), initial_state);
        bank1
            .transfer(1_000, &mint_keypair, pubkey, bank1.last_id())
            .unwrap();
        assert_eq!(bank0.hash_internal_state(), bank1.hash_internal_state());

        // Checkpointing should not change its state
        let bank2 = Bank::new_from_parent(&Arc::new(bank1));
        assert_eq!(bank0.hash_internal_state(), bank2.hash_internal_state());
    }

    #[test]
    fn test_hash_internal_state_parents() {
        let bank0 = Bank::new(&GenesisBlock::new(10).0);
        let bank1 = Bank::new(&GenesisBlock::new(20).0);
        assert_ne!(bank0.hash_internal_state(), bank1.hash_internal_state());
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

    // create a ledger with a tick every `tick_interval` entries and a couple other transactions
    fn create_sample_block_with_ticks(
        genesis_block: &GenesisBlock,
        mint_keypair: &Keypair,
        num_one_token_transfers: usize,
        tick_interval: usize,
    ) -> impl Iterator<Item = Entry> {
        let mut entries = vec![];

        let mut last_id = genesis_block.last_id();

        // Start off the ledger with the psuedo-tick linked to the genesis block
        // (see entry0 in `process_ledger`)
        let tick = Entry::new(&genesis_block.last_id(), 0, 1, vec![]);
        let mut hash = tick.id;
        entries.push(tick);

        for i in 0..num_one_token_transfers {
            // Transfer one token from the mint to a random account
            let keypair = Keypair::new();
            let tx = SystemTransaction::new_account(mint_keypair, keypair.pubkey(), 1, last_id, 0);
            let entry = Entry::new(&hash, 0, 1, vec![tx]);
            hash = entry.id;
            entries.push(entry);

            // Add a second Transaction that will produce a
            // ProgramError<0, ResultWithNegativeTokens> error when processed
            let keypair2 = Keypair::new();
            let tx = SystemTransaction::new_account(&keypair, keypair2.pubkey(), 42, last_id, 0);
            let entry = Entry::new(&hash, 0, 1, vec![tx]);
            hash = entry.id;
            entries.push(entry);

            if (i + 1) % tick_interval == 0 {
                let tick = Entry::new(&hash, 0, 1, vec![]);
                hash = tick.id;
                last_id = hash;
                entries.push(tick);
            }
        }
        entries.into_iter()
    }

    fn create_sample_ledger(
        tokens: u64,
        num_one_token_transfers: usize,
    ) -> (GenesisBlock, Keypair, impl Iterator<Item = Entry>) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(tokens);
        let block = create_sample_block_with_ticks(
            &genesis_block,
            &mint_keypair,
            num_one_token_transfers,
            num_one_token_transfers,
        );
        (genesis_block, mint_keypair, block)
    }

    #[test]
    fn test_process_ledger_simple() {
        let (genesis_block, mint_keypair, ledger) = create_sample_ledger(100, 3);
        let bank = Bank::new(&genesis_block);
        assert_eq!(bank.tick_height(), 0);
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100);
        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::default()));
        let (ledger_height, last_id) = process_ledger(&bank, ledger, &leader_scheduler).unwrap();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100 - 3);
        assert_eq!(ledger_height, 8);
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
