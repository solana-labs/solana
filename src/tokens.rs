use crate::args::{BalancesArgs, DistributeStakeArgs, DistributeTokensArgs};
use crate::thin_client::{Client, ThinClient};
use console::style;
use csv::{ReaderBuilder, Trim};
use indexmap::IndexMap;
use pickledb::{PickleDb, PickleDbDumpPolicy};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    message::Message,
    native_token::{lamports_to_sol, sol_to_lamports},
    signature::{Signature, Signer},
    system_instruction,
    transaction::Transaction,
    transport::TransportError,
};
use solana_stake_program::{
    stake_instruction,
    stake_state::{Authorized, Lockup, StakeAuthorize},
};
use solana_transaction_status::TransactionStatus;
use std::{cmp, path::Path, process};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Bid {
    accepted_amount_dollars: f64,
    primary_address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Allocation {
    recipient: String,
    amount: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
struct TransactionInfo {
    recipient: String,
    amount: f64,
    new_stake_account_address: String,
    finalized: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("CSV error")]
    CsvError(#[from] csv::Error),
    #[error("PickleDb error")]
    PickleDbError(#[from] pickledb::error::Error),
    #[error("Transport error")]
    TransportError(#[from] TransportError),
}

fn merge_allocations(allocations: &[Allocation]) -> Vec<Allocation> {
    let mut allocation_map = IndexMap::new();
    for allocation in allocations {
        allocation_map
            .entry(&allocation.recipient)
            .or_insert(Allocation {
                recipient: allocation.recipient.clone(),
                amount: 0.0,
            })
            .amount += allocation.amount;
    }
    allocation_map.values().cloned().collect()
}

fn apply_previous_transactions(
    allocations: &mut Vec<Allocation>,
    transaction_infos: &[TransactionInfo],
) {
    for transaction_info in transaction_infos {
        let mut amount = transaction_info.amount;
        for allocation in allocations.iter_mut() {
            if allocation.recipient != transaction_info.recipient {
                continue;
            }
            if allocation.amount >= amount {
                allocation.amount -= amount;
                break;
            } else {
                amount -= allocation.amount;
                allocation.amount = 0.0;
            }
        }
    }
    allocations.retain(|x| x.amount > 0.5);
}

fn create_allocation(bid: &Bid, dollars_per_sol: f64) -> Allocation {
    Allocation {
        recipient: bid.primary_address.clone(),
        amount: bid.accepted_amount_dollars / dollars_per_sol,
    }
}

fn distribute_tokens<T: Client>(
    client: &ThinClient<T>,
    db: &mut PickleDb,
    allocations: &[Allocation],
    args: &DistributeTokensArgs<Box<dyn Signer>>,
) -> Result<(), Error> {
    let signers = if args.dry_run {
        vec![]
    } else {
        let mut signers = vec![&**args.sender_keypair.as_ref().unwrap()];
        if args.sender_keypair != args.fee_payer {
            signers.push(&**args.fee_payer.as_ref().unwrap());
        }
        signers
    };

    for allocation in allocations {
        println!("{:<44}  {:>24.9}", allocation.recipient, allocation.amount);
        let result = if args.dry_run {
            Ok(Signature::default())
        } else {
            let fee_payer_pubkey = args.fee_payer.as_ref().unwrap().pubkey();
            let from = args.sender_keypair.as_ref().unwrap().pubkey();
            let to = allocation.recipient.parse().unwrap();
            let lamports = sol_to_lamports(allocation.amount);
            let instruction = system_instruction::transfer(&from, &to, lamports);
            let message = Message::new_with_payer(&[instruction], Some(&fee_payer_pubkey));
            let (blockhash, _fee_caluclator) = client.get_recent_blockhash()?;
            let transaction = Transaction::new(&signers, message, blockhash);
            let signature = transaction.signatures[0];
            set_transaction_info(db, &allocation, &signature, None, false)?;
            if args.no_wait {
                client.async_send_transaction(transaction)
            } else {
                client.send_transaction(transaction)
            }
        };
        match result {
            Ok(signature) => {
                println!("Finalized transaction with signature {}", signature);
                if !args.no_wait {
                    set_transaction_info(db, &allocation, &signature, None, true)?;
                }
            }
            Err(e) => {
                eprintln!("Error sending tokens to {}: {}", allocation.recipient, e);
            }
        };
    }
    Ok(())
}

fn distribute_stake<T: Client>(
    client: &ThinClient<T>,
    db: &mut PickleDb,
    allocations: &[Allocation],
    args: &DistributeStakeArgs<Pubkey, Box<dyn Signer>>,
) -> Result<(), Error> {
    for allocation in allocations {
        let new_stake_account_keypair = Keypair::new();
        let new_stake_account_address = new_stake_account_keypair.pubkey();
        let signers = if args.dry_run {
            vec![]
        } else {
            vec![
                &**args.fee_payer.as_ref().unwrap(),
                &**args.stake_authority.as_ref().unwrap(),
                &**args.withdraw_authority.as_ref().unwrap(),
                &new_stake_account_keypair,
            ]
        };

        println!("{:<44}  {:>24.9}", allocation.recipient, allocation.amount);
        let result = if args.dry_run {
            Ok(Signature::default())
        } else {
            let system_sol = args.sol_for_fees;
            let fee_payer_pubkey = args.fee_payer.as_ref().unwrap().pubkey();
            let stake_authority = args.stake_authority.as_ref().unwrap().pubkey();
            let withdraw_authority = args.withdraw_authority.as_ref().unwrap().pubkey();

            let mut instructions = stake_instruction::split(
                &args.stake_account_address,
                &stake_authority,
                sol_to_lamports(allocation.amount - system_sol),
                &new_stake_account_address,
            );

            let recipient = allocation.recipient.parse().unwrap();

            // Make the recipient the new stake authority
            instructions.push(stake_instruction::authorize(
                &new_stake_account_address,
                &stake_authority,
                &recipient,
                StakeAuthorize::Staker,
            ));

            // Make the recipient the new withdraw authority
            instructions.push(stake_instruction::authorize(
                &new_stake_account_address,
                &withdraw_authority,
                &recipient,
                StakeAuthorize::Withdrawer,
            ));

            instructions.push(system_instruction::transfer(
                &fee_payer_pubkey, // Should this be a sender keypair?
                &recipient,
                sol_to_lamports(system_sol),
            ));

            let message = Message::new_with_payer(&instructions, Some(&fee_payer_pubkey));
            let (blockhash, _fee_caluclator) = client.get_recent_blockhash()?;
            let transaction = Transaction::new(&signers, message, blockhash);
            let signature = transaction.signatures[0];
            set_transaction_info(
                db,
                &allocation,
                &signature,
                Some(&new_stake_account_address),
                false,
            )?;
            if args.no_wait {
                client.async_send_transaction(transaction)
            } else {
                client.send_transaction(transaction)
            }
        };
        match result {
            Ok(signature) => {
                println!("Finalized transaction with signature {}", signature);
                if !args.no_wait {
                    set_transaction_info(
                        db,
                        &allocation,
                        &signature,
                        Some(&new_stake_account_address),
                        true,
                    )?;
                }
            }
            Err(e) => {
                eprintln!("Error sending tokens to {}: {}", allocation.recipient, e);
            }
        };
    }
    Ok(())
}

fn open_db(path: &str, dry_run: bool) -> Result<PickleDb, pickledb::error::Error> {
    let policy = if dry_run {
        PickleDbDumpPolicy::NeverDump
    } else {
        PickleDbDumpPolicy::AutoDump
    };
    if Path::new(path).exists() {
        PickleDb::load_yaml(path, policy)
    } else {
        Ok(PickleDb::new_yaml(path, policy))
    }
}

fn read_transaction_data(db: &PickleDb) -> Vec<(Signature, TransactionInfo)> {
    db.iter()
        .map(|kv| {
            (
                kv.get_key().parse().unwrap(),
                kv.get_value::<TransactionInfo>().unwrap(),
            )
        })
        .collect()
}

fn read_transaction_infos(db: &PickleDb) -> Vec<TransactionInfo> {
    db.iter()
        .map(|kv| kv.get_value::<TransactionInfo>().unwrap())
        .collect()
}

fn set_transaction_info(
    db: &mut PickleDb,
    allocation: &Allocation,
    signature: &Signature,
    new_stake_account_address: Option<&Pubkey>,
    finalized: bool,
) -> Result<(), pickledb::error::Error> {
    let transaction_info = TransactionInfo {
        recipient: allocation.recipient.clone(),
        amount: allocation.amount,
        new_stake_account_address: new_stake_account_address
            .map(|pubkey| pubkey.to_string())
            .unwrap_or_else(|| "".to_string()),
        finalized,
    };
    db.set(&signature.to_string(), &transaction_info)?;
    Ok(())
}

fn read_allocations(
    input_csv: &str,
    from_bids: bool,
    dollars_per_sol: Option<f64>,
) -> Vec<Allocation> {
    let rdr = ReaderBuilder::new().trim(Trim::All).from_path(input_csv);
    if from_bids {
        let bids: Vec<Bid> = rdr.unwrap().deserialize().map(|bid| bid.unwrap()).collect();
        bids.into_iter()
            .map(|bid| create_allocation(&bid, dollars_per_sol.unwrap()))
            .collect()
    } else {
        rdr.unwrap()
            .deserialize()
            .map(|entry| entry.unwrap())
            .collect()
    }
}

pub fn process_distribute_tokens<T: Client>(
    client: &ThinClient<T>,
    args: &DistributeTokensArgs<Box<dyn Signer>>,
) -> Result<Option<usize>, Error> {
    let mut allocations: Vec<Allocation> =
        read_allocations(&args.input_csv, args.from_bids, args.dollars_per_sol);

    let starting_total_tokens: f64 = allocations.iter().map(|x| x.amount).sum();
    println!(
        "{} ◎{}",
        style("Total in allocations_csv:").bold(),
        starting_total_tokens,
    );
    if let Some(dollars_per_sol) = args.dollars_per_sol {
        println!(
            "{} ${}",
            style("Total in allocations_csv:").bold(),
            starting_total_tokens * dollars_per_sol,
        );
    }

    let mut db = open_db(&args.transactions_db, args.dry_run)?;
    let confirmations = update_finalized_transactions(client, &mut db)?;
    if confirmations.is_some() {
        eprintln!("warning: unfinalized transactions");
    }

    let transaction_infos = read_transaction_infos(&db);
    apply_previous_transactions(&mut allocations, &transaction_infos);

    if allocations.is_empty() {
        eprintln!("No work to do");
        return Ok(confirmations);
    }

    // Sanity check: the recipient should not have tokens yet. If they do, it
    // is probably because:
    //  1. The signature couldn't be found in a previous run, though the transaction was
    //     successful. If so, manually add a row to the transaction log.
    //  2. The recipient already has tokens. If so, update this code to include a `--force` flag.
    //  3. The recipient correctly got tokens in a previous run, and then later registered the same
    //     address for another bid. If so, update this code to check for that case.
    for allocation in &allocations {
        let address = allocation.recipient.parse().unwrap();
        let balance = client.get_balance(&address).unwrap();
        if !args.force && balance != 0 {
            eprintln!(
                "Error: Non-zero balance {}, refusing to send {} to {}",
                lamports_to_sol(balance),
                allocation.amount,
                allocation.recipient,
            );
            process::exit(1);
        }
    }

    println!(
        "{}",
        style(format!(
            "{:<44}  {:>24}",
            "Recipient", "Expected Balance (◎)"
        ))
        .bold()
    );

    let distributed_tokens: f64 = transaction_infos.iter().map(|x| x.amount).sum();
    let undistributed_tokens: f64 = allocations.iter().map(|x| x.amount).sum();
    println!("{} ◎{}", style("Distributed:").bold(), distributed_tokens,);
    if let Some(dollars_per_sol) = args.dollars_per_sol {
        println!(
            "{} ${}",
            style("Distributed:").bold(),
            distributed_tokens * dollars_per_sol,
        );
    }
    println!(
        "{} ◎{}",
        style("Undistributed:").bold(),
        undistributed_tokens,
    );
    if let Some(dollars_per_sol) = args.dollars_per_sol {
        println!(
            "{} ${}",
            style("Undistributed:").bold(),
            undistributed_tokens * dollars_per_sol,
        );
    }
    println!(
        "{} ◎{}",
        style("Total:").bold(),
        distributed_tokens + undistributed_tokens,
    );
    if let Some(dollars_per_sol) = args.dollars_per_sol {
        println!(
            "{} ${}",
            style("Total:").bold(),
            (distributed_tokens + undistributed_tokens) * dollars_per_sol,
        );
    }

    distribute_tokens(client, &mut db, &allocations, args)?;

    let confirmations = update_finalized_transactions(client, &mut db)?;
    Ok(confirmations)
}

// Set the finalized bit in the database if the transaction is rooted.
// Remove the TransactionInfo from the database if the transaction failed.
// Return the number of confirmations on the transaction or None if finalized.
fn update_finalized_transaction(
    db: &mut PickleDb,
    signature: &Signature,
    opt_transaction_status: Option<TransactionStatus>,
) -> Result<Option<usize>, pickledb::error::Error> {
    if opt_transaction_status.is_none() {
        eprintln!(
            "Signature not found {}. If its blockhash is expired, remove it from the database",
            signature
        );

        // Return true because the transaction might still be in flight and get accepted onto
        // the ledger.
        return Ok(Some(0));
    }
    let transaction_status = opt_transaction_status.unwrap();

    if let Some(confirmations) = transaction_status.confirmations {
        // The transaction was found but is not yet finalized.
        return Ok(Some(confirmations));
    }

    if let Err(e) = &transaction_status.status {
        // The transaction was finalized, but execution failed. Drop it.
        eprintln!(
            "Error in transaction with signature {}: {}",
            signature,
            e.to_string()
        );
        eprintln!("Discarding transaction record");
        db.rem(&signature.to_string())?;
        return Ok(None);
    }

    // Transaction is rooted. Set finalized in the database.
    let mut transaction_info = db.get::<TransactionInfo>(&signature.to_string()).unwrap();
    transaction_info.finalized = true;
    db.set(&signature.to_string(), &transaction_info)?;
    Ok(None)
}

// Update the finalized bit on any transactions that are now rooted
// Return the lowest number of confirmations on the unfinalized transactions or None if all are finalized.
fn update_finalized_transactions<T: Client>(
    client: &ThinClient<T>,
    db: &mut PickleDb,
) -> Result<Option<usize>, Error> {
    let transaction_data = read_transaction_data(db);
    let unconfirmed_signatures: Vec<_> = transaction_data
        .iter()
        .filter_map(|(signature, info)| {
            if info.finalized {
                None
            } else {
                Some(*signature)
            }
        })
        .collect();
    let transaction_statuses = client.get_signature_statuses(&unconfirmed_signatures)?;
    let mut confirmations = None;
    for (signature, opt_transaction_status) in unconfirmed_signatures
        .into_iter()
        .zip(transaction_statuses.into_iter())
    {
        if let Some(confs) = update_finalized_transaction(db, &signature, opt_transaction_status)? {
            confirmations = Some(cmp::min(confs, confirmations.unwrap_or(usize::MAX)));
        }
    }
    Ok(confirmations)
}

pub fn process_distribute_stake<T: Client>(
    client: &ThinClient<T>,
    args: &DistributeStakeArgs<Pubkey, Box<dyn Signer>>,
) -> Result<Option<usize>, Error> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&args.allocations_csv)?;
    let allocations: Vec<Allocation> = rdr
        .deserialize()
        .map(|allocation| allocation.unwrap())
        .collect();

    let mut db = open_db(&args.transactions_db, args.dry_run)?;
    let confirmations = update_finalized_transactions(client, &mut db)?;
    if confirmations.is_some() {
        eprintln!("warning: unfinalized transactions");
    }

    let mut allocations = merge_allocations(&allocations);
    let transaction_infos = read_transaction_infos(&db);
    apply_previous_transactions(&mut allocations, &transaction_infos);

    if allocations.is_empty() {
        eprintln!("No work to do");
        return Ok(confirmations);
    }

    distribute_stake(client, &mut db, &allocations, args)?;

    let confirmations = update_finalized_transactions(client, &mut db)?;
    Ok(confirmations)
}

pub fn process_balances<T: Client>(
    client: &ThinClient<T>,
    args: &BalancesArgs,
) -> Result<(), csv::Error> {
    let allocations: Vec<Allocation> =
        read_allocations(&args.input_csv, args.from_bids, args.dollars_per_sol);
    let allocations = merge_allocations(&allocations);

    println!(
        "{}",
        style(format!(
            "{:<44}  {:>24}  {:>24}  {:>24}",
            "Recipient", "Expected Balance (◎)", "Actual Balance (◎)", "Difference (◎)"
        ))
        .bold()
    );

    for allocation in &allocations {
        let address = allocation.recipient.parse().unwrap();
        let expected = lamports_to_sol(sol_to_lamports(allocation.amount));
        let actual = lamports_to_sol(client.get_balance(&address).unwrap());
        println!(
            "{:<44}  {:>24.9}  {:>24.9}  {:>24.9}",
            allocation.recipient,
            expected,
            actual,
            actual - expected
        );
    }

    Ok(())
}

use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use tempfile::{tempdir, NamedTempFile};
pub fn test_process_distribute_tokens_with_client<C: Client>(client: C, sender_keypair: Keypair) {
    let thin_client = ThinClient(client);
    let fee_payer = Keypair::new();
    thin_client
        .transfer(sol_to_lamports(1.0), &sender_keypair, &fee_payer.pubkey())
        .unwrap();

    let alice_pubkey = Pubkey::new_rand();
    let allocation = Allocation {
        recipient: alice_pubkey.to_string(),
        amount: 1000.0,
    };
    let allocations_file = NamedTempFile::new().unwrap();
    let input_csv = allocations_file.path().to_str().unwrap().to_string();
    let mut wtr = csv::WriterBuilder::new().from_writer(allocations_file);
    wtr.serialize(&allocation).unwrap();
    wtr.flush().unwrap();

    let dir = tempdir().unwrap();
    let transactions_db = dir
        .path()
        .join("transactions.db")
        .to_str()
        .unwrap()
        .to_string();

    let args: DistributeTokensArgs<Box<dyn Signer>> = DistributeTokensArgs {
        sender_keypair: Some(Box::new(sender_keypair)),
        fee_payer: Some(Box::new(fee_payer)),
        dry_run: false,
        no_wait: false,
        input_csv,
        from_bids: false,
        transactions_db: transactions_db.clone(),
        dollars_per_sol: None,
        force: false,
    };
    let confirmations = process_distribute_tokens(&thin_client, &args).unwrap();
    assert_eq!(confirmations, None);

    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey.to_string());
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        expected_amount,
    );

    // Now, run it again, and check there's no double-spend.
    process_distribute_tokens(&thin_client, &args).unwrap();
    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey.to_string());
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        expected_amount,
    );
}

pub fn test_process_distribute_stake_with_client<C: Client>(client: C, sender_keypair: Keypair) {
    let thin_client = ThinClient(client);
    let fee_payer = Keypair::new();
    thin_client
        .transfer(sol_to_lamports(1.0), &sender_keypair, &fee_payer.pubkey())
        .unwrap();

    // TODO: Create a stake account with lockups
    let stake_account_keypair = Keypair::new();
    let stake_account_address = stake_account_keypair.pubkey();
    let stake_authority = Keypair::new();
    let withdraw_authority = Keypair::new();

    let authorized = Authorized {
        staker: stake_authority.pubkey(),
        withdrawer: withdraw_authority.pubkey(),
    };
    let lockup = Lockup::default();
    let instructions = stake_instruction::create_account(
        &sender_keypair.pubkey(),
        &stake_account_address,
        &authorized,
        &lockup,
        sol_to_lamports(3000.0),
    );
    let message = Message::new(&instructions);
    let signers = [&sender_keypair, &stake_account_keypair];
    thin_client.send_message(message, &signers).unwrap();

    let alice_pubkey = Pubkey::new_rand();
    let allocation = Allocation {
        recipient: alice_pubkey.to_string(),
        amount: 1000.0,
    };
    let allocations_file = NamedTempFile::new().unwrap();
    let allocations_csv = allocations_file.path().to_str().unwrap().to_string();
    let mut wtr = csv::WriterBuilder::new().from_writer(allocations_file);
    wtr.serialize(&allocation).unwrap();
    wtr.flush().unwrap();

    let dir = tempdir().unwrap();
    let transactions_db = dir
        .path()
        .join("transactions.db")
        .to_str()
        .unwrap()
        .to_string();

    let args: DistributeStakeArgs<Pubkey, Box<dyn Signer>> = DistributeStakeArgs {
        stake_account_address,
        stake_authority: Some(Box::new(stake_authority)),
        withdraw_authority: Some(Box::new(withdraw_authority)),
        fee_payer: Some(Box::new(fee_payer)),
        dry_run: false,
        no_wait: false,
        sol_for_fees: 1.0,
        allocations_csv,
        transactions_db: transactions_db.clone(),
    };
    let confirmations = process_distribute_stake(&thin_client, &args).unwrap();
    assert_eq!(confirmations, None);

    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey.to_string());
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        sol_to_lamports(1.0),
    );
    let new_stake_account_address = transaction_infos[0]
        .new_stake_account_address
        .parse()
        .unwrap();
    assert_eq!(
        thin_client.get_balance(&new_stake_account_address).unwrap(),
        expected_amount - sol_to_lamports(1.0),
    );

    // Now, run it again, and check there's no double-spend.
    process_distribute_stake(&thin_client, &args).unwrap();
    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey.to_string());
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        sol_to_lamports(1.0),
    );
    assert_eq!(
        thin_client.get_balance(&new_stake_account_address).unwrap(),
        expected_amount - sol_to_lamports(1.0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_runtime::{bank::Bank, bank_client::BankClient};
    use solana_sdk::{genesis_config::create_genesis_config, transaction::TransactionError};

    #[test]
    fn test_process_distribute_tokens() {
        let (genesis_config, sender_keypair) = create_genesis_config(sol_to_lamports(9_000_000.0));
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);
        test_process_distribute_tokens_with_client(bank_client, sender_keypair);
    }

    #[test]
    fn test_process_distribute_stake() {
        let (genesis_config, sender_keypair) = create_genesis_config(sol_to_lamports(9_000_000.0));
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);
        test_process_distribute_stake_with_client(bank_client, sender_keypair);
    }

    #[test]
    fn test_read_allocations() {
        let alice_pubkey = Pubkey::new_rand();
        let allocation = Allocation {
            recipient: alice_pubkey.to_string(),
            amount: 42.0,
        };
        let file = NamedTempFile::new().unwrap();
        let input_csv = file.path().to_str().unwrap().to_string();
        let mut wtr = csv::WriterBuilder::new().from_writer(file);
        wtr.serialize(&allocation).unwrap();
        wtr.flush().unwrap();

        assert_eq!(read_allocations(&input_csv, false, None), vec![allocation]);
    }

    #[test]
    fn test_read_allocations_from_bids() {
        let alice_pubkey = Pubkey::new_rand();
        let bid = Bid {
            primary_address: alice_pubkey.to_string(),
            accepted_amount_dollars: 42.0,
        };
        let file = NamedTempFile::new().unwrap();
        let input_csv = file.path().to_str().unwrap().to_string();
        let mut wtr = csv::WriterBuilder::new().from_writer(file);
        wtr.serialize(&bid).unwrap();
        wtr.flush().unwrap();

        let allocation = Allocation {
            recipient: bid.primary_address,
            amount: 84.0,
        };
        assert_eq!(
            read_allocations(&input_csv, true, Some(0.5)),
            vec![allocation]
        );
    }

    #[test]
    fn test_apply_previous_transactions() {
        let mut allocations = vec![
            Allocation {
                recipient: "a".to_string(),
                amount: 1.0,
            },
            Allocation {
                recipient: "b".to_string(),
                amount: 1.0,
            },
        ];
        let transaction_infos = vec![TransactionInfo {
            recipient: "b".to_string(),
            amount: 1.0,
            new_stake_account_address: "".to_string(),
            finalized: true,
        }];
        apply_previous_transactions(&mut allocations, &transaction_infos);
        assert_eq!(allocations.len(), 1);

        // Ensure that we applied the transaction to the allocation with
        // a matching recipient address (to "b", not "a").
        assert_eq!(allocations[0].recipient, "a");
    }

    #[test]
    fn test_update_finalized_transaction_not_landed() {
        // Keep waiting for a transaction that hasn't landed yet.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        assert_eq!(
            update_finalized_transaction(&mut db, &signature, None).unwrap(),
            Some(0)
        );

        // Unchanged
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );
    }

    #[test]
    fn test_update_finalized_transaction_confirming() {
        // Keep waiting for a transaction that is still being confirmed.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: Some(1),
            status: Ok(()),
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(&mut db, &signature, Some(transaction_status)).unwrap(),
            Some(1)
        );

        // Unchanged
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );
    }

    #[test]
    fn test_update_finalized_transaction_failed() {
        // Don't wait if the transaction failed to execute.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let status = Err(TransactionError::AccountNotFound);
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status,
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(&mut db, &signature, Some(transaction_status)).unwrap(),
            None
        );

        // Ensure TransactionInfo has been purged.
        assert_eq!(db.get::<TransactionInfo>(&signature.to_string()), None);
    }

    #[test]
    fn test_update_finalized_transaction_finalized() {
        // Don't wait once the transaction has been finalized.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let mut transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status: Ok(()),
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(&mut db, &signature, Some(transaction_status)).unwrap(),
            None
        );

        transaction_info.finalized = true;
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );
    }
}
