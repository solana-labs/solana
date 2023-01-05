#![allow(clippy::integer_arithmetic)]

use {
    clap::{crate_description, crate_name, Arg, Command},
    crossbeam_channel::unbounded,
    solana_streamer::{
        packet::{Packet, PacketBatch, PacketBatchRecycler, PACKET_DATA_SIZE},
        streamer::{receiver, PacketBatchReceiver, StreamerReceiveStats},
    },
    std::{
        cmp::max,
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread::{sleep, spawn, JoinHandle, Result},
        time::{Duration, SystemTime},
    },
};

fn producer(addr: &SocketAddr, exit: Arc<AtomicBool>) -> JoinHandle<()> {
    let send = UdpSocket::bind("0.0.0.0:0").unwrap();
    let batch_size = 10;
    let mut packet_batch = PacketBatch::with_capacity(batch_size);
    packet_batch.resize(batch_size, Packet::default());
    for w in packet_batch.iter_mut() {
        w.meta_mut().size = PACKET_DATA_SIZE;
        w.meta_mut().set_socket_addr(addr);
    }
    let packet_batch = Arc::new(packet_batch);
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let mut num = 0;
        for p in packet_batch.iter() {
            let a = p.meta().socket_addr();
            assert!(p.meta().size <= PACKET_DATA_SIZE);
            let data = p.data(..).unwrap_or_default();
            send.send_to(data, a).unwrap();
            num += 1;
        }
        assert_eq!(num, 10);
    })
}

fn sink(exit: Arc<AtomicBool>, rvs: Arc<AtomicUsize>, r: PacketBatchReceiver) -> JoinHandle<()> {
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let timer = Duration::new(1, 0);
        if let Ok(packet_batch) = r.recv_timeout(timer) {
            rvs.fetch_add(packet_batch.len(), Ordering::Relaxed);
        }
    })
}

fn main() -> Result<()> {
    let mut num_sockets = 1usize;

    let matches = Command::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::new("num-recv-sockets")
                .long("num-recv-sockets")
                .value_name("NUM")
                .takes_value(true)
                .help("Use NUM receive sockets"),
        )
        .arg(
            Arg::new("num-producers")
                .long("num-producers")
                .value_name("NUM")
                .takes_value(true)
                .help("Use this many producer threads."),
        )
        .get_matches();

    if let Some(n) = matches.value_of("num-recv-sockets") {
        num_sockets = max(num_sockets, n.to_string().parse().expect("integer"));
    }

    let num_producers: u64 = matches.value_of_t("num_producers").unwrap_or(4);

    let port = 0;
    let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
    let mut addr = SocketAddr::new(ip_addr, 0);

    let exit = Arc::new(AtomicBool::new(false));

    let mut read_channels = Vec::new();
    let mut read_threads = Vec::new();
    let recycler = PacketBatchRecycler::default();
    let (_port, read_sockets) = solana_net_utils::multi_bind_in_range(
        ip_addr,
        (port, port + num_sockets as u16),
        num_sockets,
    )
    .unwrap();
    let stats = Arc::new(StreamerReceiveStats::new("bench-streamer-test"));
    for read in read_sockets {
        read.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        addr = read.local_addr().unwrap();
        let (s_reader, r_reader) = unbounded();
        read_channels.push(r_reader);
        read_threads.push(receiver(
            Arc::new(read),
            exit.clone(),
            s_reader,
            recycler.clone(),
            stats.clone(),
            1,
            true,
            None,
        ));
    }

    let producer_threads: Vec<_> = (0..num_producers)
        .map(|_| producer(&addr, exit.clone()))
        .collect();

    let rvs = Arc::new(AtomicUsize::new(0));
    let sink_threads: Vec<_> = read_channels
        .into_iter()
        .map(|r_reader| sink(exit.clone(), rvs.clone(), r_reader))
        .collect();
    let start = SystemTime::now();
    let start_val = rvs.load(Ordering::Relaxed);
    sleep(Duration::new(5, 0));
    let elapsed = start.elapsed().unwrap();
    let end_val = rvs.load(Ordering::Relaxed);
    let time = elapsed.as_secs() * 10_000_000_000 + u64::from(elapsed.subsec_nanos());
    let ftime = (time as f64) / 10_000_000_000_f64;
    let fcount = (end_val - start_val) as f64;
    println!("performance: {:?}", fcount / ftime);
    exit.store(true, Ordering::Relaxed);
    for t_reader in read_threads {
        t_reader.join()?;
    }
    for t_producer in producer_threads {
        t_producer.join()?;
    }
    for t_sink in sink_threads {
        t_sink.join()?;
    }
    Ok(())
}
