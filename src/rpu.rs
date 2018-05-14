//! The `rpu` module implements the Request Processing Unit, a
//! 5-stage transaction processing pipeline in software.

use accountant::Accountant;
use crdt::{Crdt, ReplicatedData};
use entry::Entry;
use entry_writer::EntryWriter;
use event_processor::EventProcessor;
use packet;
use record_stage::RecordStage;
use request_processor::RequestProcessor;
use request_stage::RequestStage;
use result::Result;
use sig_verify_stage::SigVerifyStage;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{spawn, JoinHandle};
use streamer;

pub struct Rpu {
    event_processor: Arc<EventProcessor>,
}

impl Rpu {
    /// Create a new Rpu that wraps the given Accountant.
    pub fn new(event_processor: EventProcessor) -> Self {
        Rpu {
            event_processor: Arc::new(event_processor),
        }
    }

    fn write_service<W: Write + Send + 'static>(
        accountant: Arc<Accountant>,
        exit: Arc<AtomicBool>,
        broadcast: streamer::BlobSender,
        blob_recycler: packet::BlobRecycler,
        writer: Mutex<W>,
        entry_receiver: Receiver<Entry>,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            let entry_writer = EntryWriter::new(&accountant);
            let _ = entry_writer.write_and_send_entries(
                &broadcast,
                &blob_recycler,
                &writer,
                &entry_receiver,
            );
            if exit.load(Ordering::Relaxed) {
                info!("broadcat_service exiting");
                break;
            }
        })
    }

    /// Create a UDP microservice that forwards messages the given Rpu.
    /// This service is the network leader
    /// Set `exit` to shutdown its threads.
    pub fn serve<W: Write + Send + 'static>(
        &self,
        me: ReplicatedData,
        requests_socket: UdpSocket,
        gossip: UdpSocket,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Result<Vec<JoinHandle<()>>> {
        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        let t_gossip = Crdt::gossip(crdt.clone(), exit.clone());
        let t_listen = Crdt::listen(crdt.clone(), gossip, exit.clone());

        // make sure we are on the same interface
        let mut local = requests_socket.local_addr()?;
        local.set_port(0);

        let packet_recycler = packet::PacketRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_receiver = streamer::receiver(
            requests_socket,
            exit.clone(),
            packet_recycler.clone(),
            packet_sender,
        )?;

        let sig_verify_stage = SigVerifyStage::new(exit.clone(), packet_receiver);

        let blob_recycler = packet::BlobRecycler::default();
        let request_processor = RequestProcessor::new(self.event_processor.accountant.clone());
        let request_stage = RequestStage::new(
            request_processor,
            exit.clone(),
            sig_verify_stage.verified_receiver,
            packet_recycler.clone(),
            blob_recycler.clone(),
        );

        let record_stage = RecordStage::new(
            request_stage.signal_receiver,
            &self.event_processor.start_hash,
            self.event_processor.tick_duration,
        );

        let (broadcast_sender, broadcast_receiver) = channel();
        let t_write = Self::write_service(
            self.event_processor.accountant.clone(),
            exit.clone(),
            broadcast_sender,
            blob_recycler.clone(),
            Mutex::new(writer),
            record_stage.entry_receiver,
        );

        let broadcast_socket = UdpSocket::bind(local)?;
        let t_broadcast = streamer::broadcaster(
            broadcast_socket,
            exit.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            broadcast_receiver,
        );

        let respond_socket = UdpSocket::bind(local.clone())?;
        let t_responder = streamer::responder(
            respond_socket,
            exit.clone(),
            blob_recycler.clone(),
            request_stage.blob_receiver,
        );

        let mut threads = vec![
            t_receiver,
            t_responder,
            request_stage.thread_hdl,
            t_write,
            t_gossip,
            t_listen,
            t_broadcast,
        ];
        threads.extend(sig_verify_stage.thread_hdls.into_iter());
        Ok(threads)
    }
}
