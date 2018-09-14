use bincode::{deserialize, serialize};
use bs58;
use clap::ArgMatches;
use crdt::NodeInfo;
use drone::DroneRequest;
use fullnode::Config;
use serde_json;
use signature::{Keypair, KeypairUtil, Pubkey, Signature};
use std::fs::File;
use std::io::prelude::*;
use std::io::{Error, ErrorKind, Write};
use std::mem::size_of;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::thread::sleep;
use std::time::Duration;
use std::{error, fmt, mem};
use thin_client::ThinClient;

pub enum WalletCommand {
    Address,
    Balance,
    AirDrop(i64),
    Pay(i64, Pubkey),
    Confirm(Signature),
}

#[derive(Debug, Clone)]
pub enum WalletError {
    CommandNotRecognized(String),
    BadParameter(String),
}

impl fmt::Display for WalletError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for WalletError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

pub struct WalletConfig {
    pub leader: NodeInfo,
    pub id: Keypair,
    pub drone_addr: SocketAddr,
    pub command: WalletCommand,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        let default_addr = socketaddr!(0, 8000);
        WalletConfig {
            leader: NodeInfo::new_with_socketaddr(&default_addr),
            id: Keypair::new(),
            drone_addr: default_addr,
            command: WalletCommand::Balance,
        }
    }
}

pub fn parse_command(
    pubkey: Pubkey,
    matches: &ArgMatches,
) -> Result<WalletCommand, Box<error::Error>> {
    let response = match matches.subcommand() {
        ("airdrop", Some(airdrop_matches)) => {
            let tokens = airdrop_matches.value_of("tokens").unwrap().parse()?;
            Ok(WalletCommand::AirDrop(tokens))
        }
        ("pay", Some(pay_matches)) => {
            let to = if pay_matches.is_present("to") {
                let pubkey_vec = bs58::decode(pay_matches.value_of("to").unwrap())
                    .into_vec()
                    .expect("base58-encoded public key");

                if pubkey_vec.len() != mem::size_of::<Pubkey>() {
                    eprintln!("{}", pay_matches.usage());
                    Err(WalletError::BadParameter("Invalid public key".to_string()))?;
                }
                Pubkey::new(&pubkey_vec)
            } else {
                pubkey
            };

            let tokens = pay_matches.value_of("tokens").unwrap().parse()?;

            Ok(WalletCommand::Pay(tokens, to))
        }
        ("confirm", Some(confirm_matches)) => {
            let signatures = bs58::decode(confirm_matches.value_of("signature").unwrap())
                .into_vec()
                .expect("base58-encoded signature");

            if signatures.len() == mem::size_of::<Signature>() {
                let signature = Signature::new(&signatures);
                Ok(WalletCommand::Confirm(signature))
            } else {
                eprintln!("{}", confirm_matches.usage());
                Err(WalletError::BadParameter("Invalid signature".to_string()))
            }
        }
        ("balance", Some(_balance_matches)) => Ok(WalletCommand::Balance),
        ("address", Some(_address_matches)) => Ok(WalletCommand::Address),
        ("", None) => {
            println!("{}", matches.usage());
            Err(WalletError::CommandNotRecognized(
                "no subcommand given".to_string(),
            ))
        }
        _ => unreachable!(),
    }?;
    Ok(response)
}

pub fn process_command(
    config: &WalletConfig,
    client: &mut ThinClient,
) -> Result<(), Box<error::Error>> {
    match config.command {
        // Check client balance
        WalletCommand::Address => {
            println!("{}", config.id.pubkey());
        }
        WalletCommand::Balance => {
            println!("Balance requested...");
            let balance = client.poll_get_balance(&config.id.pubkey());
            match balance {
                Ok(balance) => {
                    println!("Your balance is: {:?}", balance);
                }
                Err(ref e) if e.kind() == ErrorKind::Other => {
                    println!("No account found! Request an airdrop to get started.");
                }
                Err(error) => {
                    println!("An error occurred: {:?}", error);
                }
            }
        }
        // Request an airdrop from Solana Drone;
        // Request amount is set in request_airdrop function
        WalletCommand::AirDrop(tokens) => {
            println!(
                "Requesting airdrop of {:?} tokens from {}",
                tokens, config.drone_addr
            );
            let previous_balance = client.poll_get_balance(&config.id.pubkey()).unwrap_or(0);
            request_airdrop(&config.drone_addr, &config.id.pubkey(), tokens as u64)?;

            // TODO: return airdrop Result from Drone instead of polling the
            //       network
            let mut current_balance = previous_balance;
            for _ in 0..20 {
                sleep(Duration::from_millis(500));
                current_balance = client
                    .poll_get_balance(&config.id.pubkey())
                    .unwrap_or(previous_balance);

                if previous_balance != current_balance {
                    break;
                }
                println!(".");
            }
            println!("Your balance is: {:?}", current_balance);
            if current_balance - previous_balance != tokens {
                Err("Airdrop failed!")?;
            }
        }
        // If client has positive balance, spend tokens in {balance} number of transactions
        WalletCommand::Pay(tokens, to) => {
            let last_id = client.get_last_id();
            let signature = client.transfer(tokens, &config.id, to, &last_id)?;
            println!("{}", signature);
        }
        // Confirm the last client transaction by signature
        WalletCommand::Confirm(signature) => {
            if client.check_signature(&signature) {
                println!("Confirmed");
            } else {
                println!("Not found");
            }
        }
    }
    Ok(())
}

pub fn read_leader(path: &str) -> Result<Config, WalletError> {
    let file = File::open(path.to_string()).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Unable to open leader file: {}",
            err, path
        )))
    })?;

    serde_json::from_reader(file).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Failed to parse leader file: {}",
            err, path
        )))
    })
}

pub fn request_airdrop(
    drone_addr: &SocketAddr,
    id: &Pubkey,
    tokens: u64,
) -> Result<Signature, Error> {
    // TODO: make this async tokio client
    let mut stream = TcpStream::connect(drone_addr)?;
    let req = DroneRequest::GetAirdrop {
        airdrop_request_amount: tokens,
        client_pubkey: *id,
    };
    let tx = serialize(&req).expect("serialize drone request");
    stream.write_all(&tx)?;
    let mut buffer = [0; size_of::<Signature>()];
    stream
        .read_exact(&mut buffer)
        .or_else(|_| Err(Error::new(ErrorKind::Other, "Airdrop failed")))?;
    let signature: Signature = deserialize(&buffer).or_else(|err| {
        Err(Error::new(
            ErrorKind::Other,
            format!("deserialize signature in request_airdrop: {:?}", err),
        ))
    })?;
    // TODO: add timeout to this function, in case of unresponsive drone
    Ok(signature)
}

#[cfg(test)]
mod tests {}
