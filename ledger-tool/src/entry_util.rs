use {
    rayon::iter::IntoParallelIterator,
    solana_accounts_db::accounts::AccountLocks,
    solana_entry::entry::Entry,
    solana_runtime::bank::Bank,
    solana_sdk::{
        transaction::{
            Result, TransactionAccountLocks,
            TransactionVerificationMode,
        },
    },
};

// get AccountLocks for entire entry; Tick entry has empty AccountLocks;
pub fn get_entry_account_locks(
    entry: &Entry,
    bank: &Bank,
    ) -> AccountLocks {
    // use bank.verify_transaction() to get SanitizedTransactions
    // Tick are those transaction.is_empty()

    let verification_mode = TransactionVerificationMode::HashAndVerifyPrecompiles;
    let mut account_locks = AccountLocks::default();
    if !entry.transactions.is_empty() {
        let tx_account_lock_limit = bank.get_transaction_account_lock_limit();

        let sanitized_transactions = entry
            .transactions
            .iter() //into_par_iter()
            .filter_map(|versioned_tx| {
                match bank.verify_transaction(versioned_tx.clone(), verification_mode) {
                    Ok(sanitized_tx) => Some(sanitized_tx),
                    Err(err) => {println!("ERROR: versioned tx {:?} failed to sanitize: {:?}", versioned_tx, err); None},
                }
            })
            .collect::<Vec<_>>();

        sanitized_transactions
            .iter()
            .for_each(|sanitized_tx| {
                match sanitized_tx.get_account_locks(tx_account_lock_limit) {
                    Ok(transaction_account_locks) => {
                        add_to_entry_account_locks(&mut account_locks, &transaction_account_locks);
                    },
                    Err(err) => {
                        println!("ERROR: failed to get account locks for tx {:?}: {:?}", sanitized_tx, err);
                    },
                 }
            });
    }

    account_locks
}

fn add_to_entry_account_locks(
    account_locks: &mut AccountLocks,
    tx_account_locks: &TransactionAccountLocks,
    ) {
        tx_account_locks.writable
            .iter()
            .for_each(|w| {
                // just to double check that transactions of same entry shall not conflict
                if account_locks.is_locked_write(w) || account_locks.is_locked_readonly(w) {
                    println!("ERROR: writable account {:?} conflicts within entry!", w);
                } else {
                    account_locks.write_locks.insert(**w);
                }
            });

        tx_account_locks.readonly
            .iter()
            .for_each(|r| {
                // just to double check that transactions of same entry shall not conflict
                if account_locks.is_locked_write(r) {
                    println!("ERROR: readonly account {:?} conflicts within entry!", r);
                } else if !account_locks.lock_readonly(r) {
                    account_locks.insert_new_readonly(*r);
                }
            });
}
/*
        // now can scan to determine entry dependencies. Something like this: 
        // entry 0, [] 
        // entry 1, [] <-- no dep
        // entry 2, [0]
        // entry 3, [2, 1] <-- doesn't need to check against 0 since 0 is implied by 2
        let entry_dependencies = entry_accounts_locks.iter().enumerate().for_each(|(entry_index, accounts_locks)| {
            if entry_index == 0 { vec![] } else {
                for entries in [entry_index-1..0].filter_map(|earlier_entry_index| {if accounts.locks conflicts with accounts_locks(ealier_entry_idnex) {Some(earlieir_entry_index)} els e{None} })
                also note/print EntryType::Tick()
            }
        })
        .collect();
*/
pub fn has_account_lock_conflict(
    lh: &[Result<TransactionAccountLocks>],
    rh: &[Result<TransactionAccountLocks>],
    ) -> bool {
    // if any rh.write in either lh.write or lh.read, return true
    // if any rh.read in lh.write, return true
    // otherwise return false
    false
}

pub fn print_entry_account_locks(
    entry_index: usize,
    account_locks: &AccountLocks,
    ) {
    println!("Entry {entry_index} account locks:");
    println!("== writable accounts:");
    account_locks.write_locks.iter().for_each(|k| {
        println!("== {:?}", k);
    });
    println!("-- readonly accounts:");
    account_locks.readonly_locks.iter().for_each(|(k, count)| {
        println!("-- {:?}, {:?}", k, count);
    });
}

