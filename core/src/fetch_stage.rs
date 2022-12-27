//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use {
    crate::result::{Error, Result},
    crossbeam_channel::{unbounded, RecvTimeoutError},
    solana_metrics::{inc_new_counter_debug, inc_new_counter_info},
    solana_perf::{packet::PacketBatchRecycler, recycler::Recycler},
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{
        clock::{DEFAULT_TICKS_PER_SLOT, HOLD_TRANSACTIONS_SLOT_OFFSET},
        packet::{Packet, PacketFlags},
    },
    solana_streamer::streamer::{
        self, PacketBatchReceiver, PacketBatchSender, StreamerReceiveStats,
    },
    solana_tpu_client::tpu_connection_cache::DEFAULT_TPU_ENABLE_UDP,
    std::{
        net::UdpSocket,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct FetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl FetchStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        sockets: Vec<UdpSocket>,
        tpu_forwards_sockets: Vec<UdpSocket>,
        tpu_vote_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        coalesce_ms: u64,
    ) -> (Self, PacketBatchReceiver, PacketBatchReceiver) {
        let (sender, receiver) = unbounded();
        let (vote_sender, vote_receiver) = unbounded();
        let (forward_sender, forward_receiver) = unbounded();
        (
            Self::new_with_sender(
                sockets,
                tpu_forwards_sockets,
                tpu_vote_sockets,
                exit,
                &sender,
                &vote_sender,
                &forward_sender,
                forward_receiver,
                poh_recorder,
                coalesce_ms,
                None,
                DEFAULT_TPU_ENABLE_UDP,
            ),
            receiver,
            vote_receiver,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_sender(
        sockets: Vec<UdpSocket>,
        tpu_forwards_sockets: Vec<UdpSocket>,
        tpu_vote_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        sender: &PacketBatchSender,
        vote_sender: &PacketBatchSender,
        forward_sender: &PacketBatchSender,
        forward_receiver: PacketBatchReceiver,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        coalesce_ms: u64,
        in_vote_only_mode: Option<Arc<AtomicBool>>,
        tpu_enable_udp: bool,
    ) -> Self {
        let tx_sockets = sockets.into_iter().map(Arc::new).collect();
        let tpu_forwards_sockets = tpu_forwards_sockets.into_iter().map(Arc::new).collect();
        let tpu_vote_sockets = tpu_vote_sockets.into_iter().map(Arc::new).collect();
        Self::new_multi_socket(
            tx_sockets,
            tpu_forwards_sockets,
            tpu_vote_sockets,
            exit,
            sender,
            vote_sender,
            forward_sender,
            forward_receiver,
            poh_recorder,
            coalesce_ms,
            in_vote_only_mode,
            tpu_enable_udp,
        )
    }

    fn handle_forwarded_packets(
        recvr: &PacketBatchReceiver,
        sendr: &PacketBatchSender,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
    ) -> Result<()> {
        let mark_forwarded = |packet: &mut Packet| {
            packet.meta_mut().flags |= PacketFlags::FORWARDED;
        };

        let mut packet_batch = recvr.recv()?;
        let mut num_packets = packet_batch.len();
        packet_batch.iter_mut().for_each(mark_forwarded);
        let mut packet_batches = vec![packet_batch];
        while let Ok(mut packet_batch) = recvr.try_recv() {
            packet_batch.iter_mut().for_each(mark_forwarded);
            num_packets += packet_batch.len();
            packet_batches.push(packet_batch);
            // Read at most 1K transactions in a loop
            if num_packets > 1024 {
                break;
            }
        }

        if poh_recorder
            .read()
            .unwrap()
            .would_be_leader(HOLD_TRANSACTIONS_SLOT_OFFSET.saturating_mul(DEFAULT_TICKS_PER_SLOT))
        {
            inc_new_counter_debug!("fetch_stage-honor_forwards", num_packets);
            for packet_batch in packet_batches {
                #[allow(clippy::question_mark)]
                if sendr.send(packet_batch).is_err() {
                    return Err(Error::Send);
                }
            }
        } else {
            inc_new_counter_info!("fetch_stage-discard_forwards", num_packets);
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn new_multi_socket(
        tpu_sockets: Vec<Arc<UdpSocket>>,
        tpu_forwards_sockets: Vec<Arc<UdpSocket>>,
        tpu_vote_sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: &PacketBatchSender,
        vote_sender: &PacketBatchSender,
        forward_sender: &PacketBatchSender,
        forward_receiver: PacketBatchReceiver,
        poh_recorder: &Arc<RwLock<PohRecorder>>,
        coalesce_ms: u64,
        in_vote_only_mode: Option<Arc<AtomicBool>>,
        tpu_enable_udp: bool,
    ) -> Self {
        let recycler: PacketBatchRecycler = Recycler::warmed(1000, 1024);

        let tpu_stats = Arc::new(StreamerReceiveStats::new("tpu_receiver"));

        let tpu_threads: Vec<_> = if tpu_enable_udp {
            tpu_sockets
                .into_iter()
                .map(|socket| {
                    streamer::receiver(
                        socket,
                        exit.clone(),
                        sender.clone(),
                        recycler.clone(),
                        tpu_stats.clone(),
                        coalesce_ms,
                        true,
                        in_vote_only_mode.clone(),
                    )
                })
                .collect()
        } else {
            Vec::default()
        };

        let tpu_forward_stats = Arc::new(StreamerReceiveStats::new("tpu_forwards_receiver"));
        let tpu_forwards_threads: Vec<_> = if tpu_enable_udp {
            tpu_forwards_sockets
                .into_iter()
                .map(|socket| {
                    streamer::receiver(
                        socket,
                        exit.clone(),
                        forward_sender.clone(),
                        recycler.clone(),
                        tpu_forward_stats.clone(),
                        coalesce_ms,
                        true,
                        in_vote_only_mode.clone(),
                    )
                })
                .collect()
        } else {
            Vec::default()
        };

        let tpu_vote_stats = Arc::new(StreamerReceiveStats::new("tpu_vote_receiver"));
        let tpu_vote_threads: Vec<_> = tpu_vote_sockets
            .into_iter()
            .map(|socket| {
                streamer::receiver(
                    socket,
                    exit.clone(),
                    vote_sender.clone(),
                    recycler.clone(),
                    tpu_vote_stats.clone(),
                    coalesce_ms,
                    true,
                    None,
                )
            })
            .collect();

        let sender = sender.clone();
        let poh_recorder = poh_recorder.clone();

        let fwd_thread_hdl = Builder::new()
            .name("solFetchStgFwRx".to_string())
            .spawn(move || loop {
                if let Err(e) =
                    Self::handle_forwarded_packets(&forward_receiver, &sender, &poh_recorder)
                {
                    match e {
                        Error::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeout(RecvTimeoutError::Timeout) => (),
                        Error::Recv(_) => break,
                        Error::Send => break,
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();

        let exit = exit.clone();
        let metrics_thread_hdl = Builder::new()
            .name("solFetchStgMetr".to_string())
            .spawn(move || loop {
                sleep(Duration::from_secs(1));

                tpu_stats.report();
                tpu_vote_stats.report();
                tpu_forward_stats.report();

                if exit.load(Ordering::Relaxed) {
                    return;
                }
            })
            .unwrap();

        Self {
            thread_hdls: [
                tpu_threads,
                tpu_forwards_threads,
                tpu_vote_threads,
                vec![fwd_thread_hdl, metrics_thread_hdl],
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
