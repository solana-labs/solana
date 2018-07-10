extern crate bincode;
extern crate clap;
extern crate env_logger;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use bincode::serialize;
use clap::{App, Arg};
use rayon::prelude::*;
use solana::crdt::{Crdt, ReplicatedData};
use solana::drone::DroneRequest;
use solana::hash::Hash;
use solana::mint::Mint;
use solana::nat::{udp_public_bind, udp_random_bind};
use solana::ncp::Ncp;
use solana::service::Service;
use solana::signature::{GenKeys, KeyPair, KeyPairUtil};
use solana::streamer::default_window;
use solana::thin_client::ThinClient;
use solana::timing::{duration_as_ms, duration_as_s};
use solana::transaction::Transaction;
use std::error;
use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

fn sample_tx_count(
    exit: Arc<AtomicBool>,
    maxes: Arc<RwLock<Vec<(f64, u64)>>>,
    first_count: u64,
    v: ReplicatedData,
    sample_period: u64,
) {
    let mut client = mk_client(&v);
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
        println!("{}: Transactions processed {}", v.contact_info.tpu, sample);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (sample * 1_000_000_000) as f64 / ns as f64;
        if tps > max_tps {
            max_tps = tps;
        }
        println!("{}: {:.2} tps", v.contact_info.tpu, tps);
        total = tx_count - first_count;
        println!(
            "{}: Total Transactions processed {}",
            v.contact_info.tpu, total
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
    id: &Mint,
    keypairs: &Vec<KeyPair>,
    leader: &ReplicatedData,
    txs: i64,
    last_id: &mut Hash,
    threads: usize,
) {
    println!("Signing transactions... {}", txs,);
    let signing_start = Instant::now();

    let transactions: Vec<_> = keypairs
        .par_iter()
        .map(|keypair| Transaction::new(&id.keypair(), keypair.pubkey(), 1, *last_id))
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
                leader.contact_info.tpu
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

    let matches = App::new("solana-client-demo")
        .arg(
            Arg::with_name("leader")
                .short("l")
                .long("leader")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/leader.json"),
        )
        .arg(
            Arg::with_name("mint")
                .short("m")
                .long("mint")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/mint.json"),
        )
        .arg(
            Arg::with_name("num_nodes")
                .short("n")
                .long("nodes")
                .value_name("NUMBER")
                .takes_value(true)
                .help("number of nodes to converge to"),
        )
        .arg(
            Arg::with_name("threads")
                .short("t")
                .long("threads")
                .value_name("NUMBER")
                .takes_value(true)
                .help("number of threads"),
        )
        .arg(
            Arg::with_name("seconds")
                .short("s")
                .long("sec")
                .value_name("NUMBER")
                .takes_value(true)
                .help("send transactions for this many seconds"),
        )
        .get_matches();

    let leader: ReplicatedData;
    if let Some(l) = matches.value_of("leader") {
        leader = read_leader(l.to_string());
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        leader = ReplicatedData::new_leader(&server_addr);
    };

    let id: Mint;
    if let Some(m) = matches.value_of("mint") {
        id = read_mint(m.to_string()).expect("client mint");
    } else {
        eprintln!("No mint found!");
        exit(1);
    };

    if let Some(t) = matches.value_of("threads") {
        threads = t.to_string().parse().expect("integer");
    }

    if let Some(n) = matches.value_of("nodes") {
        num_nodes = n.to_string().parse().expect("integer");
    }

    if let Some(s) = matches.value_of("seconds") {
        time_sec = s.to_string().parse().expect("integer");
    }

    let mut drone_addr = leader.contact_info.tpu.clone();
    drone_addr.set_port(9900);

    let signal = Arc::new(AtomicBool::new(false));
    let mut c_threads = vec![];
    let validators = converge(&leader, signal.clone(), num_nodes, &mut c_threads);
    assert_eq!(validators.len(), num_nodes);

    let mut client = mk_client(&leader);

    let starting_balance = client.poll_get_balance(&id.pubkey()).unwrap();

    let txs: i64 = 500_000;
    println!("Airdropping {:?} tokens", txs);
    let _airdrop = request_airdrop(&drone_addr, &id, txs as u64).unwrap();
    // TODO: return airdrop Result from Drone
    sleep(Duration::from_millis(100));
    let balance = client.poll_get_balance(&id.pubkey()).unwrap();
    println!("Your balance is: {:?}", balance);

    if balance < txs || (starting_balance == balance) {
        println!("TPS airdrop limit reached; wait 60sec to retry");
        exit(1);
    }

    println!("Get last ID...");
    let mut last_id = client.get_last_id();
    println!("Got last ID {:?}", last_id);

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&id.keypair().public_key_bytes()[..32]);
    let rnd = GenKeys::new(seed);

    println!("Creating keypairs...");
    let keypairs = rnd.gen_n_keypairs(txs);

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
            let maxes = maxes.clone();
            Builder::new()
                .name("solana-client-sample".to_string())
                .spawn(move || {
                    sample_tx_count(exit, maxes, first_count, v, sample_period);
                })
                .unwrap()
        })
        .collect();

    let clients = (0..threads).map(|_| mk_client(&leader)).collect();

    // generate and send transactions for the specified duration
    let time = Duration::new(time_sec, 0);
    let now = Instant::now();
    while now.elapsed() < time {
        generate_and_send_txs(
            &mut client,
            &clients,
            &id,
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

fn mk_client(r: &ReplicatedData) -> ThinClient {
    let requests_socket = udp_random_bind(8000, 10000, 5).unwrap();
    let transactions_socket = udp_random_bind(8000, 10000, 5).unwrap();

    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();

    ThinClient::new(
        r.contact_info.rpu,
        requests_socket,
        r.contact_info.tpu,
        transactions_socket,
    )
}

fn spy_node() -> (ReplicatedData, UdpSocket) {
    let gossip_socket_pair = udp_public_bind("gossip", 8000, 10000);
    let pubkey = KeyPair::new().pubkey();
    let daddr = "0.0.0.0:0".parse().unwrap();
    let node = ReplicatedData::new(
        pubkey,
        //gossip.local_addr().unwrap(),
        gossip_socket_pair.addr,
        daddr,
        daddr,
        daddr,
        daddr,
    );
    (node, gossip_socket_pair.receiver)
}

fn converge(
    leader: &ReplicatedData,
    exit: Arc<AtomicBool>,
    num_nodes: usize,
    threads: &mut Vec<JoinHandle<()>>,
) -> Vec<ReplicatedData> {
    //lets spy on the network
    let daddr = "0.0.0.0:0".parse().unwrap();
    let (spy, spy_gossip) = spy_node();
    let mut spy_crdt = Crdt::new(spy);
    spy_crdt.insert(&leader);
    spy_crdt.set_leader(leader.id);
    let spy_ref = Arc::new(RwLock::new(spy_crdt));
    let window = default_window();
    let gossip_send_socket = udp_random_bind(8000, 10000, 5).unwrap();
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
            .filter(|x| x.contact_info.rpu != daddr)
            .cloned()
            .collect();
        if v.len() >= num_nodes {
            println!("CONVERGED!");
            rv.extend(v.into_iter());
            break;
        }
        sleep(Duration::new(1, 0));
    }
    threads.extend(ncp.thread_hdls().into_iter());
    rv
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

fn request_airdrop(
    drone_addr: &SocketAddr,
    id: &Mint,
    tokens: u64,
) -> Result<(), Box<error::Error>> {
    let mut stream = TcpStream::connect(drone_addr)?;
    let req = DroneRequest::GetAirdrop {
        airdrop_request_amount: tokens,
        client_public_key: id.pubkey(),
    };
    let tx = serialize(&req).expect("serialize drone request");
    stream.write_all(&tx).unwrap();
    // TODO: add timeout to this function, in case of unresponsive drone
    Ok(())
}
