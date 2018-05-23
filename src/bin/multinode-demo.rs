extern crate futures;
extern crate getopts;
extern crate isatty;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use futures::Future;
use getopts::Options;
use isatty::stdin_isatty;
use rayon::prelude::*;
use solana::crdt::{Crdt, ReplicatedData};
use solana::mint::MintDemo;
use solana::signature::{GenKeys, KeyPair, KeyPairUtil};
use solana::streamer::default_window;
use solana::thin_client::ThinClient;
use solana::transaction::Transaction;
use std::env;
use std::fs::File;
use std::io::{stdin, Read};
use std::net::{SocketAddr, UdpSocket};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: cat <mint.json> | {} [options]\n\n", program);
    brief += "  Solana client demo creates a number of transactions and\n";
    brief += "  sends them to a target node.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
}

fn main() {
    let mut threads = 4usize;
    let mut num_nodes = 10usize;
    let mut leader = "leader.json".to_string();
    let mut client_addr: SocketAddr = "127.0.0.1:8010".parse().unwrap();

    let mut opts = Options::new();
    opts.optopt("l", "", "leader", "leader.json");
    opts.optopt("c", "", "client address", "host");
    opts.optopt("t", "", "number of threads", &format!("{}", threads));
    opts.optopt(
        "n",
        "",
        "number of nodes to converge to",
        &format!("{}", num_nodes),
    );
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
    if matches.opt_present("l") {
        leader = matches.opt_str("l").unwrap();
    }
    if matches.opt_present("c") {
        client_addr = matches.opt_str("c").unwrap().parse().unwrap();
    }
    if matches.opt_present("t") {
        threads = matches.opt_str("t").unwrap().parse().expect("integer");
    }
    if matches.opt_present("n") {
        num_nodes = matches.opt_str("n").unwrap().parse().expect("integer");
    }

    let leader: ReplicatedData = read_leader(leader);
    let signal = Arc::new(AtomicBool::new(false));
    let mut c_threads = vec![];
    let validators = converge(
        &client_addr,
        &leader,
        signal.clone(),
        num_nodes + 2,
        &mut c_threads,
    );

    if stdin_isatty() {
        eprintln!("nothing found on stdin, expected a json file");
        exit(1);
    }

    let mut buffer = String::new();
    let num_bytes = stdin().read_to_string(&mut buffer).unwrap();
    if num_bytes == 0 {
        eprintln!("empty file on stdin, expected a json file");
        exit(1);
    }

    println!("Parsing stdin...");
    let demo: MintDemo = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });
    let mut client = mk_client(&client_addr, &leader);

    println!("Get last ID...");
    let last_id = client.get_last_id().wait().unwrap();
    println!("Got last ID {:?}", last_id);

    let rnd = GenKeys::new(demo.mint.keypair().public_key_bytes());
    let tokens_per_user = 1_000;

    println!("Creating keypairs...");
    let txs = demo.num_accounts / 2;
    let keypairs = rnd.gen_n_keypairs(demo.num_accounts);
    let keypair_pairs: Vec<_> = keypairs.chunks(2).collect();

    println!("Signing transactions...");
    let now = Instant::now();
    let transactions: Vec<_> = keypair_pairs
        .into_par_iter()
        .map(|chunk| Transaction::new(&chunk[0], chunk[1].pubkey(), 1, last_id))
        .collect();
    let mut duration = now.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = txs as f64 / ns as f64;
    let nsps = ns as f64 / txs as f64;
    println!(
        "Done. {} thousand signatures per second, {}us per signature",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64
    );

    let initial_tx_count = client.transaction_count();
    println!("initial count {}", initial_tx_count);

    println!("Transfering {} transactions in {} batches", txs, threads);
    let now = Instant::now();
    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    chunks.into_par_iter().for_each(|trs| {
        println!("Transferring 1 unit {} times... to", trs.len());
        let client = mk_client(&client_addr, &leader);
        for tr in trs {
            client.transfer_signed(tr.clone()).unwrap();
        }
    });

    println!("Waiting for transactions to complete...",);
    for _ in 0..10 {
        let mut tx_count = client.transaction_count();
        duration = now.elapsed();
        let txs = tx_count - initial_tx_count;
        println!("Transactions processed {}", txs);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (txs * 1_000_000_000) as f64 / ns as f64;
        println!("{} tps", tps);
        sleep(Duration::new(1, 0));
    }
    for val in validators {
        let mut client = mk_client(&client_addr, &val);
        let mut tx_count = client.transaction_count();
        duration = now.elapsed();
        let txs = tx_count - initial_tx_count;
        println!("Transactions processed {} on {}", txs, val.events_addr);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (txs * 1_000_000_000) as f64 / ns as f64;
        println!("{} tps on {}", tps, val.events_addr);
    }
    signal.store(true, Ordering::Relaxed);
    for t in c_threads {
        t.join().unwrap();
    }
}

fn mk_client(client_addr: &SocketAddr, r: &ReplicatedData) -> ThinClient {
    let mut c = client_addr.clone();
    c.set_port(0);
    let events_socket = UdpSocket::bind(c).unwrap();
    let mut addr = events_socket.local_addr().unwrap();
    let port = addr.port();
    addr.set_port(port + 1);
    let requests_socket = UdpSocket::bind(addr).unwrap();
    ThinClient::new(
        r.requests_addr,
        requests_socket,
        r.events_addr,
        events_socket,
    )
}

fn spy_node(client_addr: &SocketAddr) -> (ReplicatedData, UdpSocket) {
    let mut addr = client_addr.clone();
    addr.set_port(0);
    let gossip = UdpSocket::bind(addr).unwrap();
    let daddr = "0.0.0.0:0".parse().unwrap();
    let pubkey = KeyPair::new().pubkey();
    let node = ReplicatedData::new(pubkey, gossip.local_addr().unwrap(), daddr, daddr, daddr);
    (node, gossip)
}

fn converge(
    client_addr: &SocketAddr,
    leader: &ReplicatedData,
    exit: Arc<AtomicBool>,
    num_nodes: usize,
    threads: &mut Vec<JoinHandle<()>>,
) -> Vec<ReplicatedData> {
    //lets spy on the network
    let (spy, spy_gossip) = spy_node(client_addr);
    let me = spy.id.clone();
    let mut spy_crdt = Crdt::new(spy);
    spy_crdt.insert(&leader);
    spy_crdt.set_leader(leader.id);

    let spy_ref = Arc::new(RwLock::new(spy_crdt));
    let spy_window = default_window();
    let t_spy_listen = Crdt::listen(spy_ref.clone(), spy_window, spy_gossip, exit.clone());
    let t_spy_gossip = Crdt::gossip(spy_ref.clone(), exit.clone());
    //wait for the network to converge
    for _ in 0..30 {
        let min = spy_ref.read().unwrap().convergence();
        if num_nodes as u64 == min {
            println!("converged!");
            break;
        }
        sleep(Duration::new(1, 0));
    }
    threads.push(t_spy_listen);
    threads.push(t_spy_gossip);
    let v: Vec<ReplicatedData> = spy_ref
        .read()
        .unwrap()
        .table
        .values()
        .into_iter()
        .filter(|x| x.id != me)
        .map(|x| x.clone())
        .collect();
    v.clone()
}

fn read_leader(path: String) -> ReplicatedData {
    let file = File::open(path).expect("file");
    serde_json::from_reader(file).expect("parse")
}
