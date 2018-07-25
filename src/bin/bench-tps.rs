extern crate bincode;
extern crate clap;
extern crate env_logger;
extern crate rayon;
extern crate serde_json;
extern crate solana;

use bincode::serialize;
use clap::{App, Arg};
use rayon::prelude::*;
use solana::crdt::{Crdt, NodeInfo};
use solana::drone::{DroneRequest, DRONE_PORT};
use solana::fullnode::Config;
use solana::hash::Hash;
use solana::metrics::set_panic_hook;
use solana::nat::{udp_public_bind, udp_random_bind, UdpSocketPair};
use solana::ncp::Ncp;
use solana::service::Service;
use solana::signature::{read_keypair, GenKeys, KeyPair, KeyPairUtil};
use solana::streamer::default_window;
use solana::thin_client::ThinClient;
use solana::timing::{duration_as_ms, duration_as_s};
use solana::transaction::Transaction;
use std::collections::VecDeque;
use std::error;
use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::process::exit;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

pub struct NodeStats {
    pub tps: f64, // Maximum TPS reported by this node
    pub tx: u64,  // Total transactions reported by this node
}

fn sample_tx_count(
    exit_signal: &Arc<AtomicBool>,
    maxes: &Arc<RwLock<Vec<(SocketAddr, NodeStats)>>>,
    first_tx_count: u64,
    v: &NodeInfo,
    sample_period: u64,
) {
    let mut client = mk_client(&v);
    let mut now = Instant::now();
    let mut initial_tx_count = client.transaction_count();
    let mut max_tps = 0.0;
    let mut total;

    let log_prefix = format!("{:21}:", v.contact_info.tpu.to_string());

    loop {
        let tx_count = client.transaction_count();
        assert!(tx_count >= initial_tx_count);
        let duration = now.elapsed();
        now = Instant::now();
        let sample = tx_count - initial_tx_count;
        initial_tx_count = tx_count;
        println!("{} Transactions processed {}", log_prefix, sample);
        let ns = duration.as_secs() * 1_000_000_000 + u64::from(duration.subsec_nanos());
        let tps = (sample * 1_000_000_000) as f64 / ns as f64;
        if tps > max_tps {
            max_tps = tps;
        }
        println!("{} {:.2} TPS", log_prefix, tps);
        if tx_count > first_tx_count {
            total = tx_count - first_tx_count;
        } else {
            total = 0;
        }
        println!("{} Total transactions processed {}", log_prefix, total);
        sleep(Duration::new(sample_period, 0));

        if exit_signal.load(Ordering::Relaxed) {
            println!("{} Exiting validator thread", log_prefix);
            let stats = NodeStats {
                tps: max_tps,
                tx: total,
            };
            maxes.write().unwrap().push((v.contact_info.tpu, stats));
            break;
        }
    }
}

fn generate_txs(
    client: &mut ThinClient,
    shared_txs: &Arc<RwLock<VecDeque<Vec<Transaction>>>>,
    id: &KeyPair,
    keypairs: &[KeyPair],
    txs: i64,
    last_id: &mut Hash,
    threads: usize,
    reclaim: bool,
) {
    println!("Signing transactions... {} (reclaim={})", txs / 2, reclaim);
    let signing_start = Instant::now();

    let transactions: Vec<_> = if !reclaim {
        keypairs
            .par_iter()
            .map(|keypair| Transaction::new(&id, keypair.pubkey(), 1, *last_id))
            .collect()
    } else {
        keypairs
            .par_iter()
            .map(|keypair| Transaction::new(keypair, id.pubkey(), 1, *last_id))
            .collect()
    };

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

    let sz = transactions.len() / threads;
    let chunks: Vec<_> = transactions.chunks(sz).collect();
    {
        let mut shared_txs_wl = shared_txs.write().unwrap();
        for chunk in chunks {
            shared_txs_wl.push_back(chunk.to_vec());
        }
    }

    let mut found_new_last_id = false;
    // try for ~5s to get a new last_id
    for i in 0..32 {
        let new_id = client.get_last_id();
        if *last_id != new_id {
            *last_id = new_id;
            found_new_last_id = true;
            break;
        }
        if i != 0 && (i % 8) == 0 {
            println!("polling for new last_id try: {}", i);
        }
        sleep(Duration::from_millis(200));
    }

    if !found_new_last_id {
        println!("Error: Couldn't get new last id!");
        exit(1);
    }
}

fn do_tx_transfers(
    exit_signal: &Arc<AtomicBool>,
    shared_txs: &Arc<RwLock<VecDeque<Vec<Transaction>>>>,
    leader: &NodeInfo,
    shared_tx_thread_count: &Arc<AtomicIsize>,
) {
    let client = mk_client(&leader);
    loop {
        let txs;
        {
            let mut shared_txs_wl = shared_txs.write().unwrap();
            txs = shared_txs_wl.pop_front();
        }
        if let Some(txs0) = txs {
            shared_tx_thread_count.fetch_add(1, Ordering::Relaxed);
            println!(
                "Transferring 1 unit {} times... to {}",
                txs0.len(),
                leader.contact_info.tpu
            );
            let tx_len = txs0.len();
            let transfer_start = Instant::now();
            for tx in txs0 {
                client.transfer_signed(&tx).unwrap();
            }
            shared_tx_thread_count.fetch_add(-1, Ordering::Relaxed);
            println!(
                "Tx send done. {} ms {} tps",
                duration_as_ms(&transfer_start.elapsed()),
                tx_len as f32 / duration_as_s(&transfer_start.elapsed()),
            );
        }
        if exit_signal.load(Ordering::Relaxed) {
            break;
        }
    }
}

fn airdrop_tokens(client: &mut ThinClient, leader: &NodeInfo, id: &KeyPair, tx_count: i64) {
    let mut drone_addr = leader.contact_info.tpu;
    drone_addr.set_port(DRONE_PORT);

    let starting_balance = client.poll_get_balance(&id.pubkey()).unwrap();
    println!("Token balance: {}", starting_balance);

    if starting_balance < tx_count {
        let airdrop_amount = tx_count - starting_balance;
        println!(
            "Airdropping {:?} tokens from {}",
            airdrop_amount, drone_addr
        );

        let previous_balance = starting_balance;
        request_airdrop(&drone_addr, &id, airdrop_amount as u64).unwrap();

        // TODO: return airdrop Result from Drone instead of polling the
        //       network
        let mut current_balance = previous_balance;
        for _ in 0..20 {
            sleep(Duration::from_millis(500));
            current_balance = client.poll_get_balance(&id.pubkey()).unwrap();
            if starting_balance != current_balance {
                break;
            }
            println!(".");
        }
        println!("Token balance: {}", current_balance);
        if current_balance - starting_balance != airdrop_amount {
            println!("Airdrop failed!");
            exit(1);
        }
    }
}

fn compute_and_report_stats(
    maxes: &Arc<RwLock<Vec<(SocketAddr, NodeStats)>>>,
    sample_period: u64,
    tx_send_elapsed: &Duration,
) {
    // Compute/report stats
    let mut max_of_maxes = 0.0;
    let mut total_txs = 0;
    let mut nodes_with_zero_tps = 0;
    let mut total_maxes = 0.0;
    println!(" Node address        |       Max TPS | Total Transactions");
    println!("---------------------+---------------+--------------------");

    for (sock, stats) in maxes.read().unwrap().iter() {
        let maybe_flag = match stats.tx {
            0 => "!!!!!",
            _ => "",
        };

        println!(
            "{:20} | {:13.2} | {} {}",
            (*sock).to_string(),
            stats.tps,
            stats.tx,
            maybe_flag
        );

        if stats.tps == 0.0 {
            nodes_with_zero_tps += 1;
        }
        total_maxes += stats.tps;

        if stats.tps > max_of_maxes {
            max_of_maxes = stats.tps;
        }
        total_txs += stats.tx;
    }

    if total_maxes > 0.0 {
        let num_nodes_with_tps = maxes.read().unwrap().len() - nodes_with_zero_tps;
        let average_max = total_maxes / num_nodes_with_tps as f64;
        println!(
            "\nAverage max TPS: {:.2}, {} nodes had 0 TPS",
            average_max, nodes_with_zero_tps
        );
    }

    println!(
        "\nHighest TPS: {:.2} sampling period {}s total transactions: {} clients: {}",
        max_of_maxes,
        sample_period,
        total_txs,
        maxes.read().unwrap().len()
    );
    println!(
        "\tAverage TPS: {}",
        total_txs as f32 / duration_as_s(tx_send_elapsed)
    );
}

fn main() {
    env_logger::init();
    set_panic_hook("bench-tps");
    let mut threads = 4usize;
    let mut num_nodes = 1usize;
    let mut time_sec = 90;
    let mut addr = None;
    let mut sustained = false;
    let mut tx_count = 500_000;

    let matches = App::new("solana-bench-tps")
        .arg(
            Arg::with_name("leader")
                .short("l")
                .long("leader")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/leader.json"),
        )
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .default_value("~/.config/solana/id.json")
                .help("/path/to/id.json"),
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
        .arg(
            Arg::with_name("converge_only")
                .short("c")
                .help("exit immediately after converging"),
        )
        .arg(
            Arg::with_name("addr")
                .short("a")
                .long("addr")
                .value_name("PATH")
                .takes_value(true)
                .help("address to advertise to the network"),
        )
        .arg(
            Arg::with_name("sustained")
                .long("sustained")
                .help("Use sustained performance mode vs. peak mode. This overlaps the tx generation with transfers."),
        )
        .arg(
            Arg::with_name("tx_count")
                .long("tx_count")
                .value_name("NUMBER")
                .takes_value(true)
                .help("number of transactions to send in a single batch")
        )
        .get_matches();

    let leader: NodeInfo;
    if let Some(l) = matches.value_of("leader") {
        leader = read_leader(l).node_info;
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        leader = NodeInfo::new_leader(&server_addr);
    };

    let id = read_keypair(matches.value_of("keypair").unwrap()).expect("client keypair");

    if let Some(t) = matches.value_of("threads") {
        threads = t.to_string().parse().expect("integer");
    }

    if let Some(n) = matches.value_of("num_nodes") {
        num_nodes = n.to_string().parse().expect("integer");
    }

    if let Some(s) = matches.value_of("seconds") {
        time_sec = s.to_string().parse().expect("integer");
    }

    if let Some(s) = matches.value_of("addr") {
        addr = Some(s.to_string());
    }

    if let Some(s) = matches.value_of("tx_count") {
        tx_count = s.to_string().parse().expect("integer");
    }

    if matches.is_present("sustained") {
        sustained = true;
    }

    let exit_signal = Arc::new(AtomicBool::new(false));
    let mut c_threads = vec![];
    let validators = converge(&leader, &exit_signal, num_nodes, &mut c_threads, addr);

    println!(" Node address         | Node identifier");
    println!("----------------------+------------------");
    for node in &validators {
        println!(
            " {:20} | {:16x}",
            node.contact_info.tpu.to_string(),
            node.debug_id()
        );
    }
    println!("Nodes: {}", validators.len());

    if validators.len() < num_nodes {
        println!(
            "Error: Insufficient nodes discovered.  Expecting {} or more",
            num_nodes
        );
        exit(1);
    }

    if matches.is_present("converge_only") {
        return;
    }

    let mut client = mk_client(&leader);

    // get some tokens
    airdrop_tokens(&mut client, &leader, &id, tx_count);

    println!("Get last ID...");
    let mut last_id = client.get_last_id();
    println!("Got last ID {:?}", last_id);

    let mut seed = [0u8; 32];
    seed.copy_from_slice(&id.public_key_bytes()[..32]);
    let rnd = GenKeys::new(seed);

    println!("Creating {} keypairs...", tx_count / 2);
    let keypairs = rnd.gen_n_keypairs(tx_count / 2);

    let first_tx_count = client.transaction_count();
    println!("Initial transaction count {}", first_tx_count);

    // Setup a thread per validator to sample every period
    // collect the max transaction rate and total tx count seen
    let maxes = Arc::new(RwLock::new(Vec::new()));
    let sample_period = 1; // in seconds
    println!("Sampling TPS every {} second...", sample_period);
    let v_threads: Vec<_> = validators
        .into_iter()
        .map(|v| {
            let exit_signal = exit_signal.clone();
            let maxes = maxes.clone();
            Builder::new()
                .name("solana-client-sample".to_string())
                .spawn(move || {
                    sample_tx_count(&exit_signal, &maxes, first_tx_count, &v, sample_period);
                })
                .unwrap()
        })
        .collect();

    let shared_txs: Arc<RwLock<VecDeque<Vec<Transaction>>>> =
        Arc::new(RwLock::new(VecDeque::new()));

    let shared_tx_active_thread_count = Arc::new(AtomicIsize::new(0));

    let s_threads: Vec<_> = (0..threads)
        .map(|_| {
            let exit_signal = exit_signal.clone();
            let shared_txs = shared_txs.clone();
            let leader = leader.clone();
            let shared_tx_active_thread_count = shared_tx_active_thread_count.clone();
            Builder::new()
                .name("solana-client-sender".to_string())
                .spawn(move || {
                    do_tx_transfers(
                        &exit_signal,
                        &shared_txs,
                        &leader,
                        &shared_tx_active_thread_count,
                    );
                })
                .unwrap()
        })
        .collect();

    // generate and send transactions for the specified duration
    let time = Duration::new(time_sec, 0);
    let now = Instant::now();
    let mut reclaim_tokens_back_to_source_account = false;
    while now.elapsed() < time || reclaim_tokens_back_to_source_account {
        let balance = client.poll_get_balance(&id.pubkey()).unwrap_or(-1);
        println!("Token balance: {}", balance);

        // ping-pong between source and destination accounts for each loop iteration
        // this seems to be faster than trying to determine the balance of individual
        // accounts
        generate_txs(
            &mut client,
            &shared_txs,
            &id,
            &keypairs,
            tx_count,
            &mut last_id,
            threads,
            reclaim_tokens_back_to_source_account,
        );
        reclaim_tokens_back_to_source_account = !reclaim_tokens_back_to_source_account;

        // In sustained mode overlap the transfers with generation
        // this has higher average performance but lower peak performance
        // in tested environments.
        if !sustained {
            while shared_tx_active_thread_count.load(Ordering::Relaxed) > 0 {
                sleep(Duration::from_millis(100));
            }
        }
    }

    // Stop the sampling threads so it will collect the stats
    exit_signal.store(true, Ordering::Relaxed);
    for t in v_threads {
        t.join().unwrap();
    }

    // join the tx send threads
    for t in s_threads {
        t.join().unwrap();
    }

    let balance = client.poll_get_balance(&id.pubkey()).unwrap_or(-1);
    println!("Token balance: {}", balance);

    compute_and_report_stats(&maxes, sample_period, &now.elapsed());

    // join the crdt client threads
    for t in c_threads {
        t.join().unwrap();
    }
}

fn mk_client(r: &NodeInfo) -> ThinClient {
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

fn spy_node(addr: Option<String>) -> (NodeInfo, UdpSocket) {
    let gossip_socket_pair;
    if let Some(a) = addr {
        gossip_socket_pair = UdpSocketPair {
            addr: a.parse().unwrap(),
            receiver: UdpSocket::bind("0.0.0.0:0").unwrap(),
            sender: UdpSocket::bind("0.0.0.0:0").unwrap(),
        };
    } else {
        gossip_socket_pair = udp_public_bind("gossip", 8000, 10000);
    }

    let pubkey = KeyPair::new().pubkey();
    let daddr = "0.0.0.0:0".parse().unwrap();
    assert!(!gossip_socket_pair.addr.ip().is_unspecified());
    assert!(!gossip_socket_pair.addr.ip().is_multicast());
    let node = NodeInfo::new(
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
    leader: &NodeInfo,
    exit_signal: &Arc<AtomicBool>,
    num_nodes: usize,
    threads: &mut Vec<JoinHandle<()>>,
    addr: Option<String>,
) -> Vec<NodeInfo> {
    //lets spy on the network
    let (spy, spy_gossip) = spy_node(addr);
    let mut spy_crdt = Crdt::new(spy).expect("Crdt::new");
    spy_crdt.insert(&leader);
    spy_crdt.set_leader(leader.id);
    let spy_ref = Arc::new(RwLock::new(spy_crdt));
    let window = default_window();
    let gossip_send_socket = udp_random_bind(8000, 10000, 5).unwrap();
    let ncp = Ncp::new(
        &spy_ref,
        window.clone(),
        spy_gossip,
        gossip_send_socket,
        exit_signal.clone(),
    ).expect("DataReplicator::new");
    let mut rv = vec![];
    //wait for the network to converge, 30 seconds should be plenty
    for _ in 0..30 {
        let v: Vec<NodeInfo> = spy_ref
            .read()
            .unwrap()
            .table
            .values()
            .into_iter()
            .filter(|x| Crdt::is_valid_address(x.contact_info.rpu))
            .cloned()
            .collect();
        if v.len() >= num_nodes {
            println!("CONVERGED!");
            rv.extend(v.into_iter());
            break;
        } else {
            println!(
                "{} node(s) discovered (looking for {} or more)",
                v.len(),
                num_nodes
            );
        }
        sleep(Duration::new(1, 0));
    }
    threads.extend(ncp.thread_hdls().into_iter());
    rv
}

fn read_leader(path: &str) -> Config {
    let file = File::open(path).unwrap_or_else(|_| panic!("file not found: {}", path));
    serde_json::from_reader(file).unwrap_or_else(|_| panic!("failed to parse {}", path))
}

fn request_airdrop(
    drone_addr: &SocketAddr,
    id: &KeyPair,
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
