//! The `fetch_stage` batches input from a UDP socket and sends it to a channel.

use crate::banking_stage::FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET;
use crate::packet::PacketsRecycler;
use crate::poh_recorder::PohRecorder;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::{self, PacketReceiver, PacketSender};
use solana_metrics::{inc_new_counter_debug, inc_new_counter_info};
use solana_perf::recycler::Recycler;
use solana_sdk::clock::DEFAULT_TICKS_PER_SLOT;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, Builder, JoinHandle};

pub struct FetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl FetchStage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        sockets: Vec<UdpSocket>,
        tpu_forwards_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> (Self, PacketReceiver) {
        let (sender, receiver) = channel();
        (
            Self::new_with_sender(sockets, tpu_forwards_sockets, exit, &sender, &poh_recorder),
            receiver,
        )
    }
    pub fn new_with_sender(
        sockets: Vec<UdpSocket>,
        tpu_forwards_sockets: Vec<UdpSocket>,
        exit: &Arc<AtomicBool>,
        sender: &PacketSender,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> Self {
        let tx_sockets = sockets.into_iter().map(Arc::new).collect();
        let tpu_forwards_sockets = tpu_forwards_sockets.into_iter().map(Arc::new).collect();
        Self::new_multi_socket(
            tx_sockets,
            tpu_forwards_sockets,
            exit,
            &sender,
            &poh_recorder,
        )
    }

    fn handle_forwarded_packets(
        recvr: &PacketReceiver,
        sendr: &PacketSender,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> Result<()> {
        let msgs = recvr.recv()?;
        let mut len = msgs.packets.len();
        let mut batch = vec![msgs];
        while let Ok(more) = recvr.try_recv() {
            len += more.packets.len();
            batch.push(more);
            // Read at most 1K transactions in a loop
            if len > 1024 {
                break;
            }
        }

        if poh_recorder.lock().unwrap().would_be_leader(
            FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET
                .saturating_add(1)
                .saturating_mul(DEFAULT_TICKS_PER_SLOT),
        ) {
            inc_new_counter_debug!("fetch_stage-honor_forwards", len);
            for packets in batch {
                if sendr.send(packets).is_err() {
                    return Err(Error::SendError);
                }
            }
        } else {
            inc_new_counter_info!("fetch_stage-discard_forwards", len);
        }

        Ok(())
    }

    fn new_multi_socket(
        sockets: Vec<Arc<UdpSocket>>,
        tpu_forwards_sockets: Vec<Arc<UdpSocket>>,
        exit: &Arc<AtomicBool>,
        sender: &PacketSender,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> Self {
        let recycler: PacketsRecycler = Recycler::warmed(1000, 1024);

        let tpu_threads = sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                &exit,
                sender.clone(),
                recycler.clone(),
                "fetch_stage",
            )
        });

        let (forward_sender, forward_receiver) = channel();
        let tpu_forwards_threads = tpu_forwards_sockets.into_iter().map(|socket| {
            streamer::receiver(
                socket,
                &exit,
                forward_sender.clone(),
                recycler.clone(),
                "fetch_forward_stage",
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
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        Error::RecvError(_) => break,
                        Error::SendError => break,
                        _ => error!("{:?}", e),
                    }
                }
            })
            .unwrap();

        let mut thread_hdls: Vec<_> = tpu_threads.chain(tpu_forwards_threads).collect();
        thread_hdls.push(fwd_thread_hdl);
        Self { thread_hdls }
    }
}

impl Service for FetchStage {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
