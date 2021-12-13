//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!

use {
    crate::{
        packet::{self, send_to, PacketBatch, PacketBatchRecycler, PACKETS_PER_BATCH},
        recvmmsg::NUM_RCVMMSGS,
        socket::SocketAddrSpace,
    },
    solana_sdk::timing::timestamp,
    std::{
        net::UdpSocket,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::{Receiver, RecvTimeoutError, SendError, Sender},
            Arc,
        },
        thread::{Builder, JoinHandle},
        time::{Duration, Instant},
    },
    thiserror::Error,
};

pub type PacketBatchReceiver = Receiver<PacketBatch>;
pub type PacketBatchSender = Sender<PacketBatch>;

#[derive(Error, Debug)]
pub enum StreamerError {
    #[error("I/O error")]
    Io(#[from] std::io::Error),

    #[error("receive timeout error")]
    RecvTimeout(#[from] RecvTimeoutError),

    #[error("send packets error")]
    Send(#[from] SendError<PacketBatch>),
}

pub type Result<T> = std::result::Result<T, StreamerError>;

fn recv_loop(
    sock: &UdpSocket,
    exit: Arc<AtomicBool>,
    channel: &PacketBatchSender,
    recycler: &PacketBatchRecycler,
    name: &'static str,
    coalesce_ms: u64,
    use_pinned_memory: bool,
) -> Result<()> {
    let mut recv_count = 0;
    let mut call_count = 0;
    let mut now = Instant::now();
    let mut num_max_received = 0; // Number of times maximum packets were received
    loop {
        let mut packet_batch = if use_pinned_memory {
            PacketBatch::new_with_recycler(recycler.clone(), PACKETS_PER_BATCH, name)
        } else {
            PacketBatch::with_capacity(PACKETS_PER_BATCH)
        };
        loop {
            // Check for exit signal, even if socket is busy
            // (for instance the leader transaction socket)
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Ok(len) = packet::recv_from(&mut packet_batch, sock, coalesce_ms) {
                if len == NUM_RCVMMSGS {
                    num_max_received += 1;
                }
                recv_count += len;
                call_count += 1;
                if len > 0 {
                    channel.send(packet_batch)?;
                }
                break;
            }
        }
        if recv_count > 1024 {
            datapoint_debug!(
                name,
                ("received", recv_count as i64, i64),
                ("call_count", i64::from(call_count), i64),
                ("elapsed", now.elapsed().as_millis() as i64, i64),
                ("max_received", i64::from(num_max_received), i64),
            );
            recv_count = 0;
            call_count = 0;
            num_max_received = 0;
        }
        now = Instant::now();
    }
}

pub fn receiver(
    sock: Arc<UdpSocket>,
    exit: &Arc<AtomicBool>,
    packet_sender: PacketBatchSender,
    recycler: PacketBatchRecycler,
    name: &'static str,
    coalesce_ms: u64,
    use_pinned_memory: bool,
) -> JoinHandle<()> {
    let res = sock.set_read_timeout(Some(Duration::new(1, 0)));
    assert!(!res.is_err(), "streamer::receiver set_read_timeout error");
    let exit = exit.clone();
    Builder::new()
        .name("solana-receiver".to_string())
        .spawn(move || {
            let _ = recv_loop(
                &sock,
                exit,
                &packet_sender,
                &recycler.clone(),
                name,
                coalesce_ms,
                use_pinned_memory,
            );
        })
        .unwrap()
}

fn recv_send(
    sock: &UdpSocket,
    r: &PacketBatchReceiver,
    socket_addr_space: &SocketAddrSpace,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let packet_batch = r.recv_timeout(timer)?;
    send_to(&packet_batch, sock, socket_addr_space)?;
    Ok(())
}

pub fn recv_packet_batches(
    recvr: &PacketBatchReceiver,
) -> Result<(Vec<PacketBatch>, usize, Duration)> {
    let timer = Duration::new(1, 0);
    let packet_batch = recvr.recv_timeout(timer)?;
    let recv_start = Instant::now();
    trace!("got packets");
    let mut num_packets = packet_batch.packets.len();
    let mut packet_batches = vec![packet_batch];
    while let Ok(packet_batch) = recvr.try_recv() {
        trace!("got more packets");
        num_packets += packet_batch.packets.len();
        packet_batches.push(packet_batch);
    }
    let recv_duration = recv_start.elapsed();
    trace!(
        "packet batches len: {}, num packets: {}",
        packet_batches.len(),
        num_packets
    );
    Ok((packet_batches, num_packets, recv_duration))
}

pub fn responder(
    name: &'static str,
    sock: Arc<UdpSocket>,
    r: PacketBatchReceiver,
    socket_addr_space: SocketAddrSpace,
) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solana-responder-{}", name))
        .spawn(move || {
            let mut errors = 0;
            let mut last_error = None;
            let mut last_print = 0;
            loop {
                if let Err(e) = recv_send(&sock, &r, &socket_addr_space) {
                    match e {
                        StreamerError::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                        StreamerError::RecvTimeout(RecvTimeoutError::Timeout) => (),
                        _ => {
                            errors += 1;
                            last_error = Some(e);
                        }
                    }
                }
                let now = timestamp();
                if now - last_print > 1000 && errors != 0 {
                    datapoint_info!(name, ("errors", errors, i64),);
                    info!("{} last-error: {:?} count: {}", name, last_error, errors);
                    last_print = now;
                    errors = 0;
                }
            }
        })
        .unwrap()
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{
            packet::{Packet, PacketBatch, PACKET_DATA_SIZE},
            streamer::{receiver, responder},
        },
        solana_perf::recycler::Recycler,
        std::{
            io,
            io::Write,
            net::UdpSocket,
            sync::{
                atomic::{AtomicBool, Ordering},
                mpsc::channel,
                Arc,
            },
            time::Duration,
        },
    };

    fn get_packet_batches(r: PacketBatchReceiver, num_packets: &mut usize) {
        for _ in 0..10 {
            let packet_batch_res = r.recv_timeout(Duration::new(1, 0));
            if packet_batch_res.is_err() {
                continue;
            }

            *num_packets -= packet_batch_res.unwrap().packets.len();

            if *num_packets == 0 {
                break;
            }
        }
    }

    #[test]
    fn streamer_debug() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", PacketBatch::default()).unwrap();
    }
    #[test]
    fn streamer_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        read.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(
            Arc::new(read),
            &exit,
            s_reader,
            Recycler::default(),
            "test",
            1,
            true,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let t_responder = responder(
                "streamer_send_test",
                Arc::new(send),
                r_responder,
                SocketAddrSpace::Unspecified,
            );
            let mut packet_batch = PacketBatch::default();
            for i in 0..5 {
                let mut p = Packet::default();
                {
                    p.data[0] = i as u8;
                    p.meta.size = PACKET_DATA_SIZE;
                    p.meta.set_addr(&addr);
                }
                packet_batch.packets.push(p);
            }
            s_responder.send(packet_batch).expect("send");
            t_responder
        };

        let mut packets_remaining = 5;
        get_packet_batches(r_reader, &mut packets_remaining);
        assert_eq!(packets_remaining, 0);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
}
