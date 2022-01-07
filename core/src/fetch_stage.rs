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
        packet::{ExtendedPacket, Packet, PacketInterface},
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

// todo: do we want all of these to use the type P or
// should some of these always be the standard packets (e.g. the vote sender/receiver)
impl FetchStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        tpu_tx_sockets: Vec<UdpSocket>,
        tpu_tx_forwards_sockets: Vec<UdpSocket>,
        tpu_tx_extended_sockets: Vec<UdpSocket>,
        tpu_tx_extended_forwards_sockets: Vec<UdpSocket>,
        tpu_vote_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        coalesce_ms: u64,
    ) -> (
        Self,
        PacketBatchReceiver<Packet>,
        PacketBatchReceiver<ExtendedPacket>,
        PacketBatchReceiver<Packet>,
    ) {
        let (tx_sender, tx_receiver) = channel();
        let (extended_sender, extended_receiver) = channel();
        let (vote_sender, vote_receiver) = channel();
        (
            Self::new_with_sender(
                tpu_tx_sockets,
                tpu_tx_forwards_sockets,
                tpu_tx_extended_sockets,
                tpu_tx_extended_forwards_sockets,
                tpu_vote_sockets,
                exit,
                &tx_sender,
                &extended_sender,
                &vote_sender,
                poh_recorder,
                coalesce_ms,
            ),
            tx_receiver,
            extended_receiver,
            vote_receiver,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_sender(
        tpu_tx_sockets: Vec<UdpSocket>,
        tpu_tx_forwards_sockets: Vec<UdpSocket>,
        tpu_tx_extended_sockets: Vec<UdpSocket>,
        tpu_tx_extended_forwards_sockets: Vec<UdpSocket>,
        tpu_vote_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        tx_sender: &PacketBatchSender<Packet>,
        tx_extended_sender: &PacketBatchSender<ExtendedPacket>,
        vote_sender: &PacketBatchSender<Packet>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        coalesce_ms: u64,
    ) -> Self {
        let tpu_tx_sockets = tpu_tx_sockets.into_iter().map(Arc::new).collect();
        let tpu_tx_forwards_sockets = tpu_tx_forwards_sockets.into_iter().map(Arc::new).collect();
        let tpu_tx_extended_sockets = tpu_tx_extended_sockets.into_iter().map(Arc::new).collect();
        let tpu_tx_extended_forwards_sockets = tpu_tx_extended_forwards_sockets
            .into_iter()
            .map(Arc::new)
            .collect();
        let tpu_vote_sockets = tpu_vote_sockets.into_iter().map(Arc::new).collect();

        Self::new_multi_socket(
            tpu_tx_sockets,
            tpu_tx_forwards_sockets,
            tpu_tx_extended_sockets,
            tpu_tx_extended_forwards_sockets,
            tpu_vote_sockets,
            exit,
            tx_sender,
            tx_extended_sender,
            vote_sender,
            poh_recorder,
            coalesce_ms,
        )
    }

    fn new_handle_forwarded_packets_thread<P: 'static + PacketInterface>(
        receiver: PacketBatchReceiver<P>,
        sender: PacketBatchSender<P>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
    ) -> JoinHandle<()> {
        Builder::new()
            .name("solana-fetch-stage-fwd-rcvr".to_string())
            .spawn(move || loop {
                if let Err(e) = Self::handle_forwarded_packets(&receiver, &sender, &poh_recorder) {
                    match e {
                        Error::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeout(RecvTimeoutError::Timeout) => (),
                        Error::Recv(_) => break,
                        Error::Send => break,
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap()
    }

    fn handle_forwarded_packets<P: PacketInterface>(
        receiver: &PacketBatchReceiver<P>,
        sender: &PacketBatchSender<P>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> Result<()> {
        let mark_forwarded = |packet: &mut P| {
            packet.get_meta_mut().forwarded = true;
        };

        let mut packet_batch = receiver.recv()?;
        let mut num_packets = packet_batch.packets.len();
        packet_batch.packets.iter_mut().for_each(mark_forwarded);
        let mut packet_batches = vec![packet_batch];
        while let Ok(mut packet_batch) = receiver.try_recv() {
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
                if sender.send(packet_batch).is_err() {
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
        tpu_tx_extended_sockets: Vec<Arc<UdpSocket>>,
        tpu_tx_extended_forwards_sockets: Vec<Arc<UdpSocket>>,
        tpu_vote_sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        tx_sender: &PacketBatchSender<Packet>,
        tx_extended_sender: &PacketBatchSender<ExtendedPacket>,
        vote_sender: &PacketBatchSender<Packet>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
        coalesce_ms: u64,
    ) -> Self {
        let recycler: PacketBatchRecycler<Packet> = Recycler::warmed(1000, 1024);
        let extended_recycler: PacketBatchRecycler<ExtendedPacket> = Recycler::warmed(1000, 1024);

        let tpu_threads = tpu_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                exit,
                tx_sender.clone(),
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

        let tpu_extended_threads = tpu_tx_extended_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                exit,
                tx_extended_sender.clone(),
                extended_recycler.clone(),
                "fetch_extended_stage",
                coalesce_ms,
                true,
            )
        });

        let (extended_forward_sender, extended_forward_receiver) = channel();
        let tpu_extended_forwards_threads =
            tpu_tx_extended_forwards_sockets.into_iter().map(|socket| {
                streamer::receiver(
                    socket,
                    exit,
                    extended_forward_sender.clone(),
                    extended_recycler.clone(),
                    "fetch_forward_extended_stage",
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

        let forward_thread_hdl = Self::new_handle_forwarded_packets_thread(
            forward_receiver,
            tx_sender.clone(),
            poh_recorder.clone(),
        );

        let forward_extended_thread_hdl = Self::new_handle_forwarded_packets_thread(
            extended_forward_receiver,
            tx_extended_sender.clone(),
            poh_recorder.clone(),
        );

        let mut thread_hdls: Vec<_> = tpu_threads
            .chain(tpu_forwards_threads)
            .chain(tpu_vote_threads)
            .chain(tpu_extended_threads)
            .chain(tpu_extended_forwards_threads)
            .collect();
        thread_hdls.push(forward_thread_hdl);
        thread_hdls.push(forward_extended_thread_hdl);

        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
