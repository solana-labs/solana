use crate::args::{BalancesArgs, DistributeTokensArgs, StakeArgs, TransactionLogArgs};
use crate::db::{self, TransactionInfo};
use console::style;
use csv::{ReaderBuilder, Trim};
use indexmap::IndexMap;
use indicatif::{ProgressBar, ProgressStyle};
use pickledb::PickleDb;
use serde::{Deserialize, Serialize};
use solana_banks_client::{BanksClient, BanksClientExt};
use solana_sdk::{
    commitment_config::CommitmentLevel,
    message::Message,
    native_token::{lamports_to_sol, sol_to_lamports},
    signature::{unique_signers, Signature, Signer},
    system_instruction,
    transaction::Transaction,
    transport::{self, TransportError},
};
use solana_stake_program::{
    stake_instruction,
    stake_state::{Authorized, Lockup, StakeAuthorize},
};
use std::{
    cmp::{self},
    io,
    time::Duration,
};
use tokio::time::delay_for;

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

async fn transfer<S: Signer>(
    client: &mut BanksClient,
    lamports: u64,
    sender_keypair: &S,
    to_pubkey: &Pubkey,
) -> io::Result<Transaction> {
    let create_instruction =
        system_instruction::transfer(&sender_keypair.pubkey(), &to_pubkey, lamports);
    let message = Message::new(&[create_instruction], Some(&sender_keypair.pubkey()));
    let recent_blockhash = client.get_recent_blockhash().await?;
    Ok(Transaction::new(
        &[sender_keypair],
        message,
        recent_blockhash,
    ))
}

async fn distribute_tokens(
    client: &mut BanksClient,
    db: &mut PickleDb,
    allocations: &[Allocation],
    args: &DistributeTokensArgs,
) -> Result<(), Error> {
    for allocation in allocations {
        let new_stake_account_keypair = Keypair::new();
        let new_stake_account_address = new_stake_account_keypair.pubkey();

        let mut signers = vec![&*args.fee_payer, &*args.sender_keypair];
        if let Some(stake_args) = &args.stake_args {
            signers.push(&*stake_args.stake_authority);
            signers.push(&*stake_args.withdraw_authority);
            signers.push(&new_stake_account_keypair);
        }
        let signers = unique_signers(signers);

        println!("{:<44}  {:>24.9}", allocation.recipient, allocation.amount);
        let instructions = if let Some(stake_args) = &args.stake_args {
            let sol_for_fees = stake_args.sol_for_fees;
            let sender_pubkey = args.sender_keypair.pubkey();
            let stake_authority = stake_args.stake_authority.pubkey();
            let withdraw_authority = stake_args.withdraw_authority.pubkey();

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
            let from = args.sender_keypair.pubkey();
            let to = allocation.recipient.parse().unwrap();
            let lamports = sol_to_lamports(allocation.amount);
            let instruction = system_instruction::transfer(&from, &to, lamports);
            vec![instruction]
        };

        let fee_payer_pubkey = args.fee_payer.pubkey();
        let message = Message::new(&instructions, Some(&fee_payer_pubkey));
        let result: transport::Result<(Transaction, u64)> = {
            if args.dry_run {
                Ok((Transaction::new_unsigned(message), std::u64::MAX))
            } else {
                let (_fee_calculator, blockhash, last_valid_slot) = client.get_fees().await?;
                let transaction = Transaction::new(&signers, message, blockhash);
                client.send_transaction(transaction.clone()).await?;
                Ok((transaction, last_valid_slot))
            }
        };
        match result {
            Ok((transaction, last_valid_slot)) => {
                db::set_transaction_info(
                    db,
                    &allocation.recipient.parse().unwrap(),
                    allocation.amount,
                    &transaction,
                    Some(&new_stake_account_address),
                    false,
                    last_valid_slot,
                )?;
            }
            Err(e) => {
                eprintln!("Error sending tokens to {}: {}", allocation.recipient, e);
            }
        };
    }
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

pub async fn process_distribute_tokens(
    client: &mut BanksClient,
    args: &DistributeTokensArgs,
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

    let mut db = db::open_db(&args.transaction_db, args.dry_run)?;

    // Start by finalizing any transactions from the previous run.
    let confirmations = finalize_transactions(client, &mut db, args.dry_run).await?;

    let transaction_infos = db::read_transaction_infos(&db);
    apply_previous_transactions(&mut allocations, &transaction_infos);

    if allocations.is_empty() {
        eprintln!("No work to do");
        return Ok(confirmations);
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

    distribute_tokens(client, &mut db, &allocations, args).await?;

    let opt_confirmations = finalize_transactions(client, &mut db, args.dry_run).await?;
    Ok(opt_confirmations)
}

async fn finalize_transactions(
    client: &mut BanksClient,
    db: &mut PickleDb,
    dry_run: bool,
) -> Result<Option<usize>, Error> {
    if dry_run {
        return Ok(None);
    }

    let mut opt_confirmations = update_finalized_transactions(client, db).await?;

    let progress_bar = new_spinner_progress_bar();

    while opt_confirmations.is_some() {
        if let Some(confirmations) = opt_confirmations {
            progress_bar.set_message(&format!(
                "[{}/{}] Finalizing transactions",
                confirmations, 32,
            ));
        }

        // Sleep for about 1 slot
        delay_for(Duration::from_millis(500)).await;
        let opt_conf = update_finalized_transactions(client, db).await?;
        opt_confirmations = opt_conf;
    }

    Ok(opt_confirmations)
}

// Update the finalized bit on any transactions that are now rooted
// Return the lowest number of confirmations on the unfinalized transactions or None if all are finalized.
async fn update_finalized_transactions(
    client: &mut BanksClient,
    db: &mut PickleDb,
) -> Result<Option<usize>, Error> {
    let transaction_infos = db::read_transaction_infos(db);
    let unconfirmed_transactions: Vec<_> = transaction_infos
        .iter()
        .filter_map(|info| {
            if info.finalized_date.is_some() {
                None
            } else {
                Some((&info.transaction, info.last_valid_slot))
            }
        })
        .collect();
    let unconfirmed_signatures: Vec<_> = unconfirmed_transactions
        .iter()
        .map(|(tx, _slot)| tx.signatures[0])
        .filter(|sig| *sig != Signature::default()) // Filter out dry-run signatures
        .collect();
    let transaction_statuses = client
        .get_transaction_statuses(unconfirmed_signatures)
        .await?;
    let root_slot = client.get_root_slot().await?;

    let mut confirmations = None;
    for ((transaction, last_valid_slot), opt_transaction_status) in unconfirmed_transactions
        .into_iter()
        .zip(transaction_statuses.into_iter())
    {
        match db::update_finalized_transaction(
            db,
            &transaction.signatures[0],
            opt_transaction_status,
            last_valid_slot,
            root_slot,
        ) {
            Ok(Some(confs)) => {
                confirmations = Some(cmp::min(confs, confirmations.unwrap_or(usize::MAX)));
            }
            result => {
                result?;
            }
        }
    }
    Ok(confirmations)
}

pub async fn process_balances(
    client: &mut BanksClient,
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
        let actual = lamports_to_sol(client.get_balance(address).await.unwrap());
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
    let db = db::open_db(&args.transaction_db, true)?;
    db::write_transaction_log(&db, &args.output_path)?;
    Ok(())
}

use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use tempfile::{tempdir, NamedTempFile};
pub async fn test_process_distribute_tokens_with_client(
    client: &mut BanksClient,
    sender_keypair: Keypair,
) {
    let fee_payer = Keypair::new();
    let transaction = transfer(
        client,
        sol_to_lamports(1.0),
        &sender_keypair,
        &fee_payer.pubkey(),
    )
    .await
    .unwrap();
    client
        .process_transaction_with_commitment(transaction, CommitmentLevel::Recent)
        .await
        .unwrap();
    assert_eq!(
        client
            .get_balance_with_commitment(fee_payer.pubkey(), CommitmentLevel::Recent)
            .await
            .unwrap(),
        sol_to_lamports(1.0),
    );

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
    let transaction_db = dir
        .path()
        .join("transactions.db")
        .to_str()
        .unwrap()
        .to_string();

    let args = DistributeTokensArgs {
        sender_keypair: Box::new(sender_keypair),
        fee_payer: Box::new(fee_payer),
        dry_run: false,
        input_csv,
        from_bids: false,
        transaction_db: transaction_db.clone(),
        dollars_per_sol: None,
        stake_args: None,
    };
    let confirmations = process_distribute_tokens(client, &args).await.unwrap();
    assert_eq!(confirmations, None);

    let transaction_infos =
        db::read_transaction_infos(&db::open_db(&transaction_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        client.get_balance(alice_pubkey).await.unwrap(),
        expected_amount,
    );

    // Now, run it again, and check there's no double-spend.
    process_distribute_tokens(client, &args).await.unwrap();
    let transaction_infos =
        db::read_transaction_infos(&db::open_db(&transaction_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        client.get_balance(alice_pubkey).await.unwrap(),
        expected_amount,
    );
}

pub async fn test_process_distribute_stake_with_client(
    client: &mut BanksClient,
    sender_keypair: Keypair,
) {
    let fee_payer = Keypair::new();
    let transaction = transfer(
        client,
        sol_to_lamports(1.0),
        &sender_keypair,
        &fee_payer.pubkey(),
    )
    .await
    .unwrap();
    client
        .process_transaction_with_commitment(transaction, CommitmentLevel::Recent)
        .await
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
    let message = Message::new(&instructions, Some(&sender_keypair.pubkey()));
    let signers = [&sender_keypair, &stake_account_keypair];
    let blockhash = client.get_recent_blockhash().await.unwrap();
    let transaction = Transaction::new(&signers, message, blockhash);
    client
        .process_transaction_with_commitment(transaction, CommitmentLevel::Recent)
        .await
        .unwrap();

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
    let transaction_db = dir
        .path()
        .join("transactions.db")
        .to_str()
        .unwrap()
        .to_string();

    let stake_args = StakeArgs {
        stake_account_address,
        stake_authority: Box::new(stake_authority),
        withdraw_authority: Box::new(withdraw_authority),
        sol_for_fees: 1.0,
    };
    let args = DistributeTokensArgs {
        fee_payer: Box::new(fee_payer),
        dry_run: false,
        input_csv,
        transaction_db: transaction_db.clone(),
        stake_args: Some(stake_args),
        from_bids: false,
        sender_keypair: Box::new(sender_keypair),
        dollars_per_sol: None,
    };
    let confirmations = process_distribute_tokens(client, &args).await.unwrap();
    assert_eq!(confirmations, None);

    let transaction_infos =
        db::read_transaction_infos(&db::open_db(&transaction_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        client.get_balance(alice_pubkey).await.unwrap(),
        sol_to_lamports(1.0),
    );
    let new_stake_account_address = transaction_infos[0].new_stake_account_address.unwrap();
    assert_eq!(
        client.get_balance(new_stake_account_address).await.unwrap(),
        expected_amount - sol_to_lamports(1.0),
    );

    // Now, run it again, and check there's no double-spend.
    process_distribute_tokens(client, &args).await.unwrap();
    let transaction_infos =
        db::read_transaction_infos(&db::open_db(&transaction_db, true).unwrap());
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey);
    let expected_amount = sol_to_lamports(allocation.amount);
    assert_eq!(
        sol_to_lamports(transaction_infos[0].amount),
        expected_amount
    );

    assert_eq!(
        client.get_balance(alice_pubkey).await.unwrap(),
        sol_to_lamports(1.0),
    );
    assert_eq!(
        client.get_balance(new_stake_account_address).await.unwrap(),
        expected_amount - sol_to_lamports(1.0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_banks_client::start_client;
    use solana_banks_server::banks_server::start_local_server;
    use solana_runtime::{bank::Bank, bank_forks::BankForks};
    use solana_sdk::genesis_config::create_genesis_config;
    use std::sync::{Arc, RwLock};
    use tokio::runtime::Runtime;

    #[test]
    fn test_process_distribute_tokens() {
        let (genesis_config, sender_keypair) = create_genesis_config(sol_to_lamports(9_000_000.0));
        let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::new(&genesis_config))));
        Runtime::new().unwrap().block_on(async {
            let transport = start_local_server(&bank_forks).await;
            let mut banks_client = start_client(transport).await.unwrap();
            test_process_distribute_tokens_with_client(&mut banks_client, sender_keypair).await;
        });
    }

    #[test]
    fn test_process_distribute_stake() {
        let (genesis_config, sender_keypair) = create_genesis_config(sol_to_lamports(9_000_000.0));
        let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::new(&genesis_config))));
        Runtime::new().unwrap().block_on(async {
            let transport = start_local_server(&bank_forks).await;
            let mut banks_client = start_client(transport).await.unwrap();
            test_process_distribute_stake_with_client(&mut banks_client, sender_keypair).await;
        });
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
            ..TransactionInfo::default()
        }];
        apply_previous_transactions(&mut allocations, &transaction_infos);
        assert_eq!(allocations.len(), 1);

        // Ensure that we applied the transaction to the allocation with
        // a matching recipient address (to bob, not alice).
        assert_eq!(allocations[0].recipient, alice.to_string());
    }
}
