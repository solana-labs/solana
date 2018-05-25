//! The `server` module hosts all the server microservices.

use bank::Bank;
use crdt::ReplicatedData;
use hash::Hash;
use rpu::Rpu;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::JoinHandle;
use std::time::Duration;
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
        let tpu = Tpu::new(
            bank.clone(),
            start_hash,
            tick_duration,
            me,
            transactions_socket,
            broadcast_socket,
            gossip_socket,
            exit.clone(),
            writer,
        );
        thread_hdls.extend(tpu.thread_hdls);
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
