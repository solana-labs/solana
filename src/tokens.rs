use crate::args::{BalancesArgs, DistributeTokensArgs, StakeArgs, TransactionLogArgs};
use crate::thin_client::{Client, ThinClient};
use console::style;
use csv::{ReaderBuilder, Trim};
use indexmap::IndexMap;
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use pickledb::{PickleDb, PickleDbDumpPolicy};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    hash::Hash,
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
use std::{cmp, io, path::Path, process, thread::sleep, time::Duration};

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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct TransactionInfo {
    recipient: Pubkey,
    amount: f64,
    new_stake_account_address: Option<Pubkey>,
    finalized: bool,
    transaction: Transaction,
}

impl Default for TransactionInfo {
    fn default() -> Self {
        let mut transaction = Transaction::new_unsigned_instructions(vec![]);
        transaction.signatures.push(Signature::default());
        Self {
            recipient: Pubkey::default(),
            amount: 0.0,
            new_stake_account_address: None,
            finalized: false,
            transaction,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct SignedTransactionInfo {
    recipient: String,
    amount: f64,
    new_stake_account_address: String,
    finalized: bool,
    signature: String,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error")]
    IoError(#[from] io::Error),
    #[error("CSV error")]
    CsvError(#[from] csv::Error),
    #[error("PickleDb error")]
    PickleDbError(#[from] pickledb::error::Error),
    #[error("Transport error")]
    TransportError(#[from] TransportError),
    #[error("Signature not found")]
    SignatureNotFound,
}

fn unique_signers(signers: Vec<&dyn Signer>) -> Vec<&dyn Signer> {
    signers.into_iter().unique_by(|s| s.pubkey()).collect_vec()
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
            if allocation.recipient != transaction_info.recipient.to_string() {
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
    args: &DistributeTokensArgs<Pubkey, Box<dyn Signer>>,
) -> Result<(), Error> {
    for allocation in allocations {
        let new_stake_account_keypair = Keypair::new();
        let new_stake_account_address = new_stake_account_keypair.pubkey();
        let signers = if args.dry_run {
            vec![]
        } else {
            let mut signers = vec![
                &**args.fee_payer.as_ref().unwrap(),
                &**args.sender_keypair.as_ref().unwrap(),
            ];
            if let Some(stake_args) = &args.stake_args {
                signers.push(&**stake_args.stake_authority.as_ref().unwrap());
                signers.push(&**stake_args.withdraw_authority.as_ref().unwrap());
                signers.push(&new_stake_account_keypair);
            }
            unique_signers(signers)
        };

        println!("{:<44}  {:>24.9}", allocation.recipient, allocation.amount);
        let result = if args.dry_run {
            Ok(Signature::default())
        } else {
            let instructions = if let Some(stake_args) = &args.stake_args {
                let sol_for_fees = stake_args.sol_for_fees;
                let sender_pubkey = args.sender_keypair.as_ref().unwrap().pubkey();
                let stake_authority = stake_args.stake_authority.as_ref().unwrap().pubkey();
                let withdraw_authority = stake_args.withdraw_authority.as_ref().unwrap().pubkey();

                let mut instructions = stake_instruction::split(
                    &stake_args.stake_account_address,
                    &stake_authority,
                    sol_to_lamports(allocation.amount - sol_for_fees),
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
                    &sender_pubkey,
                    &recipient,
                    sol_to_lamports(sol_for_fees),
                ));

                instructions
            } else {
                let from = args.sender_keypair.as_ref().unwrap().pubkey();
                let to = allocation.recipient.parse().unwrap();
                let lamports = sol_to_lamports(allocation.amount);
                let instruction = system_instruction::transfer(&from, &to, lamports);
                vec![instruction]
            };

            let fee_payer_pubkey = args.fee_payer.as_ref().unwrap().pubkey();
            let message = Message::new_with_payer(&instructions, Some(&fee_payer_pubkey));
            let (blockhash, _fee_caluclator) = client.get_recent_blockhash()?;
            let transaction = Transaction::new(&signers, message, blockhash);
            set_transaction_info(
                db,
                &allocation,
                &transaction,
                Some(&new_stake_account_address),
                false,
            )?;

            client.async_send_transaction(transaction)
        };
        if let Err(e) = result {
            eprintln!("Error sending tokens to {}: {}", allocation.recipient, e);
        }
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

pub fn write_transaction_log<P: AsRef<Path>>(db: &PickleDb, path: &P) -> Result<(), io::Error> {
    let mut wtr = csv::WriterBuilder::new().from_path(path).unwrap();
    for info in read_transaction_infos(db) {
        let signed_info = SignedTransactionInfo {
            recipient: info.recipient.to_string(),
            amount: info.amount,
            new_stake_account_address: info
                .new_stake_account_address
                .map(|x| x.to_string())
                .unwrap_or_else(|| "".to_string()),
            finalized: info.finalized,
            signature: info.transaction.signatures[0].to_string(),
        };
        wtr.serialize(&signed_info)?;
    }
    wtr.flush()
}

fn read_transaction_infos(db: &PickleDb) -> Vec<TransactionInfo> {
    db.iter()
        .map(|kv| kv.get_value::<TransactionInfo>().unwrap())
        .collect()
}

fn set_transaction_info(
    db: &mut PickleDb,
    allocation: &Allocation,
    transaction: &Transaction,
    new_stake_account_address: Option<&Pubkey>,
    finalized: bool,
) -> Result<(), pickledb::error::Error> {
    let transaction_info = TransactionInfo {
        recipient: allocation.recipient.parse().unwrap(),
        amount: allocation.amount,
        new_stake_account_address: new_stake_account_address.cloned(),
        finalized,
        transaction: transaction.clone(),
    };
    let signature = transaction.signatures[0];
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

fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

pub fn process_distribute_tokens<T: Client>(
    client: &ThinClient<T>,
    args: &DistributeTokensArgs<Pubkey, Box<dyn Signer>>,
) -> Result<Option<usize>, Error> {
    let mut allocations: Vec<Allocation> =
        read_allocations(&args.input_csv, args.from_bids, args.dollars_per_sol);

    let starting_total_tokens: f64 = allocations.iter().map(|x| x.amount).sum();
    println!(
        "{} ◎{}",
        style("Total in input_csv:").bold(),
        starting_total_tokens,
    );
    if let Some(dollars_per_sol) = args.dollars_per_sol {
        println!(
            "{} ${}",
            style("Total in input_csv:").bold(),
            starting_total_tokens * dollars_per_sol,
        );
    }

    let mut db = open_db(&args.transactions_db, args.dry_run)?;

    // Start by finalizing any transactions from the previous run.
    let confirmations = finalize_transactions(client, &mut db, args.dry_run, args.no_wait)?;

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
        if args.stake_args.is_none() && !args.force && balance != 0 {
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

    finalize_transactions(client, &mut db, args.dry_run, args.no_wait)
}

fn finalize_transactions<T: Client>(
    client: &ThinClient<T>,
    db: &mut PickleDb,
    dry_run: bool,
    no_wait: bool,
) -> Result<Option<usize>, Error> {
    let (mut opt_confirmations, mut unconfirmed_transactions) =
        update_finalized_transactions(client, db)?;

    if no_wait {
        return Ok(opt_confirmations);
    }

    let progress_bar = new_spinner_progress_bar();

    while opt_confirmations.is_some() || !unconfirmed_transactions.is_empty() {
        if let Some(confirmations) = opt_confirmations {
            progress_bar.set_message(&format!(
                "[{}/{}] Finalizing transactions",
                confirmations, 32,
            ));
        }

        if !dry_run {
            for transaction in unconfirmed_transactions {
                client.async_send_transaction(transaction)?;
            }
        }

        // Sleep for about 1 slot
        sleep(Duration::from_millis(500));
        let (opt_conf, unconfirmed) = update_finalized_transactions(client, db)?;
        opt_confirmations = opt_conf;
        unconfirmed_transactions = unconfirmed;
    }

    Ok(opt_confirmations)
}

// Set the finalized bit in the database if the transaction is rooted.
// Remove the TransactionInfo from the database if the transaction failed.
// Return the number of confirmations on the transaction or None if finalized.
fn update_finalized_transaction(
    db: &mut PickleDb,
    signature: &Signature,
    opt_transaction_status: Option<TransactionStatus>,
    blockhash: &Hash,
    recent_blockhashes: &[Hash],
) -> Result<Option<usize>, Error> {
    if opt_transaction_status.is_none() {
        if !recent_blockhashes.contains(blockhash) {
            eprintln!("Signature not found {} and blockhash expired", signature);
            eprintln!("Discarding transaction record");
            db.rem(&signature.to_string())?;
            return Ok(None);
        }

        // Return an error to signal the option resend the transaction.
        return Err(Error::SignatureNotFound);
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
// Also return the unconfirmed transactions with valid blockhashes.
fn update_finalized_transactions<T: Client>(
    client: &ThinClient<T>,
    db: &mut PickleDb,
) -> Result<(Option<usize>, Vec<Transaction>), Error> {
    let transaction_infos = read_transaction_infos(db);
    let unconfirmed_transactions: Vec<_> = transaction_infos
        .iter()
        .filter_map(|info| {
            if info.finalized {
                None
            } else {
                Some(&info.transaction)
            }
        })
        .collect();
    let unconfirmed_signatures = unconfirmed_transactions
        .iter()
        .map(|tx| tx.signatures[0])
        .collect_vec();
    let transaction_statuses = client.get_signature_statuses(&unconfirmed_signatures)?;
    let recent_blockhashes = client.get_recent_blockhashes()?;

    let mut confirmations = None;
    let mut mia_transactions = vec![];
    for (transaction, opt_transaction_status) in unconfirmed_transactions
        .into_iter()
        .zip(transaction_statuses.into_iter())
    {
        match update_finalized_transaction(
            db,
            &transaction.signatures[0],
            opt_transaction_status,
            &transaction.message.recent_blockhash,
            &recent_blockhashes,
        ) {
            Ok(Some(confs)) => {
                confirmations = Some(cmp::min(confs, confirmations.unwrap_or(usize::MAX)));
            }
            Err(Error::SignatureNotFound) => {
                mia_transactions.push(transaction.clone());
            }
            result => {
                result?;
            }
        }
    }
    Ok((confirmations, mia_transactions))
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

pub fn process_transaction_log(args: &TransactionLogArgs) -> Result<(), Error> {
    let db = open_db(&args.transactions_db, true)?;
    write_transaction_log(&db, &args.output_path)?;
    Ok(())
}

use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use tempfile::{tempdir, NamedTempFile};
pub fn test_process_distribute_tokens_with_client<C: Client>(client: C, sender_keypair: Keypair) {
    let thin_client = ThinClient::new(client);
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

    let args: DistributeTokensArgs<Pubkey, Box<dyn Signer>> = DistributeTokensArgs {
        sender_keypair: Some(Box::new(sender_keypair)),
        fee_payer: Some(Box::new(fee_payer)),
        dry_run: false,
        no_wait: false,
        input_csv,
        from_bids: false,
        transactions_db: transactions_db.clone(),
        dollars_per_sol: None,
        force: false,
        stake_args: None,
    };
    let confirmations = process_distribute_tokens(&thin_client, &args).unwrap();
    assert_eq!(confirmations, None);

    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
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
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
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
    let thin_client = ThinClient::new(client);
    let fee_payer = Keypair::new();
    thin_client
        .transfer(sol_to_lamports(1.0), &sender_keypair, &fee_payer.pubkey())
        .unwrap();

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
    let file = NamedTempFile::new().unwrap();
    let input_csv = file.path().to_str().unwrap().to_string();
    let mut wtr = csv::WriterBuilder::new().from_writer(file);
    wtr.serialize(&allocation).unwrap();
    wtr.flush().unwrap();

    let dir = tempdir().unwrap();
    let transactions_db = dir
        .path()
        .join("transactions.db")
        .to_str()
        .unwrap()
        .to_string();

    let stake_args: StakeArgs<Pubkey, Box<dyn Signer>> = StakeArgs {
        stake_account_address,
        stake_authority: Some(Box::new(stake_authority)),
        withdraw_authority: Some(Box::new(withdraw_authority)),
        sol_for_fees: 1.0,
    };
    let args: DistributeTokensArgs<Pubkey, Box<dyn Signer>> = DistributeTokensArgs {
        fee_payer: Some(Box::new(fee_payer)),
        dry_run: false,
        no_wait: false,
        input_csv,
        transactions_db: transactions_db.clone(),
        stake_args: Some(stake_args),
        force: false,
        from_bids: false,
        sender_keypair: Some(Box::new(sender_keypair)),
        dollars_per_sol: None,
    };
    let confirmations = process_distribute_tokens(&thin_client, &args).unwrap();
    assert_eq!(confirmations, None);

    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        sol_to_lamports(1.0),
    );
    let new_stake_account_address = transaction_infos[0].new_stake_account_address.unwrap();
    assert_eq!(
        thin_client.get_balance(&new_stake_account_address).unwrap(),
        expected_amount - sol_to_lamports(1.0),
    );

    // Now, run it again, and check there's no double-spend.
    process_distribute_tokens(&thin_client, &args).unwrap();
    let transaction_infos = read_transaction_infos(&open_db(&transactions_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
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
        let alice = Pubkey::new_rand();
        let bob = Pubkey::new_rand();
        let mut allocations = vec![
            Allocation {
                recipient: alice.to_string(),
                amount: 1.0,
            },
            Allocation {
                recipient: bob.to_string(),
                amount: 1.0,
            },
        ];
        let transaction_infos = vec![TransactionInfo {
            recipient: bob,
            amount: 1.0,
            new_stake_account_address: None,
            finalized: true,
            transaction: Transaction::new_unsigned_instructions(vec![]),
        }];
        apply_previous_transactions(&mut allocations, &transaction_infos);
        assert_eq!(allocations.len(), 1);

        // Ensure that we applied the transaction to the allocation with
        // a matching recipient address (to bob, not alice).
        assert_eq!(allocations[0].recipient, alice.to_string());
    }

    #[test]
    fn test_update_finalized_transaction_not_landed() {
        // Keep waiting for a transaction that hasn't landed yet.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let blockhash = Hash::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        assert!(matches!(
            update_finalized_transaction(&mut db, &signature, None, &blockhash, &[blockhash])
                .unwrap_err(),
            Error::SignatureNotFound
        ));

        // Unchanged
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );

        // Same as before, but now with an expired blockhash
        assert_eq!(
            update_finalized_transaction(&mut db, &signature, None, &blockhash, &[]).unwrap(),
            None
        );

        // Ensure TransactionInfo has been purged.
        assert_eq!(db.get::<TransactionInfo>(&signature.to_string()), None);
    }

    #[test]
    fn test_update_finalized_transaction_confirming() {
        // Keep waiting for a transaction that is still being confirmed.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let blockhash = Hash::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: Some(1),
            status: Ok(()),
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(
                &mut db,
                &signature,
                Some(transaction_status),
                &blockhash,
                &[blockhash]
            )
            .unwrap(),
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
        let blockhash = Hash::default();
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
            update_finalized_transaction(
                &mut db,
                &signature,
                Some(transaction_status),
                &blockhash,
                &[blockhash]
            )
            .unwrap(),
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
        let blockhash = Hash::default();
        let mut transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status: Ok(()),
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(
                &mut db,
                &signature,
                Some(transaction_status),
                &blockhash,
                &[blockhash]
            )
            .unwrap(),
            None
        );

        transaction_info.finalized = true;
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );
    }

    #[test]
    fn test_write_transaction_log() {
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();

        let csv_file = NamedTempFile::new().unwrap();
        write_transaction_log(&db, &csv_file).unwrap();

        let mut rdr = ReaderBuilder::new().trim(Trim::All).from_reader(csv_file);
        let signed_infos: Vec<SignedTransactionInfo> =
            rdr.deserialize().map(|entry| entry.unwrap()).collect();

        let signed_info = SignedTransactionInfo {
            recipient: Pubkey::default().to_string(),
            signature: Signature::default().to_string(),
            ..SignedTransactionInfo::default()
        };
        assert_eq!(signed_infos, vec![signed_info]);
    }
}
