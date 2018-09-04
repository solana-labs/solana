extern crate solana;

use solana::packet::{Packet, PacketRecycler, BLOB_SIZE, PACKET_DATA_SIZE};
use solana::result::Result;
use solana::streamer::{receiver, PacketReceiver};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::sleep;
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use std::time::SystemTime;

fn producer(addr: &SocketAddr, recycler: &PacketRecycler, exit: Arc<AtomicBool>) -> JoinHandle<()> {
    let send = UdpSocket::bind("0.0.0.0:0").unwrap();
    let msgs = recycler.allocate();
    let msgs_ = msgs.clone();
    msgs.write().unwrap().packets.resize(10, Packet::default());
    for w in &mut msgs.write().unwrap().packets {
        w.meta.size = PACKET_DATA_SIZE;
        w.meta.set_addr(&addr);
    }
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let mut num = 0;
        for p in &msgs_.read().unwrap().packets {
            let a = p.meta.addr();
            assert!(p.meta.size < BLOB_SIZE);
            send.send_to(&p.data[..p.meta.size], &a).unwrap();
            num += 1;
        }
        assert_eq!(num, 10);
    })
}

fn sink(
    recycler: PacketRecycler,
    exit: Arc<AtomicBool>,
    rvs: Arc<AtomicUsize>,
    r: PacketReceiver,
) -> JoinHandle<()> {
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let timer = Duration::new(1, 0);
        if let Ok(msgs) = r.recv_timeout(timer) {
            rvs.fetch_add(msgs.read().unwrap().packets.len(), Ordering::Relaxed);
            recycler.recycle(msgs, "sink");
        }
    })
}

fn main() -> Result<()> {
    let read = UdpSocket::bind("127.0.0.1:0")?;
    read.set_read_timeout(Some(Duration::new(1, 0)))?;

    let addr = read.local_addr()?;
    let exit = Arc::new(AtomicBool::new(false));
    let pack_recycler = PacketRecycler::default();

    let (s_reader, r_reader) = channel();
    let t_reader = receiver(
        Arc::new(read),
        exit.clone(),
        pack_recycler.clone(),
        s_reader,
    );
    let t_producer1 = producer(&addr, &pack_recycler, exit.clone());
    let t_producer2 = producer(&addr, &pack_recycler, exit.clone());
    let t_producer3 = producer(&addr, &pack_recycler, exit.clone());

    let rvs = Arc::new(AtomicUsize::new(0));
    let t_sink = sink(pack_recycler.clone(), exit.clone(), rvs.clone(), r_reader);

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
    t_reader.join()?;
    t_producer1.join()?;
    t_producer2.join()?;
    t_producer3.join()?;
    t_sink.join()?;
    Ok(())
}
