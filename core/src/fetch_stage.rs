//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use {
    crate::{
        banking_stage::HOLD_TRANSACTIONS_SLOT_OFFSET,
        result::{Error, Result},
    },
    solana_metrics::{inc_new_counter_debug, inc_new_counter_info},
    solana_perf::{packet::PacketBatchRecycler, recycler::Recycler},
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{
        clock::DEFAULT_TICKS_PER_SLOT,
        packet::{Packet, PacketFlags},
    },
    solana_streamer::streamer::{self, PacketBatchReceiver, PacketBatchSender},
    std::{
        net::UdpSocket,
        sync::{
            atomic::AtomicBool,
            mpsc::{channel, RecvTimeoutError},
            Arc, Mutex,
        },
        thread::{self, Builder, JoinHandle},
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
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        coalesce_ms: u64,
    ) -> (Self, PacketBatchReceiver, PacketBatchReceiver) {
        let (sender, receiver) = channel();
        let (vote_sender, vote_receiver) = channel();
        (
            Self::new_with_sender(
                sockets,
                tpu_forwards_sockets,
                tpu_vote_sockets,
                exit,
                &sender,
                &vote_sender,
                poh_recorder,
                coalesce_ms,
            ),
            receiver,
            vote_receiver,
        )
    }

    pub fn new_with_sender(
        sockets: Vec<UdpSocket>,
        tpu_forwards_sockets: Vec<UdpSocket>,
        tpu_vote_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        sender: &PacketBatchSender,
        vote_sender: &PacketBatchSender,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        coalesce_ms: u64,
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
            poh_recorder,
            coalesce_ms,
        )
    }

    fn handle_forwarded_packets(
        recvr: &PacketBatchReceiver,
        sendr: &PacketBatchSender,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> Result<()> {
        let mark_forwarded = |packet: &mut Packet| {
            packet.meta.flags |= PacketFlags::FORWARDED;
        };

        let mut packet_batch = recvr.recv()?;
        let mut num_packets = packet_batch.packets.len();
        packet_batch.packets.iter_mut().for_each(mark_forwarded);
        let mut packet_batches = vec![packet_batch];
        while let Ok(mut packet_batch) = recvr.try_recv() {
            packet_batch.packets.iter_mut().for_each(mark_forwarded);
            num_packets += packet_batch.packets.len();
            packet_batches.push(packet_batch);
            // Read at most 1K transactions in a loop
            if num_packets > 1024 {
                break;
            }
        }

        if poh_recorder
            .lock()
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

    fn new_multi_socket(
        tpu_sockets: Vec<Arc<UdpSocket>>,
        tpu_forwards_sockets: Vec<Arc<UdpSocket>>,
        tpu_vote_sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: &PacketBatchSender,
        vote_sender: &PacketBatchSender,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        coalesce_ms: u64,
    ) -> Self {
        let recycler: PacketBatchRecycler = Recycler::warmed(1000, 1024);

        let tpu_threads = tpu_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                exit,
                sender.clone(),
                recycler.clone(),
                "fetch_stage",
                coalesce_ms,
                true,
            )
        });

        let (forward_sender, forward_receiver) = channel();
        let tpu_forwards_threads = tpu_forwards_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                exit,
                forward_sender.clone(),
                recycler.clone(),
                "fetch_forward_stage",
                coalesce_ms,
                true,
            )
        });

        let tpu_vote_threads = tpu_vote_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                exit,
                vote_sender.clone(),
                recycler.clone(),
                "fetch_vote_stage",
                coalesce_ms,
                true,
            )
        });

        let sender = sender.clone();
        let poh_recorder = poh_recorder.clone();

        let fwd_thread_hdl = Builder::new()
            .name("solana-fetch-stage-fwd-rcvr".to_string())
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

        let mut thread_hdls: Vec<_> = tpu_threads
            .chain(tpu_forwards_threads)
            .chain(tpu_vote_threads)
            .collect();
        thread_hdls.push(fwd_thread_hdl);
        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
