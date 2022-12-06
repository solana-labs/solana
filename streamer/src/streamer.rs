//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!

use {
    crate::{
        packet::{self, PacketBatch, PacketBatchRecycler, PACKETS_PER_BATCH},
        sendmmsg::{batch_send, SendPktsError},
        socket::SocketAddrSpace,
    },
    crossbeam_channel::{Receiver, RecvTimeoutError, SendError, Sender},
    histogram::Histogram,
    solana_sdk::{packet::Packet, pubkey::Pubkey, timing::timestamp},
    std::{
        cmp::Reverse,
        collections::HashMap,
        net::{IpAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
    thiserror::Error,
};

// Total stake and nodes => stake map
#[derive(Default)]
pub struct StakedNodes {
    pub total_stake: u64,
    pub max_stake: u64,
    pub min_stake: u64,
    pub ip_stake_map: HashMap<IpAddr, u64>,
    pub pubkey_stake_map: HashMap<Pubkey, u64>,
}

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

    #[error(transparent)]
    SendPktsError(#[from] SendPktsError),
}

pub struct StreamerReceiveStats {
    pub name: &'static str,
    pub packets_count: AtomicUsize,
    pub packet_batches_count: AtomicUsize,
    pub full_packet_batches_count: AtomicUsize,
    pub max_channel_len: AtomicUsize,
}

impl StreamerReceiveStats {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            packets_count: AtomicUsize::default(),
            packet_batches_count: AtomicUsize::default(),
            full_packet_batches_count: AtomicUsize::default(),
            max_channel_len: AtomicUsize::default(),
        }
    }

    pub fn report(&self) {
        datapoint_info!(
            self.name,
            (
                "packets_count",
                self.packets_count.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "packet_batches_count",
                self.packet_batches_count.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "full_packet_batches_count",
                self.full_packet_batches_count.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
            (
                "channel_len",
                self.max_channel_len.swap(0, Ordering::Relaxed) as i64,
                i64
            ),
        );
    }
}

pub type Result<T> = std::result::Result<T, StreamerError>;

fn recv_loop(
    socket: &UdpSocket,
    exit: Arc<AtomicBool>,
    packet_batch_sender: &PacketBatchSender,
    recycler: &PacketBatchRecycler,
    stats: &StreamerReceiveStats,
    coalesce_ms: u64,
    use_pinned_memory: bool,
    in_vote_only_mode: Option<Arc<AtomicBool>>,
) -> Result<()> {
    loop {
        let mut packet_batch = if use_pinned_memory {
            PacketBatch::new_with_recycler(recycler.clone(), PACKETS_PER_BATCH, stats.name)
        } else {
            PacketBatch::with_capacity(PACKETS_PER_BATCH)
        };
        loop {
            // Check for exit signal, even if socket is busy
            // (for instance the leader transaction socket)
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            if let Some(ref in_vote_only_mode) = in_vote_only_mode {
                if in_vote_only_mode.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(1));
                    continue;
                }
            }

            if let Ok(len) = packet::recv_from(&mut packet_batch, socket, coalesce_ms) {
                if len > 0 {
                    let StreamerReceiveStats {
                        packets_count,
                        packet_batches_count,
                        full_packet_batches_count,
                        max_channel_len,
                        ..
                    } = stats;

                    packets_count.fetch_add(len, Ordering::Relaxed);
                    packet_batches_count.fetch_add(1, Ordering::Relaxed);
                    max_channel_len.fetch_max(packet_batch_sender.len(), Ordering::Relaxed);
                    if len == PACKETS_PER_BATCH {
                        full_packet_batches_count.fetch_add(1, Ordering::Relaxed);
                    }

                    packet_batch_sender.send(packet_batch)?;
                }
                break;
            }
        }
    }
}

pub fn receiver(
    socket: Arc<UdpSocket>,
    exit: Arc<AtomicBool>,
    packet_batch_sender: PacketBatchSender,
    recycler: PacketBatchRecycler,
    stats: Arc<StreamerReceiveStats>,
    coalesce_ms: u64,
    use_pinned_memory: bool,
    in_vote_only_mode: Option<Arc<AtomicBool>>,
) -> JoinHandle<()> {
    let res = socket.set_read_timeout(Some(Duration::new(1, 0)));
    assert!(res.is_ok(), "streamer::receiver set_read_timeout error");
    Builder::new()
        .name("solReceiver".to_string())
        .spawn(move || {
            let _ = recv_loop(
                &socket,
                exit,
                &packet_batch_sender,
                &recycler,
                &stats,
                coalesce_ms,
                use_pinned_memory,
                in_vote_only_mode,
            );
        })
        .unwrap()
}

#[derive(Debug, Default)]
struct SendStats {
    bytes: u64,
    count: u64,
}

#[derive(Default)]
struct StreamerSendStats {
    host_map: HashMap<IpAddr, SendStats>,
    since: Option<Instant>,
}

impl StreamerSendStats {
    fn report_stats(
        name: &'static str,
        host_map: HashMap<IpAddr, SendStats>,
        sample_duration: Option<Duration>,
    ) {
        const MAX_REPORT_ENTRIES: usize = 5;
        let sample_ms = sample_duration.map(|d| d.as_millis()).unwrap_or_default();
        let mut hist = Histogram::default();
        let mut byte_sum = 0;
        let mut pkt_count = 0;
        host_map.iter().for_each(|(_addr, host_stats)| {
            hist.increment(host_stats.bytes).unwrap();
            byte_sum += host_stats.bytes;
            pkt_count += host_stats.count;
        });

        datapoint_info!(
            name,
            ("streamer-send-sample_duration_ms", sample_ms, i64),
            ("streamer-send-host_count", host_map.len(), i64),
            ("streamer-send-bytes_total", byte_sum, i64),
            ("streamer-send-pkt_count_total", pkt_count, i64),
            (
                "streamer-send-host_bytes_min",
                hist.minimum().unwrap_or_default(),
                i64
            ),
            (
                "streamer-send-host_bytes_max",
                hist.maximum().unwrap_or_default(),
                i64
            ),
            (
                "streamer-send-host_bytes_mean",
                hist.mean().unwrap_or_default(),
                i64
            ),
            (
                "streamer-send-host_bytes_90pct",
                hist.percentile(90.0).unwrap_or_default(),
                i64
            ),
            (
                "streamer-send-host_bytes_50pct",
                hist.percentile(50.0).unwrap_or_default(),
                i64
            ),
            (
                "streamer-send-host_bytes_10pct",
                hist.percentile(10.0).unwrap_or_default(),
                i64
            ),
        );

        let num_entries = host_map.len();
        let mut entries: Vec<_> = host_map.into_iter().collect();
        if entries.len() > MAX_REPORT_ENTRIES {
            entries.select_nth_unstable_by_key(MAX_REPORT_ENTRIES, |(_addr, stats)| {
                Reverse(stats.bytes)
            });
            entries.truncate(MAX_REPORT_ENTRIES);
        }
        info!(
            "streamer send {} hosts: count:{} {:?}",
            name, num_entries, entries,
        );
    }

    fn maybe_submit(&mut self, name: &'static str, sender: &Sender<Box<dyn FnOnce() + Send>>) {
        const SUBMIT_CADENCE: Duration = Duration::from_secs(10);
        const MAP_SIZE_REPORTING_THRESHOLD: usize = 1_000;
        let elapsed = self.since.as_ref().map(Instant::elapsed);
        if elapsed.map(|e| e < SUBMIT_CADENCE).unwrap_or_default()
            && self.host_map.len() < MAP_SIZE_REPORTING_THRESHOLD
        {
            return;
        }

        let host_map = std::mem::take(&mut self.host_map);
        let _ = sender.send(Box::new(move || {
            Self::report_stats(name, host_map, elapsed);
        }));

        *self = Self {
            since: Some(Instant::now()),
            ..Self::default()
        };
    }

    fn record(&mut self, pkt: &Packet) {
        let ent = self.host_map.entry(pkt.meta().addr).or_default();
        ent.count += 1;
        ent.bytes += pkt.data(..).map(<[u8]>::len).unwrap_or_default() as u64;
    }
}

fn recv_send(
    sock: &UdpSocket,
    r: &PacketBatchReceiver,
    socket_addr_space: &SocketAddrSpace,
    stats: &mut Option<StreamerSendStats>,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let packet_batch = r.recv_timeout(timer)?;
    if let Some(stats) = stats {
        packet_batch.iter().for_each(|p| stats.record(p));
    }
    let packets = packet_batch.iter().filter_map(|pkt| {
        let addr = pkt.meta().socket_addr();
        let data = pkt.data(..)?;
        socket_addr_space.check(&addr).then_some((data, addr))
    });
    batch_send(sock, &packets.collect::<Vec<_>>())?;
    Ok(())
}

pub fn recv_vec_packet_batches(
    recvr: &Receiver<Vec<PacketBatch>>,
) -> Result<(Vec<PacketBatch>, usize, Duration)> {
    let timer = Duration::new(1, 0);
    let mut packet_batches = recvr.recv_timeout(timer)?;
    let recv_start = Instant::now();
    trace!("got packets");
    let mut num_packets = packet_batches
        .iter()
        .map(|packets| packets.len())
        .sum::<usize>();
    while let Ok(packet_batch) = recvr.try_recv() {
        trace!("got more packets");
        num_packets += packet_batch
            .iter()
            .map(|packets| packets.len())
            .sum::<usize>();
        packet_batches.extend(packet_batch);
    }
    let recv_duration = recv_start.elapsed();
    trace!(
        "packet batches len: {}, num packets: {}",
        packet_batches.len(),
        num_packets
    );
    Ok((packet_batches, num_packets, recv_duration))
}

pub fn recv_packet_batches(
    recvr: &PacketBatchReceiver,
) -> Result<(Vec<PacketBatch>, usize, Duration)> {
    let timer = Duration::new(1, 0);
    let packet_batch = recvr.recv_timeout(timer)?;
    let recv_start = Instant::now();
    trace!("got packets");
    let mut num_packets = packet_batch.len();
    let mut packet_batches = vec![packet_batch];
    while let Ok(packet_batch) = recvr.try_recv() {
        trace!("got more packets");
        num_packets += packet_batch.len();
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
    stats_reporter_sender: Option<Sender<Box<dyn FnOnce() + Send>>>,
) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solRspndr{name}"))
        .spawn(move || {
            let mut errors = 0;
            let mut last_error = None;
            let mut last_print = 0;
            let mut stats = None;

            if stats_reporter_sender.is_some() {
                stats = Some(StreamerSendStats::default());
            }

            loop {
                if let Err(e) = recv_send(&sock, &r, &socket_addr_space, &mut stats) {
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
                if let Some(ref stats_reporter_sender) = stats_reporter_sender {
                    if let Some(ref mut stats) = stats {
                        stats.maybe_submit(name, stats_reporter_sender);
                    }
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
        crossbeam_channel::unbounded,
        solana_perf::recycler::Recycler,
        std::{
            io,
            io::Write,
            net::UdpSocket,
            sync::{
                atomic::{AtomicBool, Ordering},
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

            *num_packets -= packet_batch_res.unwrap().len();

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
        let (s_reader, r_reader) = unbounded();
        let stats = Arc::new(StreamerReceiveStats::new("test"));
        let t_receiver = receiver(
            Arc::new(read),
            exit.clone(),
            s_reader,
            Recycler::default(),
            stats.clone(),
            1,
            true,
            None,
        );
        const NUM_PACKETS: usize = 5;
        let t_responder = {
            let (s_responder, r_responder) = unbounded();
            let t_responder = responder(
                "SendTest",
                Arc::new(send),
                r_responder,
                SocketAddrSpace::Unspecified,
                None,
            );
            let mut packet_batch = PacketBatch::default();
            for i in 0..NUM_PACKETS {
                let mut p = Packet::default();
                {
                    p.buffer_mut()[0] = i as u8;
                    p.meta_mut().size = PACKET_DATA_SIZE;
                    p.meta_mut().set_socket_addr(&addr);
                }
                packet_batch.push(p);
            }
            s_responder.send(packet_batch).expect("send");
            t_responder
        };

        let mut packets_remaining = NUM_PACKETS;
        get_packet_batches(r_reader, &mut packets_remaining);
        assert_eq!(packets_remaining, 0);
        exit.store(true, Ordering::Relaxed);
        assert!(stats.packet_batches_count.load(Ordering::Relaxed) >= 1);
        assert_eq!(stats.packets_count.load(Ordering::Relaxed), NUM_PACKETS);
        assert_eq!(stats.full_packet_batches_count.load(Ordering::Relaxed), 0);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
}
