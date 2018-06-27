extern crate atty;
extern crate bincode;
extern crate env_logger;
extern crate getopts;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use bincode::serialize;
use getopts::Options;
use solana::crdt::{get_ip_addr, ReplicatedData};
use solana::drone::DroneRequest;
use solana::mint::Mint;
use solana::thin_client::ThinClient;
use std::env;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: {} [options]\n\n", program);
    brief += "  solana-wallet allows you to perform basic actions, including\n";
    brief += "  requesting an airdrop, checking your balance, and spending tokens.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() -> io::Result<()> {
    env_logger::init();
    if is(Stream::Stdin) {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = io::stdin().read_to_string(&mut buffer).unwrap();
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    let id: Mint = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });

    let mut opts = Options::new();
    opts.optopt("l", "", "leader", "leader.json");
    opts.optopt("c", "", "client port", "port");
    opts.optflag("d", "dyn", "detect network address dynamically");
    opts.optflag("h", "help", "print help");
    let args: Vec<String> = env::args().collect();
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    if matches.opt_present("h") {
        let program = args[0].clone();
        print_usage(&program, opts);
        return Ok(());
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

    let mut client = mk_client(&client_addr, &leader)?;
    let mut drone_addr = leader.transactions_addr.clone();
    drone_addr.set_port(9900);

    // Start the a, generate a random client keypair, and show user possible commands
    display_actions();

    loop {
        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(_) => {
                match input.trim() {
                    // Check client balance
                    "balance" => {
                        println!("Balance requested...");
                        let balance = client.poll_get_balance(&id.pubkey());
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
                    "airdrop" => {
                        println!("Airdrop requested...");
                        let _airdrop = request_airdrop(&drone_addr, &id);
                        // TODO: return airdrop Result from Drone
                        sleep(Duration::from_millis(100));
                        println!(
                            "Your balance is: {:?}",
                            client.poll_get_balance(&id.pubkey()).unwrap()
                        );
                    }
                    // If client has positive balance, spend tokens in {balance} number of transactions
                    "pay" => {
                        let last_id = client.get_last_id();
                        let balance = client.poll_get_balance(&id.pubkey());
                        match balance {
                            Ok(0) => {
                                println!("You don't have any tokens!");
                            }
                            Ok(balance) => {
                                println!("Sending {:?} tokens to self...", balance);
                                let sig =
                                    client.transfer(balance, &id.keypair(), id.pubkey(), &last_id);
                                println!("Sent transaction! Signature: {:?}", sig);
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::Other => {
                                println!("No account found! Request an airdrop to get started.");
                            }
                            Err(error) => {
                                println!("An error occurred: {:?}", error);
                            }
                        }
                    }
                    _ => {
                        println!("Command {:?} not recognized", input.trim());
                    }
                }
                display_actions();
            }
            Err(error) => println!("error: {}", error),
        }
    }
}

fn display_actions() {
    println!("");
    println!("What would you like to do? Type a command:");
    println!("  `balance` - Get your account balance");
    println!("  `airdrop` - Request a batch of 50 tokens");
    println!("  `pay` - Spend your tokens as fast as possible");
    println!("");
}

fn read_leader(path: String) -> ReplicatedData {
    let file = File::open(path.clone()).expect(&format!("file not found: {}", path));
    serde_json::from_reader(file).expect(&format!("failed to parse {}", path))
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
