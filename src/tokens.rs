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

#[derive(Deserialize, Debug, Clone)]
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
    let messages: Vec<Message> = allocations
        .iter()
        .map(|allocation| {
            println!("{:<44}  {}", allocation.recipient, allocation.amount);
            let from = args.sender_keypair.as_ref().unwrap().pubkey();
            let to = allocation.recipient.parse().unwrap();
            let lamports = sol_to_lamports(allocation.amount);
            let instruction = system_instruction::transfer(&from, &to, lamports);
            Message::new(&[instruction])
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

pub(crate) fn process_distribute<T: Client>(
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
        let mut state_rdr = ReaderBuilder::new()
            .trim(Trim::All)
            .from_path(&args.transactions_csv)?;
        state_rdr.deserialize().map(|x| x.unwrap()).collect()
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
