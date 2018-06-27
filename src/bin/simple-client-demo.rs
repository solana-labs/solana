extern crate bincode;
extern crate env_logger;
extern crate getopts;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use bincode::serialize;
use getopts::Options;
use rayon::prelude::*;
use solana::crdt::{get_ip_addr, ReplicatedData};
use solana::drone::DroneRequest;
use solana::signature::{GenKeys, KeyPair, KeyPairUtil, PublicKey};
use solana::thin_client::ThinClient;
use solana::timing::{duration_as_ms, duration_as_s};
use solana::transaction::Transaction;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::process::exit;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn print_usage(program: &str, opts: Options) {
    //TODO: Edit this!!!!
    let mut brief = format!("Usage: {} [options]\n\n", program);
    brief += "  Solana simple client demo allows you to perform basic actions, including\n";
    brief += "  requesting an airdrop, checking your balance, and spending tokens.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() {
    env_logger::init();
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
        return;
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

    let mut client = mk_client(&client_addr, &leader);
    let mut drone_addr = leader.transactions_addr.clone();
    drone_addr.set_port(9900);

    // Start the demo, generate a random client keypair, and show user possible commands
    println!("Generating keypair...");
    let client_keypair = KeyPair::new();
    let client_pubkey = client_keypair.pubkey();
    println!("Your public key is: {:?}", client_pubkey);
    display_actions();

    loop {
        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(_) => {
                match input.trim() {
                    // Check client balance
                    "balance" => {
                        println!("Balance requested...");
                        let balance = client.poll_get_balance(&client_pubkey);
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
                        let _airdrop = request_airdrop(&drone_addr, &client_pubkey);
                        // TODO: return airdrop Result from Drone
                        sleep(Duration::from_millis(100));
                        println!(
                            "Your balance is: {:?}",
                            client.poll_get_balance(&client_pubkey).unwrap()
                        );
                    }
                    // If client has positive balance, spend tokens in {balance} number of transactions
                    "pay" => {
                        let last_id = client.get_last_id();
                        let balance = client.poll_get_balance(&client_pubkey);
                        match balance {
                            Ok(0) => {
                                println!("You don't have any tokens!");
                            }
                            Ok(balance) => {
                                println!("Spending tokens in {:?} transactions...", balance);
                                let mut seed = [0u8; 32];
                                seed.copy_from_slice(&client_keypair.public_key_bytes()[..32]);
                                let rnd = GenKeys::new(seed);
                                let txs = balance.clone();
                                let keypairs = rnd.gen_n_keypairs(balance);
                                let transactions: Vec<_> = keypairs
                                    .par_iter()
                                    .map(|keypair| {
                                        Transaction::new(
                                            &client_keypair,
                                            keypair.pubkey(),
                                            1,
                                            last_id,
                                        )
                                    })
                                    .collect();
                                let transfer_start = Instant::now();
                                for tx in transactions {
                                    client.transfer_signed(tx.clone()).unwrap();
                                }
                                println!(
                                    "Transactions complete. {:?} ms {} tps",
                                    duration_as_ms(&transfer_start.elapsed()),
                                    txs as f32 / (duration_as_s(&transfer_start.elapsed()))
                                );
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

fn mk_client(client_addr: &SocketAddr, r: &ReplicatedData) -> ThinClient {
    let mut addr = client_addr.clone();
    let port = addr.port();
    let transactions_socket = UdpSocket::bind(addr.clone()).unwrap();
    addr.set_port(port + 1);
    let requests_socket = UdpSocket::bind(addr.clone()).unwrap();
    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();

    addr.set_port(port + 2);
    ThinClient::new(
        r.requests_addr,
        requests_socket,
        r.transactions_addr,
        transactions_socket,
    )
}

fn request_airdrop(drone_addr: &SocketAddr, client_pubkey: &PublicKey) {
    let mut stream = TcpStream::connect(drone_addr).unwrap();
    let req = DroneRequest::GetAirdrop {
        airdrop_request_amount: 50,
        client_public_key: *client_pubkey,
    };
    let tx = serialize(&req).expect("serialize drone request");
    stream.write_all(&tx).unwrap();
    // TODO: add timeout to this function, in case of unresponsive drone
}
