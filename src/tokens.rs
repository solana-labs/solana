use crate::args::{BalancesArgs, DistributeArgs};
use crate::thin_client::{Client, ThinClient};
use console::style;
use csv::{ReaderBuilder, Trim};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    message::Message,
    native_token::{lamports_to_sol, sol_to_lamports},
    signature::{Signature, Signer},
    system_instruction,
};
use std::fs;
use std::path::Path;
use std::process;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Bid {
    accepted_amount_dollars: f64,
    primary_address: String,
}

#[derive(Debug, Clone)]
struct Allocation {
    recipient: String,
    amount: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct TransactionInfo {
    recipient: String,
    amount: f64,
    signature: String,
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
    allocations: &[Allocation],
    args: &DistributeArgs<Box<dyn Signer>>,
) -> Result<(), csv::Error> {
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
            client.send_message(message, &signers)
        };
        match result {
            Ok(signature) => {
                println!("Finalized transaction with signature {}", signature);
                if !args.dry_run {
                    append_transaction_info(&allocation, &signature, &args.transactions_csv)?;
                }
            }
            Err(e) => {
                eprintln!("Error sending tokens to {}: {}", allocation.recipient, e);
            }
        };
    }
    Ok(())
}

fn read_transaction_infos(path: &str) -> Vec<TransactionInfo> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&path)
        .unwrap();
    rdr.deserialize().map(|x| x.unwrap()).collect()
}

fn append_transaction_info(
    allocation: &Allocation,
    signature: &Signature,
    transactions_csv: &str,
) -> Result<(), csv::Error> {
    let existed = Path::new(&transactions_csv).exists();
    let file = fs::OpenOptions::new()
        .create_new(!existed)
        .write(true)
        .append(existed)
        .open(&transactions_csv)?;
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(!existed)
        .from_writer(file);

    let transaction_info = TransactionInfo {
        recipient: allocation.recipient.clone(),
        amount: allocation.amount,
        signature: signature.to_string(),
    };
    wtr.serialize(transaction_info)?;
    wtr.flush()?;
    Ok(())
}

pub fn process_distribute<T: Client>(
    client: &ThinClient<T>,
    args: &DistributeArgs<Box<dyn Signer>>,
) -> Result<(), csv::Error> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&args.allocations_csv)?;
    let bids: Vec<Bid> = rdr.deserialize().map(|bid| bid.unwrap()).collect();
    let allocations: Vec<Allocation> = bids
        .into_iter()
        .map(|bid| create_allocation(&bid, args.dollars_per_sol))
        .collect();

    let transaction_infos: Vec<TransactionInfo> = if Path::new(&args.transactions_csv).exists() {
        read_transaction_infos(&args.transactions_csv)
    } else {
        vec![]
    };

    let starting_total_tokens: f64 = allocations.iter().map(|x| x.amount).sum();
    println!(
        "{} ◎{} (${})",
        style(format!("{}", "Total in allocations_csv:")).bold(),
        starting_total_tokens,
        starting_total_tokens * args.dollars_per_sol,
    );

    let mut allocations = merge_allocations(&allocations);
    apply_previous_transactions(&mut allocations, &transaction_infos);

    let distributed_tokens: f64 = transaction_infos.iter().map(|x| x.amount).sum();
    let undistributed_tokens: f64 = allocations.iter().map(|x| x.amount).sum();
    println!(
        "{} ◎{} (${})",
        style(format!("{}", "Distributed:")).bold(),
        distributed_tokens,
        distributed_tokens * args.dollars_per_sol,
    );
    println!(
        "{} ◎{} (${})",
        style(format!("{}", "Undistributed:")).bold(),
        undistributed_tokens,
        undistributed_tokens * args.dollars_per_sol,
    );
    println!(
        "{} ◎{} (${})\n",
        style(format!("{}", "Total:")).bold(),
        distributed_tokens + undistributed_tokens,
        (distributed_tokens + undistributed_tokens) * args.dollars_per_sol,
    );

    if allocations.is_empty() {
        eprintln!("No work to do");
        return Ok(());
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
        if balance != 0 {
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

    distribute_tokens(&client, &allocations, &args)?;

    Ok(())
}

pub fn process_balances<T: Client>(
    client: &ThinClient<T>,
    args: &BalancesArgs,
) -> Result<(), csv::Error> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&args.allocations_csv)?;
    let bids: Vec<Bid> = rdr.deserialize().map(|bid| bid.unwrap()).collect();
    let allocations: Vec<Allocation> = bids
        .into_iter()
        .map(|bid| create_allocation(&bid, args.dollars_per_sol))
        .collect();
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
pub fn test_process_distribute_with_client<C: Client>(
    thin_client: &ThinClient<C>,
    sender_keypair: Keypair,
) {
    let fee_payer = Keypair::new();
    thin_client
        .transfer(sol_to_lamports(1.0), &sender_keypair, &fee_payer.pubkey())
        .unwrap();

    let alice_pubkey = Pubkey::new_rand();
    let bid = Bid {
        primary_address: alice_pubkey.to_string(),
        accepted_amount_dollars: 1000.0,
    };
    let allocations_file = NamedTempFile::new().unwrap();
    let allocations_csv = allocations_file.path().to_str().unwrap().to_string();
    let mut wtr = csv::WriterBuilder::new().from_writer(allocations_file);
    wtr.serialize(&bid).unwrap();
    wtr.flush().unwrap();

    let dir = tempdir().unwrap();
    let transactions_csv = dir
        .path()
        .join("transactions.csv")
        .to_str()
        .unwrap()
        .to_string();

    let args: DistributeArgs<Box<dyn Signer>> = DistributeArgs {
        sender_keypair: Some(Box::new(sender_keypair)),
        fee_payer: Some(Box::new(fee_payer)),
        dry_run: false,
        allocations_csv,
        transactions_csv: transactions_csv.clone(),
        dollars_per_sol: 0.22,
    };
    process_distribute(&thin_client, &args).unwrap();
    let transaction_infos = read_transaction_infos(&transactions_csv);
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey.to_string());
    let expected_amount = bid.accepted_amount_dollars / args.dollars_per_sol;
    assert_eq!(transaction_infos[0].amount, expected_amount);

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        sol_to_lamports(expected_amount),
    );

    // Now, run it again, and check there's no double-spend.
    process_distribute(&thin_client, &args).unwrap();
    let transaction_infos = read_transaction_infos(&transactions_csv);
    assert_eq!(transaction_infos.len(), 1);
    assert_eq!(transaction_infos[0].recipient, alice_pubkey.to_string());
    let expected_amount = bid.accepted_amount_dollars / args.dollars_per_sol;
    assert_eq!(transaction_infos[0].amount, expected_amount);

    assert_eq!(
        thin_client.get_balance(&alice_pubkey).unwrap(),
        sol_to_lamports(expected_amount),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_runtime::{bank::Bank, bank_client::BankClient};
    use solana_sdk::genesis_config::create_genesis_config;

    #[test]
    fn test_process_distribute() {
        let (genesis_config, sender_keypair) = create_genesis_config(sol_to_lamports(9_000_000.0));
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);
        let thin_client = ThinClient(bank_client);
        test_process_distribute_with_client(&thin_client, sender_keypair);
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
            signature: "".to_string(),
        }];
        apply_previous_transactions(&mut allocations, &transaction_infos);
        assert_eq!(allocations.len(), 1);

        // Ensure that we applied the transaction to the allocation with
        // a matching recipient address (to "b", not "a").
        assert_eq!(allocations[0].recipient, "a");
    }
}
