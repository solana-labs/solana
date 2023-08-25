use {
    solana_accounts_db::accounts::AccountLocks,
    solana_entry::entry::Entry,
    solana_runtime::bank::Bank,
    solana_sdk::transaction::{TransactionAccountLocks, TransactionVerificationMode},
};

// get AccountLocks for entire entry; Tick entry has empty AccountLocks;
pub fn get_entry_account_locks(entry: &Entry, bank: &Bank) -> AccountLocks {
    // use bank.verify_transaction() to get SanitizedTransactions
    // Tick are those transaction.is_empty()

    let verification_mode = TransactionVerificationMode::HashAndVerifyPrecompiles;
    let mut account_locks = AccountLocks::default();
    if !entry.transactions.is_empty() {
        let tx_account_lock_limit = bank.get_transaction_account_lock_limit();

        let sanitized_transactions = entry
            .transactions
            .iter()
            .filter_map(|versioned_tx| {
                match bank.verify_transaction(versioned_tx.clone(), verification_mode) {
                    Ok(sanitized_tx) => Some(sanitized_tx),
                    Err(err) => {
                        println!(
                            "ERROR: versioned tx {:?} failed to sanitize: {:?}",
                            versioned_tx, err
                        );
                        None
                    }
                }
            })
            .collect::<Vec<_>>();

        sanitized_transactions.iter().for_each(|sanitized_tx| {
            match sanitized_tx.get_account_locks(tx_account_lock_limit) {
                Ok(transaction_account_locks) => {
                    add_to_entry_account_locks(&mut account_locks, &transaction_account_locks);
                }
                Err(err) => {
                    println!(
                        "ERROR: failed to get account locks for tx {:?}: {:?}",
                        sanitized_tx, err
                    );
                }
            }
        });
    }

    account_locks
}

fn add_to_entry_account_locks(
    account_locks: &mut AccountLocks,
    tx_account_locks: &TransactionAccountLocks,
) {
    tx_account_locks.writable.iter().for_each(|w| {
        // just to double check that transactions of same entry shall not conflict
        if account_locks.is_locked_write(w) || account_locks.is_locked_readonly(w) {
            println!("ERROR: writable account {:?} conflicts within entry!", w);
        } else {
            account_locks.write_locks.insert(**w);
        }
    });

    tx_account_locks.readonly.iter().for_each(|r| {
        // just to double check that transactions of same entry shall not conflict
        if account_locks.is_locked_write(r) {
            println!("ERROR: readonly account {:?} conflicts within entry!", r);
        } else if !account_locks.lock_readonly(r) {
            account_locks.insert_new_readonly(*r);
        }
    });
}

pub fn does_entry_account_locks_conflict(lh: &AccountLocks, rh: &AccountLocks) -> bool {
    for k in rh.write_locks.iter() {
        if lh.is_locked_write(k) || lh.is_locked_readonly(k) {
            return true;
        }
    }

    for (k, _) in rh.readonly_locks.iter() {
        if lh.is_locked_write(k) {
            return true;
        }
    }

    false
}

// Returns a vec of same size of entries, None for EntryType::Tick, otherwise
// the entry's dependencies to ancestors.
pub fn collect_entry_account_locks_conflicts(
    entries_account_locks: &Vec<AccountLocks>,
) -> Vec<Option<Vec<usize>>> {
    let mut deps = vec![];
    for (entry_index, entry_account_locks) in entries_account_locks.iter().enumerate() {
        // build deps map for EntryType::Transactions only, which must have at least 1 write-lock
        // (payer account), otherwise insert None
        let entry_deps = if entry_account_locks.write_locks.len() < 1 {
            None
        } else {
            let mut entry_deps = vec![];
            for (ancestor_index, ancestor) in entries_account_locks[..entry_index]
                .iter()
                .enumerate()
                .rev()
            {
                if does_entry_account_locks_conflict(ancestor, entry_account_locks) {
                    entry_deps.push(ancestor_index);
                }
            }
            Some(entry_deps)
        };
        deps.push(entry_deps);
    }

    deps
}

pub fn print_entry_account_locks(entry_index: usize, account_locks: &AccountLocks) {
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
