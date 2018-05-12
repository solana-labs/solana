//! The `tpu` module implements the Transaction Processing Unit, a
//! 5-stage transaction processing pipeline in software.

use accounting_stage::AccountingStage;
use crdt::{Crdt, ReplicatedData};
use entry_writer::EntryWriter;
use packet;
use request_stage::{RequestProcessor, RequestStage};
use result::Result;
use sig_verify_stage::SigVerifyStage;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{spawn, JoinHandle};
use streamer;

pub struct Tpu {
    accounting_stage: Arc<AccountingStage>,
    request_processor: Arc<RequestProcessor>,
}

impl Tpu {
    /// Create a new Tpu that wraps the given Accountant.
    pub fn new(accounting_stage: AccountingStage) -> Self {
        let request_processor = RequestProcessor::new(accounting_stage.accountant.clone());
        Tpu {
            accounting_stage: Arc::new(accounting_stage),
            request_processor: Arc::new(request_processor),
        }
    }

    pub fn write_service<W: Write + Send + 'static>(
        accounting_stage: Arc<AccountingStage>,
        request_processor: Arc<RequestProcessor>,
        exit: Arc<AtomicBool>,
        broadcast: streamer::BlobSender,
        blob_recycler: packet::BlobRecycler,
        writer: Mutex<W>,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            let entry_writer = EntryWriter::new(&accounting_stage, &request_processor);
            let _ = entry_writer.write_and_send_entries(&broadcast, &blob_recycler, &writer);
            if exit.load(Ordering::Relaxed) {
                info!("broadcat_service exiting");
                break;
            }
        })
    }

    pub fn drain_service(
        accounting_stage: Arc<AccountingStage>,
        request_processor: Arc<RequestProcessor>,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        spawn(move || {
            let entry_writer = EntryWriter::new(&accounting_stage, &request_processor);
            loop {
                let _ = entry_writer.drain_entries();
                if exit.load(Ordering::Relaxed) {
                    info!("drain_service exiting");
                    break;
                }
            }
        })
    }

    /// Create a UDP microservice that forwards messages the given Tpu.
    /// This service is the network leader
    /// Set `exit` to shutdown its threads.
    pub fn serve<W: Write + Send + 'static>(
        &self,
        me: ReplicatedData,
        requests_socket: UdpSocket,
        _events_socket: UdpSocket,
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
        let request_stage = RequestStage::new(
            self.request_processor.clone(),
            self.accounting_stage.clone(),
            exit.clone(),
            sig_verify_stage.output,
            packet_recycler.clone(),
            blob_recycler.clone(),
        );

        let (broadcast_sender, broadcast_receiver) = channel();
        let t_write = Self::write_service(
            self.accounting_stage.clone(),
            self.request_processor.clone(),
            exit.clone(),
            broadcast_sender,
            blob_recycler.clone(),
            Mutex::new(writer),
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
            request_stage.output,
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
