extern crate atty;
extern crate bincode;
extern crate bs58;
extern crate env_logger;
extern crate getopts;
extern crate serde_json;
extern crate solana;

use bincode::serialize;
use getopts::{Matches, Options};
use solana::crdt::{get_ip_addr, ReplicatedData};
use solana::drone::DroneRequest;
use solana::mint::Mint;
use solana::signature::{PublicKey, Signature};
use solana::thin_client::ThinClient;
use std::env;
use std::error;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;

enum WalletCommand {
    Balance,
    AirDrop,
    Address,
    Pay(i64, PublicKey),
    Confirm(Signature),
}

#[derive(Debug, Clone)]
enum WalletError {
    CommandNotRecognized(String),
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

struct WalletConfig {
    leader: ReplicatedData,
    id: Mint,
    client_addr: SocketAddr,
    drone_addr: SocketAddr,
    command: WalletCommand,
}

impl Default for WalletConfig {
    fn default() -> WalletConfig {
        let default_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        WalletConfig {
            leader: ReplicatedData::new_leader(&default_addr.clone()),
            id: Mint::new(0),
            client_addr: default_addr.clone(),
            drone_addr: default_addr.clone(),
            command: WalletCommand::Balance,
        }
    }
}

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: {} [options]\n\n", program);
    brief += "  solana-wallet allows you to perform basic actions, including";
    brief += "  requesting an airdrop, checking your balance, and spending tokens.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
    display_actions();
}

fn parse_command(matches: &Matches) -> Result<WalletCommand, WalletError> {
    let input = &matches.free[0];
    match input.as_ref() {
        "address" => Ok(WalletCommand::Address),
        "balance" => Ok(WalletCommand::Balance),
        "airdrop" => Ok(WalletCommand::AirDrop),
        "pay" => {
            if matches.free.len() < 3 {
                eprintln!("No tokens and public key provided");
                exit(1);
            }
            let tokens = matches.free[1].parse().expect("parse integer");
            let pubkey_vec = bs58::decode(&matches.free[2])
                .into_vec()
                .expect("base58-encoded public key");
            let to = PublicKey::clone_from_slice(&pubkey_vec);
            Ok(WalletCommand::Pay(tokens, to))
        }
        "confirm" => {
            if matches.free.len() < 2 {
                eprintln!("No signature provided");
                exit(1);
            }
            let sig_vec = bs58::decode(&matches.free[1])
                .into_vec()
                .expect("base58-encoded signature");
            let sig = Signature::clone_from_slice(&sig_vec);
            Ok(WalletCommand::Confirm(sig))
        }
        _ => Err(WalletError::CommandNotRecognized(input.to_string())),
    }
}

fn parse_args(args: Vec<String>) -> Result<WalletConfig, Box<error::Error>> {
    let mut opts = Options::new();
    opts.optopt("l", "", "leader", "leader.json");
    opts.optopt("m", "", "mint", "mint.json");
    opts.optopt("c", "", "client port", "port");
    opts.optflag("d", "dyn", "detect network address dynamically");
    opts.optflag("h", "help", "print help");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };

    if matches.opt_present("h") || matches.free.len() < 1 {
        print_usage(&args[0], opts);
        return Ok(WalletConfig::default());
    }

    let mut client_addr: SocketAddr = "0.0.0.0:8100".parse().unwrap();
    if matches.opt_present("c") {
        let port = matches.opt_str("c").unwrap().parse().unwrap();
        client_addr.set_port(port);
    }

    if matches.opt_present("d") {
        client_addr.set_ip(get_ip_addr().unwrap());
    }

    let leader = if matches.opt_present("l") {
        read_leader(matches.opt_str("l").unwrap())
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        ReplicatedData::new_leader(&server_addr)
    };

    let id = if matches.opt_present("m") {
        read_mint(matches.opt_str("m").unwrap())?
    } else {
        eprintln!("No mint found!");
        exit(1);
    };

    let mut drone_addr = leader.transactions_addr.clone();
    drone_addr.set_port(9900);

    let command = parse_command(&matches)?;
    Ok(WalletConfig {
        leader,
        id,
        client_addr,
        drone_addr, // TODO: Add an option for this.
        command,
    })
}

fn process_command(
    config: &WalletConfig,
    client: &mut ThinClient,
) -> Result<(), Box<error::Error>> {
    match config.command {
        // Check client balance
        WalletCommand::Address => {
            println!("{}", bs58::encode(config.id.pubkey()).into_string());
        }
        WalletCommand::Balance => {
            println!("Balance requested...");
            let balance = client.poll_get_balance(&config.id.pubkey());
            match balance {
                Ok(balance) => {
                    println!("Your balance is: {:?}", balance);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Other => {
                    println!("No account found! Request an airdrop to get started.");
                }
                Err(error) => {
                    println!("An error occurred: {:?}", error);
                }
            }
        }
        // Request an airdrop from Solana Drone;
        // Request amount is set in request_airdrop function
        WalletCommand::AirDrop => {
            println!("Airdrop requested...");
            let _airdrop = request_airdrop(&config.drone_addr, &config.id);
            // TODO: return airdrop Result from Drone
            sleep(Duration::from_millis(100));
            println!(
                "Your balance is: {:?}",
                client.poll_get_balance(&config.id.pubkey()).unwrap()
            );
        }
        // If client has positive balance, spend tokens in {balance} number of transactions
        WalletCommand::Pay(tokens, to) => {
            let last_id = client.get_last_id();
            let sig = client.transfer(tokens, &config.id.keypair(), to, &last_id)?;
            println!("{}", bs58::encode(sig).into_string());
        }
        // Confirm the last client transaction by signature
        WalletCommand::Confirm(sig) => {
            if client.check_signature(&sig) {
                println!("Confirmed");
            } else {
                println!("Not found");
            }
        }
    }
    Ok(())
}

fn display_actions() {
    println!("");
    println!("Commands:");
    println!("  balance   Get your account balance");
    println!("  airdrop   Request a batch of tokens");
    println!("  pay       Spend your tokens as fast as possible");
    println!("  confirm   Confirm your last payment by signature");
    println!("");
}

fn read_leader(path: String) -> ReplicatedData {
    let file = File::open(path.clone()).expect(&format!("file not found: {}", path));
    serde_json::from_reader(file).expect(&format!("failed to parse {}", path))
}

fn read_mint(path: String) -> Result<Mint, Box<error::Error>> {
    let file = File::open(path.clone())?;
    let mint = serde_json::from_reader(file)?;
    Ok(mint)
}

fn mk_client(client_addr: &SocketAddr, r: &ReplicatedData) -> io::Result<ThinClient> {
    let mut addr = client_addr.clone();
    let port = addr.port();
    let transactions_socket = UdpSocket::bind(addr.clone())?;
    addr.set_port(port + 1);
    let requests_socket = UdpSocket::bind(addr.clone())?;
    requests_socket.set_read_timeout(Some(Duration::new(1, 0)))?;

    addr.set_port(port + 2);
    Ok(ThinClient::new(
        r.requests_addr,
        requests_socket,
        r.transactions_addr,
        transactions_socket,
    ))
}

fn request_airdrop(drone_addr: &SocketAddr, id: &Mint) {
    let mut stream = TcpStream::connect(drone_addr).unwrap();
    let req = DroneRequest::GetAirdrop {
        airdrop_request_amount: id.tokens as u64,
        client_public_key: id.pubkey(),
    };
    let tx = serialize(&req).expect("serialize drone request");
    stream.write_all(&tx).unwrap();
    // TODO: add timeout to this function, in case of unresponsive drone
}

fn main() -> Result<(), Box<error::Error>> {
    env_logger::init();
    let config = parse_args(env::args().collect())?;
    let mut client = mk_client(&config.client_addr, &config.leader)?;
    process_command(&config, &mut client)
}
