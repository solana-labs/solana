use crate::serve_repair::ServeRepair;
use solana_ledger::blockstore::Blockstore;
use solana_perf::recycler::Recycler;
use solana_streamer::streamer;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

pub struct ServeRepairService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ServeRepairService {
    pub fn new(
        serve_repair: &Arc<RwLock<ServeRepair>>,
        blockstore: Option<Arc<Blockstore>>,
        serve_repair_socket: UdpSocket,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let (request_sender, request_receiver) = channel();
        let serve_repair_socket = Arc::new(serve_repair_socket);
        trace!(
            "ServeRepairService: id: {}, listening on: {:?}",
            &serve_repair.read().unwrap().my_info().id,
            serve_repair_socket.local_addr().unwrap()
        );
        let t_receiver = streamer::receiver(
            serve_repair_socket.clone(),
            &exit,
            request_sender,
            Recycler::default(),
            "serve_repair_receiver",
        );
        let (response_sender, response_receiver) = channel();
        let t_responder =
            streamer::responder("serve-repairs", serve_repair_socket, response_receiver);
        let t_listen = ServeRepair::listen(
            serve_repair.clone(),
            blockstore,
            request_receiver,
            response_sender,
            exit,
        );

        let thread_hdls = vec![t_receiver, t_responder, t_listen];
        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
