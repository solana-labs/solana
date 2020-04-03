use crate::args::DistributeArgs;
use crate::thin_client::{Client, ThinClient};
use console::style;
use csv::{ReaderBuilder, Trim};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    message::Message,
    native_token::sol_to_lamports,
    signature::{Signature, Signer},
    system_instruction,
    transport::TransportError,
};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Bid {
    bid_amount_dollars: f64,
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
            if allocation.amount >= amount {
                allocation.amount -= amount;
                break;
            } else {
                amount -= allocation.amount;
                allocation.amount = 0.0;
            }
        }
    }
    allocations.retain(|x| x.amount > 0.0);
}

fn create_allocation(bid: &Bid, dollars_per_sol: f64) -> Allocation {
    Allocation {
        recipient: bid.primary_address.clone(),
        amount: bid.bid_amount_dollars / dollars_per_sol,
    }
}

fn distribute_tokens<T: Client>(
    client: &ThinClient<T>,
    allocations: &[Allocation],
    args: &DistributeArgs<Box<dyn Signer>>,
) -> Vec<Result<Signature, TransportError>> {
    let fee_payer_pubkey = args.fee_payer.as_ref().unwrap().pubkey();
    let messages: Vec<Message> = allocations
        .iter()
        .map(|allocation| {
            println!("{:<44}  {}", allocation.recipient, allocation.amount);
            let from = args.sender_keypair.as_ref().unwrap().pubkey();
            let to = allocation.recipient.parse().unwrap();
            let lamports = sol_to_lamports(allocation.amount);
            let instruction = system_instruction::transfer(&from, &to, lamports);
            Message::new_with_payer(&[instruction], Some(&fee_payer_pubkey))
        })
        .collect();

    let signers = vec![
        &**args.sender_keypair.as_ref().unwrap(),
        &**args.fee_payer.as_ref().unwrap(),
    ];

    messages
        .into_iter()
        .map(|message| client.send_message(message, &signers))
        .collect()
}

fn read_transaction_infos(path: &str) -> Vec<TransactionInfo> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&path)
        .unwrap();
    rdr.deserialize().map(|x| x.unwrap()).collect()
}

fn append_transaction_infos(
    allocations: &[Allocation],
    results: &[Result<Signature, TransportError>],
    transactions_csv: &str,
) -> Result<(), csv::Error> {
    let existed = Path::new(&transactions_csv).exists();
    if existed {
        let transactions_bak = format!("{}.bak", &transactions_csv);
        fs::copy(&transactions_csv, transactions_bak)?;
    }
    let file = fs::OpenOptions::new()
        .create_new(!existed)
        .write(true)
        .append(existed)
        .open(&transactions_csv)?;
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(!existed)
        .from_writer(file);

    for (i, allocation) in allocations.iter().enumerate() {
        match &results[i] {
            Ok(signature) => {
                let transaction_info = TransactionInfo {
                    recipient: allocation.recipient.clone(),
                    amount: allocation.amount,
                    signature: signature.to_string(),
                };
                wtr.serialize(transaction_info)?;
            }
            Err(e) => {
                eprintln!("Error sending tokens to {}: {}", allocation.recipient, e);
            }
        }
    }
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
    let allocations: Vec<Allocation> = rdr
        .deserialize()
        .map(|bid| create_allocation(&bid.unwrap(), args.dollars_per_sol))
        .collect();

    let transaction_infos: Vec<TransactionInfo> = if Path::new(&args.transactions_csv).exists() {
        read_transaction_infos(&args.transactions_csv)
    } else {
        vec![]
    };
    let mut allocations = merge_allocations(&allocations);
    apply_previous_transactions(&mut allocations, &transaction_infos);

    if allocations.is_empty() {
        eprintln!("No work to do");
        return Ok(());
    }

    println!(
        "{}",
        style(format!("{:<44}  {}", "Recipient", "Amount")).bold()
    );

    let results = distribute_tokens(&client, &allocations, &args);
    if !args.dry_run {
        append_transaction_infos(&allocations, &results, &args.transactions_csv)?;
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
        bid_amount_dollars: 1000.0,
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
    let expected_amount = bid.bid_amount_dollars / args.dollars_per_sol;
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
    let expected_amount = bid.bid_amount_dollars / args.dollars_per_sol;
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
}
