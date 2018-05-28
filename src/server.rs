//! The `server` module hosts all the server microservices.

use bank::Bank;
use crdt::{Crdt, ReplicatedData};
use data_replicator::DataReplicator;
use hash::Hash;
use packet;
use rpu::Rpu;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use streamer;
use tpu::Tpu;
use tvu::Tvu;

pub struct Server {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Server {
    pub fn new_leader<W: Write + Send + 'static>(
        bank: Bank,
        start_hash: Hash,
        tick_duration: Option<Duration>,
        me: ReplicatedData,
        requests_socket: UdpSocket,
        transactions_socket: UdpSocket,
        broadcast_socket: UdpSocket,
        respond_socket: UdpSocket,
        gossip_socket: UdpSocket,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(bank.clone(), requests_socket, respond_socket, exit.clone());
        thread_hdls.extend(rpu.thread_hdls);

        let blob_recycler = packet::BlobRecycler::default();
        let tpu = Tpu::new(
            bank.clone(),
            start_hash,
            tick_duration,
            transactions_socket,
            blob_recycler.clone(),
            exit.clone(),
            writer,
        );
        thread_hdls.extend(tpu.thread_hdls);

        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        let window = streamer::default_window();
        let gossip_send_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let data_replicator = DataReplicator::new(
            crdt.clone(),
            window.clone(),
            gossip_socket,
            gossip_send_socket,
            exit.clone(),
        ).expect("DataReplicator::new");
        thread_hdls.extend(data_replicator.thread_hdls);

        let t_broadcast = streamer::broadcaster(
            broadcast_socket,
            exit.clone(),
            crdt,
            window,
            blob_recycler.clone(),
            tpu.blob_receiver,
        );
        thread_hdls.extend(vec![t_broadcast]);

        Server { thread_hdls }
    }
    pub fn new_validator(
        bank: Bank,
        me: ReplicatedData,
        requests_socket: UdpSocket,
        respond_socket: UdpSocket,
        replicate_socket: UdpSocket,
        gossip_socket: UdpSocket,
        leader_repl_data: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(bank.clone(), requests_socket, respond_socket, exit.clone());
        thread_hdls.extend(rpu.thread_hdls);
        let tvu = Tvu::new(
            bank.clone(),
            me,
            gossip_socket,
            replicate_socket,
            leader_repl_data,
            exit.clone(),
        );
        thread_hdls.extend(tvu.thread_hdls);
        Server { thread_hdls }
    }
}
