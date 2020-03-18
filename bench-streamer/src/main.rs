use clap::{crate_description, crate_name, App, Arg};
use solana_streamer::packet::{Packet, Packets, PacketsRecycler, PACKET_DATA_SIZE};
use solana_streamer::streamer::{receiver, PacketReceiver};
use std::cmp::max;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::sleep;
use std::thread::{spawn, JoinHandle, Result};
use std::time::Duration;
use std::time::SystemTime;

fn producer(addr: &SocketAddr, exit: Arc<AtomicBool>) -> JoinHandle<()> {
    let send = UdpSocket::bind("0.0.0.0:0").unwrap();
    let mut msgs = Packets::default();
    msgs.packets.resize(10, Packet::default());
    for w in msgs.packets.iter_mut() {
        w.meta.size = PACKET_DATA_SIZE;
        w.meta.set_addr(&addr);
    }
    let msgs = Arc::new(msgs);
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let mut num = 0;
        for p in &msgs.packets {
            let a = p.meta.addr();
            assert!(p.meta.size < PACKET_DATA_SIZE);
            send.send_to(&p.data[..p.meta.size], &a).unwrap();
            num += 1;
        }
        assert_eq!(num, 10);
    })
}

fn sink(exit: Arc<AtomicBool>, rvs: Arc<AtomicUsize>, r: PacketReceiver) -> JoinHandle<()> {
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let timer = Duration::new(1, 0);
        if let Ok(msgs) = r.recv_timeout(timer) {
            rvs.fetch_add(msgs.packets.len(), Ordering::Relaxed);
        }
    })
}

fn main() -> Result<()> {
    let mut num_sockets = 1usize;

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("num-recv-sockets")
                .long("num-recv-sockets")
                .value_name("NUM")
                .takes_value(true)
                .help("Use NUM receive sockets"),
        )
        .get_matches();

    if let Some(n) = matches.value_of("num-recv-sockets") {
        num_sockets = max(num_sockets, n.to_string().parse().expect("integer"));
    }

    let mut port = 0;
    let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
    let mut addr = SocketAddr::new(ip_addr, 0);

    let exit = Arc::new(AtomicBool::new(false));

    let mut read_channels = Vec::new();
    let mut read_threads = Vec::new();
    let recycler = PacketsRecycler::default();
    for _ in 0..num_sockets {
        let read = solana_net_utils::bind_to(ip_addr, port, false).unwrap();
        read.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        addr = read.local_addr().unwrap();
        port = addr.port();

        let (s_reader, r_reader) = channel();
        read_channels.push(r_reader);
        read_threads.push(receiver(
            Arc::new(read),
            &exit,
            s_reader,
            recycler.clone(),
            "bench-streamer-test",
        ));
    }

    let t_producer1 = producer(&addr, exit.clone());
    let t_producer2 = producer(&addr, exit.clone());
    let t_producer3 = producer(&addr, exit.clone());

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
    t_producer1.join()?;
    t_producer2.join()?;
    t_producer3.join()?;
    for t_sink in sink_threads {
        t_sink.join()?;
    }
    Ok(())
}
