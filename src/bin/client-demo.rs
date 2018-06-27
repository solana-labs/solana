extern crate atty;
extern crate env_logger;
extern crate getopts;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use atty::{is, Stream};
use getopts::Options;
use rayon::prelude::*;
use solana::crdt::{get_ip_addr, Crdt, ReplicatedData};
use solana::hash::Hash;
use solana::mint::Mint;
use solana::ncp::Ncp;
use solana::signature::{GenKeys, KeyPair, KeyPairUtil};
use solana::streamer::default_window;
use solana::thin_client::ThinClient;
use solana::timing::{duration_as_ms, duration_as_s};
use solana::transaction::Transaction;
use std::env;
use std::fs::File;
use std::io::{stdin, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

fn print_usage(program: &str, opts: Options) {
    let mut brief = format!("Usage: cat <mint.json> | {} [options]\n\n", program);
    brief += "  Solana client demo creates a number of transactions and\n";
    brief += "  sends them to a target node.";
    brief += "  Takes json formatted mint file to stdin.";

    print!("{}", opts.usage(&brief));
}

fn sample_tx_count(
    thread_addr: Arc<RwLock<SocketAddr>>,
    exit: Arc<AtomicBool>,
    maxes: Arc<RwLock<Vec<(f64, u64)>>>,
    first_count: u64,
    v: ReplicatedData,
    sample_period: u64,
) {
    let mut client = mk_client(&thread_addr, &v);
    let mut now = Instant::now();
    let mut initial_tx_count = client.transaction_count();
    let mut max_tps = 0.0;
    let mut total;
    loop {
        let tx_count = client.transaction_count();
        let duration = now.elapsed();
        now = Instant::now();
        let sample = tx_count - initial_tx_count;
        initial_tx_count = tx_count;
        println!("{}: Transactions processed {}", v.transactions_addr, sample);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (sample * 1_000_000_000) as f64 / ns as f64;
        if tps > max_tps {
            max_tps = tps;
        }
        println!("{}: {:.2} tps", v.transactions_addr, tps);
        total = tx_count - first_count;
        println!(
            "{}: Total Transactions processed {}",
            v.transactions_addr, total
        );
        sleep(Duration::new(sample_period, 0));

        if exit.load(Ordering::Relaxed) {
            println!("exiting validator thread");
            maxes.write().unwrap().push((max_tps, total));
            break;
        }
    }
}

fn generate_and_send_txs(
    client: &mut ThinClient,
    tx_clients: &Vec<ThinClient>,
    mint: &Mint,
    keypairs: &Vec<KeyPair>,
    leader: &ReplicatedData,
    txs: i64,
    last_id: &mut Hash,
    threads: usize,
) {
    println!("Signing transactions... {}", keypairs.len(),);
    let signing_start = Instant::now();
    let transactions: Vec<_> = keypairs
        .par_iter()
        .map(|keypair| Transaction::new(&mint.keypair(), keypair.pubkey(), 1, *last_id))
        .collect();

    let duration = signing_start.elapsed();
    let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
    let bsps = txs as f64 / ns as f64;
    let nsps = ns as f64 / txs as f64;
    println!(
        "Done. {:.2} thousand signatures per second, {:.2} us per signature, {} ms total time",
        bsps * 1_000_000_f64,
        nsps / 1_000_f64,
        duration_as_ms(&duration),
    );

    println!("Transfering {} transactions in {} batches", txs, threads);
    let transfer_start = Instant::now();
    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    chunks
        .into_par_iter()
        .zip(tx_clients)
        .for_each(|(txs, client)| {
            println!(
                "Transferring 1 unit {} times... to {:?}",
                txs.len(),
                leader.transactions_addr
            );
            for tx in txs {
                client.transfer_signed(tx.clone()).unwrap();
            }
        });
    println!(
        "Transfer done. {:?} ms {} tps",
        duration_as_ms(&transfer_start.elapsed()),
        txs as f32 / (duration_as_s(&transfer_start.elapsed()))
    );

    loop {
        let new_id = client.get_last_id();
        if *last_id != new_id {
            *last_id = new_id;
            break;
        }
        sleep(Duration::from_millis(100));
    }
}

fn main() {
    env_logger::init();
    let mut threads = 4usize;
    let mut num_nodes = 1usize;
    let mut time_sec = 60;

    let mut opts = Options::new();
    opts.optopt("l", "", "leader", "leader.json");
    opts.optopt("c", "", "client port", "port");
    opts.optopt("t", "", "number of threads", &format!("{}", threads));
    opts.optflag("d", "dyn", "detect network address dynamically");
    opts.optopt(
        "s",
        "",
        "send transactions for this many seconds",
        &format!("{}", time_sec),
    );
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
    let mut addr: SocketAddr = "0.0.0.0:8100".parse().unwrap();
    if matches.opt_present("c") {
        let port = matches.opt_str("c").unwrap().parse().unwrap();
        addr.set_port(port);
    }
    if matches.opt_present("d") {
        addr.set_ip(get_ip_addr().unwrap());
    }
    let client_addr: Arc<RwLock<SocketAddr>> = Arc::new(RwLock::new(addr));
    if matches.opt_present("t") {
        threads = matches.opt_str("t").unwrap().parse().expect("integer");
    }
    if matches.opt_present("n") {
        num_nodes = matches.opt_str("n").unwrap().parse().expect("integer");
    }
    if matches.opt_present("s") {
        time_sec = matches.opt_str("s").unwrap().parse().expect("integer");
    }

    let leader = if matches.opt_present("l") {
        read_leader(matches.opt_str("l").unwrap())
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        ReplicatedData::new_leader(&server_addr)
    };

    let signal = Arc::new(AtomicBool::new(false));
    let mut c_threads = vec![];
    let validators = converge(
        &client_addr,
        &leader,
        signal.clone(),
        num_nodes,
        &mut c_threads,
    );
    assert_eq!(validators.len(), num_nodes);

    if is(Stream::Stdin) {
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
    let mint: Mint = serde_json::from_str(&buffer).unwrap_or_else(|e| {
        eprintln!("failed to parse json: {}", e);
        exit(1);
    });
    let mut client = mk_client(&client_addr, &leader);

    println!("Get last ID...");
    let mut last_id = client.get_last_id();
    println!("Got last ID {:?}", last_id);

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&mint.keypair().public_key_bytes()[..32]);
    let rnd = GenKeys::new(seed);

    println!("Creating keypairs...");
    let num_accounts = 500_000;
    let txs = num_accounts / 2;
    let keypairs = rnd.gen_n_keypairs(num_accounts);

    let first_count = client.transaction_count();
    println!("initial count {}", first_count);

    println!("Sampling tps every second...",);

    // Setup a thread per validator to sample every period
    // collect the max transaction rate and total tx count seen
    let maxes = Arc::new(RwLock::new(Vec::new()));
    let sample_period = 1; // in seconds
    let v_threads: Vec<_> = validators
        .into_iter()
        .map(|v| {
            let exit = signal.clone();
            let thread_addr = client_addr.clone();
            let maxes = maxes.clone();
            Builder::new()
                .name("solana-client-sample".to_string())
                .spawn(move || {
                    sample_tx_count(thread_addr, exit, maxes, first_count, v, sample_period);
                })
                .unwrap()
        })
        .collect();

    let clients = (0..threads)
        .map(|_| mk_client(&client_addr, &leader))
        .collect();

    // generate and send transactions for the specified duration
    let time = Duration::new(time_sec, 0);
    let now = Instant::now();
    while now.elapsed() < time {
        generate_and_send_txs(
            &mut client,
            &clients,
            &mint,
            &keypairs,
            &leader,
            txs,
            &mut last_id,
            threads,
        );
    }

    // Stop the sampling threads so it will collect the stats
    signal.store(true, Ordering::Relaxed);
    for t in v_threads {
        t.join().unwrap();
    }

    // Compute/report stats
    let mut max_of_maxes = 0.0;
    let mut total_txs = 0;
    for (max, txs) in maxes.read().unwrap().iter() {
        if *max > max_of_maxes {
            max_of_maxes = *max;
        }
        total_txs += *txs;
    }
    println!(
        "\nHighest TPS: {:.2} sampling period {}s total transactions: {} clients: {}",
        max_of_maxes,
        sample_period,
        total_txs,
        maxes.read().unwrap().len()
    );

    // join the crdt client threads
    for t in c_threads {
        t.join().unwrap();
    }
}

fn mk_client(locked_addr: &Arc<RwLock<SocketAddr>>, r: &ReplicatedData) -> ThinClient {
    let mut addr = locked_addr.write().unwrap();
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

fn spy_node(client_addr: &Arc<RwLock<SocketAddr>>) -> (ReplicatedData, UdpSocket) {
    let mut addr = client_addr.write().unwrap();
    let port = addr.port();
    let gossip = UdpSocket::bind(addr.clone()).unwrap();
    addr.set_port(port + 1);
    let daddr = "0.0.0.0:0".parse().unwrap();
    let pubkey = KeyPair::new().pubkey();
    let node = ReplicatedData::new(
        pubkey,
        gossip.local_addr().unwrap(),
        daddr,
        daddr,
        daddr,
        daddr,
    );
    (node, gossip)
}

fn converge(
    client_addr: &Arc<RwLock<SocketAddr>>,
    leader: &ReplicatedData,
    exit: Arc<AtomicBool>,
    num_nodes: usize,
    threads: &mut Vec<JoinHandle<()>>,
) -> Vec<ReplicatedData> {
    //lets spy on the network
    let daddr = "0.0.0.0:0".parse().unwrap();
    let (spy, spy_gossip) = spy_node(client_addr);
    let mut spy_crdt = Crdt::new(spy);
    spy_crdt.insert(&leader);
    spy_crdt.set_leader(leader.id);
    let spy_ref = Arc::new(RwLock::new(spy_crdt));
    let window = default_window();
    let gossip_send_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
    let ncp = Ncp::new(
        spy_ref.clone(),
        window.clone(),
        spy_gossip,
        gossip_send_socket,
        exit.clone(),
    ).expect("DataReplicator::new");
    let mut rv = vec![];
    //wait for the network to converge, 30 seconds should be plenty
    for _ in 0..30 {
        let v: Vec<ReplicatedData> = spy_ref
            .read()
            .unwrap()
            .table
            .values()
            .into_iter()
            .filter(|x| x.requests_addr != daddr)
            .cloned()
            .collect();
        if v.len() >= num_nodes {
            println!("CONVERGED!");
            rv.extend(v.into_iter());
            break;
        }
        sleep(Duration::new(1, 0));
    }
    threads.extend(ncp.thread_hdls.into_iter());
    rv
}

fn read_leader(path: String) -> ReplicatedData {
    let file = File::open(path.clone()).expect(&format!("file not found: {}", path));
    serde_json::from_reader(file).expect(&format!("failed to parse {}", path))
}
