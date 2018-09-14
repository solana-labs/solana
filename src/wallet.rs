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

#[derive(Debug, PartialEq)]
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
            println!("{:?}", confirm_matches.value_of("signature").unwrap());
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
mod tests {
    use super::*;
    use bank::Bank;
    use clap::{App, Arg, SubCommand};
    use client::mk_client;
    use crdt::Node;
    use drone::run_local_drone;
    use fullnode::Fullnode;
    use ledger::LedgerWriter;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::net::UdpSocket;
    use std::sync::mpsc::channel;

    fn tmp_ledger(name: &str, mint: &Mint) -> String {
        use std::env;
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let keypair = Keypair::new();

        let path = format!("{}/tmp-ledger-{}-{}", out_dir, name, keypair.pubkey());

        let mut writer = LedgerWriter::open(&path, true).unwrap();
        writer.write_entries(mint.create_entries()).unwrap();

        path
    }

    #[test]
    fn test_parse_command() {
        let pubkey = Keypair::new().pubkey();
        let test_commands = App::new("test")
            .subcommand(
                SubCommand::with_name("airdrop")
                    .about("Request a batch of tokens")
                    .arg(
                        Arg::with_name("tokens")
                            .long("tokens")
                            .value_name("NUMBER")
                            .takes_value(true)
                            .required(true)
                            .help("The number of tokens to request"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("pay")
                    .about("Send a payment")
                    .arg(
                        Arg::with_name("tokens")
                            .long("tokens")
                            .value_name("NUMBER")
                            .takes_value(true)
                            .required(true)
                            .help("The number of tokens to send"),
                    )
                    .arg(
                        Arg::with_name("to")
                            .long("to")
                            .value_name("PUBKEY")
                            .takes_value(true)
                            .help("The pubkey of recipient"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("confirm")
                    .about("Confirm your payment by signature")
                    .arg(
                        Arg::with_name("signature")
                            .index(1)
                            .value_name("SIGNATURE")
                            .required(true)
                            .help("The transaction signature to confirm"),
                    ),
            )
            .subcommand(SubCommand::with_name("balance").about("Get your balance"))
            .subcommand(SubCommand::with_name("address").about("Get your public key"));

        let test_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "--tokens", "50"]);
        assert_eq!(
            parse_command(pubkey, &test_airdrop).unwrap(),
            WalletCommand::AirDrop(50)
        );
        let test_bad_airdrop = test_commands
            .clone()
            .get_matches_from(vec!["test", "airdrop", "--tokens", "notint"]);
        assert!(parse_command(pubkey, &test_bad_airdrop).is_err());

        let pubkey_string = format!("{}", pubkey);
        let test_pay = test_commands.clone().get_matches_from(vec![
            "test",
            "pay",
            "--tokens",
            "50",
            "--to",
            &pubkey_string,
        ]);
        assert_eq!(
            parse_command(pubkey, &test_pay).unwrap(),
            WalletCommand::Pay(50, pubkey)
        );
        let test_bad_pubkey = test_commands
            .clone()
            .get_matches_from(vec!["test", "pay", "--tokens", "50", "--to", "deadbeef"]);
        assert!(parse_command(pubkey, &test_bad_pubkey).is_err());

        let signature = Signature::new(&vec![1; 64]);
        let signature_string = format!("{:?}", signature);
        let test_confirm =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "confirm", &signature_string]);
        assert_eq!(
            parse_command(pubkey, &test_confirm).unwrap(),
            WalletCommand::Confirm(signature)
        );
        let test_bad_signature =
            test_commands.get_matches_from(vec!["test", "confirm", "deadbeef"]);
        assert!(parse_command(pubkey, &test_bad_signature).is_err());
    }
    #[test]
    fn test_process_command() {
        let leader_keypair = Keypair::new();
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let alice = Mint::new(10_000_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let leader_data1 = leader.info.clone();
        let ledger_path = tmp_ledger("thin_client", &alice);

        let mut config = WalletConfig::default();

        let _server = Fullnode::new_with_bank(
            leader_keypair,
            bank,
            0,
            &[],
            leader,
            None,
            Some(&ledger_path),
            false,
        );
        sleep(Duration::from_millis(200));

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), leader_data.contact_info.ncp, sender);
        config.drone_addr = receiver.recv().unwrap();
        config.leader = leader_data1;

        config.command = WalletCommand::AirDrop(50);
        let mut client = mk_client(&config.leader);
        assert!(process_command(&config, &mut client).is_ok());

        config.command = WalletCommand::Balance;
        assert!(process_command(&config, &mut client).is_ok());

        config.command = WalletCommand::Address;
        assert!(process_command(&config, &mut client).is_ok());

        config.command = WalletCommand::Pay(10, bob_pubkey);
        assert!(process_command(&config, &mut client).is_ok());

        config.command = WalletCommand::Balance;
        assert!(process_command(&config, &mut client).is_ok());
    }
    #[test]
    fn test_request_airdrop() {
        let leader_keypair = Keypair::new();
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let alice = Mint::new(10_000_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let ledger_path = tmp_ledger("thin_client", &alice);

        let _server = Fullnode::new_with_bank(
            leader_keypair,
            bank,
            0,
            &[],
            leader,
            None,
            Some(&ledger_path),
            false,
        );
        sleep(Duration::from_millis(200));

        let requests_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        let mut client = ThinClient::new(
            leader_data.contact_info.rpu,
            requests_socket,
            leader_data.contact_info.tpu,
            transactions_socket,
        );

        let (sender, receiver) = channel();
        run_local_drone(alice.keypair(), leader_data.contact_info.ncp, sender);
        let drone_addr = receiver.recv().unwrap();

        let signature = request_airdrop(&drone_addr, &bob_pubkey, 50);
        assert!(signature.is_ok());
        assert!(client.check_signature(&signature.unwrap()));
    }
}
