#![feature(test)]

#[macro_use]
extern crate log;
extern crate solana;
extern crate test;

use solana::packet::{Packet, PacketRecycler, BLOB_SIZE, PACKET_DATA_SIZE};
use solana::result::Result;
use solana::streamer::{receiver, PacketReceiver};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use std::time::SystemTime;
use test::Bencher;

fn producer(addr: &SocketAddr, recycler: PacketRecycler, exit: Arc<AtomicBool>) -> JoinHandle<()> {
    let send = UdpSocket::bind("0.0.0.0:0").unwrap();
    let msgs = recycler.allocate();
    let msgs_ = msgs.clone();
    msgs.write().unwrap().packets.resize(10, Packet::default());
    for w in msgs.write().unwrap().packets.iter_mut() {
        w.meta.size = PACKET_DATA_SIZE;
        w.meta.set_addr(&addr);
    }
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let mut num = 0;
        for p in msgs_.read().unwrap().packets.iter() {
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
    rvs: Arc<Mutex<usize>>,
    r: PacketReceiver,
) -> JoinHandle<()> {
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            return;
        }
        let timer = Duration::new(1, 0);
        match r.recv_timeout(timer) {
            Ok(msgs) => {
                *rvs.lock().unwrap() += msgs.read().unwrap().packets.len();
                recycler.recycle(msgs);
            }
            _ => (),
        }
    })
}

fn bench_streamer_with_result() -> Result<()> {
    let read = UdpSocket::bind("127.0.0.1:0")?;
    read.set_read_timeout(Some(Duration::new(1, 0)))?;

    let addr = read.local_addr()?;
    let exit = Arc::new(AtomicBool::new(false));
    let pack_recycler = PacketRecycler::default();

    let (s_reader, r_reader) = channel();
    let t_reader = receiver(read, exit.clone(), pack_recycler.clone(), s_reader);
    let t_producer1 = producer(&addr, pack_recycler.clone(), exit.clone());
    let t_producer2 = producer(&addr, pack_recycler.clone(), exit.clone());
    let t_producer3 = producer(&addr, pack_recycler.clone(), exit.clone());

    let rvs = Arc::new(Mutex::new(0));
    let t_sink = sink(pack_recycler.clone(), exit.clone(), rvs.clone(), r_reader);

    let start = SystemTime::now();
    let start_val = *rvs.lock().unwrap();
    sleep(Duration::new(5, 0));
    let elapsed = start.elapsed().unwrap();
    let end_val = *rvs.lock().unwrap();
    let time = elapsed.as_secs() * 10000000000 + elapsed.subsec_nanos() as u64;
    let ftime = (time as f64) / 10000000000f64;
    let fcount = (end_val - start_val) as f64;
    trace!("performance: {:?}", fcount / ftime);
    exit.store(true, Ordering::Relaxed);
    t_reader.join()?;
    t_producer1.join()?;
    t_producer2.join()?;
    t_producer3.join()?;
    t_sink.join()?;
    Ok(())
}

#[bench]
fn bench_streamer(_bench: &mut Bencher) {
    bench_streamer_with_result().unwrap();
}
