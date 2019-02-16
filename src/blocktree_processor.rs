use crate::bank::{Bank, BankError, Result};
use crate::blocktree::Blocktree;
use crate::entry::{Entry, EntrySlice};
use itertools::Itertools;
use solana_sdk::hash::Hash;

pub const VERIFY_BLOCK_SIZE: usize = 16;

/// Process an ordered list of entries, populating a circular buffer "tail"
/// as we go.
fn process_block(bank: &Bank, entries: &[Entry]) -> Result<()> {
    for entry in entries {
        bank.process_entry(entry)?;
    }

    Ok(())
}

/// Starting from the genesis block, append the provided entries to the ledger verifying them
/// along the way.
fn process_ledger<I>(bank: &Bank, entries: I) -> Result<(u64, Hash)>
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

        process_block(bank, &block)?;

        last_entry_id = block.last().unwrap().id;
        entry_height += block.len() as u64;
    }
    Ok((entry_height, last_entry_id))
}

pub fn process_blocktree(bank: &Bank, blocktree: &Blocktree) -> Result<(u64, Hash)> {
    let entries = blocktree.read_ledger().expect("opening ledger");
    process_ledger(&bank, entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;

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
        let (ledger_height, last_id) = process_ledger(&bank, ledger).unwrap();
        assert_eq!(bank.get_balance(&mint_keypair.pubkey()), 100 - 3);
        assert_eq!(ledger_height, 8);
        assert_eq!(bank.tick_height(), 1);
        assert_eq!(bank.last_id(), last_id);
    }
}
