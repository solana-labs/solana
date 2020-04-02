mod arg_parser;
mod args;

use crate::arg_parser::parse_args;
use crate::args::{resolve_command, Command, ScrubArgs, TransferArgs};
use console::style;
use csv::{ReaderBuilder, Trim};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
//use solana_cli_config::Config;
//use solana_client::client_error::ClientError;
//use solana_client::rpc_client::RpcClient;
//use solana_sdk::{
//    message::Message, pubkey::Pubkey, signature::Signature, signers::Signers,
//    transaction::Transaction,
//};
use solana_sdk::signature::Signature;
use std::env;
use std::error::Error;

#[derive(Deserialize, Debug)]
struct Purchase {
    bid_amount_dollars: f64,
    primary_address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Transfer {
    recipient: String,
    amount: f64,
    signature: Option<Signature>,
}

const CLEARING_PRICE: f64 = 0.22;

fn scrub(purchases: &[Purchase]) -> Vec<Transfer> {
    purchases
        .iter()
        .group_by(|x| &x.primary_address)
        .into_iter()
        .map(|(pubkey, xs)| {
            let dollars: f64 = xs.map(|x| x.bid_amount_dollars).sum();
            Transfer {
                recipient: pubkey.clone(),
                amount: dollars / CLEARING_PRICE,
                signature: None,
            }
        })
        .collect()
}

fn process_scrub(args: &ScrubArgs) -> Result<(), csv::Error> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&args.input_csv)?;
    let purchases: Vec<Purchase> = rdr.deserialize().map(|x| x.unwrap()).collect();

    let mut wtr = csv::Writer::from_writer(io::stdout());
    for transfer in scrub(&purchases) {
        wtr.serialize(transfer)?;
    }
    wtr.flush()?;
    Ok(())
}

fn find_new_transfers(transfers: &[Transfer], state: &[Transfer]) -> Vec<Transfer> {
    let mut needed = vec![];
    for transfer in transfers {
        if let Some(prev_transfer) = state.iter().find(|x| x.recipient == transfer.recipient) {
            if transfer.amount > prev_transfer.amount {
                needed.push(Transfer {
                    recipient: transfer.recipient.clone(),
                    amount: transfer.amount - prev_transfer.amount,
                    signature: None,
                });
            }
        } else {
            needed.push(transfer.clone());
        }
    }
    needed
}

fn process_transfer(args: &TransferArgs) -> Result<(), csv::Error> {
    let mut rdr = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(&args.input_csv)?;
    let transfers: Vec<Transfer> = rdr.deserialize().map(|x| x.unwrap()).collect();

    let state: Vec<Transfer> = if Path::new(&args.state_csv).exists() {
        let mut state_rdr = ReaderBuilder::new()
            .trim(Trim::All)
            .from_path(&args.state_csv)?;
        state_rdr.deserialize().map(|x| x.unwrap()).collect()
    } else {
        vec![]
    };

    let needed = find_new_transfers(&transfers, &state);

    println!(
        "{}",
        style(format!("{:<44}  {}", "Recipient", "Amount")).bold()
    );
    for transfer in &needed {
        println!("{:<44}  {}", transfer.recipient, transfer.amount);
    }

    if !args.dry_run && !needed.is_empty() {
        let state_bak = format!("{}.bak", &args.state_csv);
        fs::rename(&args.state_csv, state_bak)?;
        let mut wtr = csv::Writer::from_path(&args.state_csv)?;
        for transfer in &transfers {
            wtr.serialize(transfer)?;
        }
        wtr.flush()?;
    }

    Ok(())
}

//fn send_message<S: Signers>(
//    client: &RpcClient,
//    message: Message,
//    signers: &S,
//) -> Result<Signature, ClientError> {
//    let mut transaction = Transaction::new_unsigned(message);
//    client.resign_transaction(&mut transaction, signers)?;
//    client.send_and_confirm_transaction_with_spinner(&mut transaction, signers)
//}

fn main() -> Result<(), Box<dyn Error>> {
    let command_args = parse_args(env::args_os());
    //let config = Config::load(&command_args.config_file)?;
    //let json_rpc_url = command_args.url.unwrap_or(config.json_rpc_url);
    //let client = RpcClient::new(json_rpc_url);

    match resolve_command(&command_args.command)? {
        Command::Scrub(args) => {
            process_scrub(&args)?;
        }
        Command::Transfer(args) => {
            process_transfer(&args)?;
        }
    }
    Ok(())
}
