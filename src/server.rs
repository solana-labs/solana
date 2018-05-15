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
//use tpu::Tpu;

pub struct Server {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Server {
    pub fn new<W: Write + Send + 'static>(
        bank: Bank,
        start_hash: Hash,
        tick_duration: Option<Duration>,
        me: ReplicatedData,
        requests_socket: UdpSocket,
        _events_socket: UdpSocket,
        broadcast_socket: UdpSocket,
        respond_socket: UdpSocket,
        gossip: UdpSocket,
        exit: Arc<AtomicBool>,
        writer: W,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(
            bank.clone(),
            start_hash,
            tick_duration,
            me,
            requests_socket,
            broadcast_socket,
            respond_socket,
            gossip,
            exit.clone(),
            writer,
        );
        thread_hdls.extend(rpu.thread_hdls);

        //let tpu = Tpu::new(
        //    bank.clone(),
        //    start_hash,
        //    tick_duration,
        //    me,
        //    events_socket,
        //    broadcast_socket,
        //    gossip,
        //    exit.clone(),
        //    writer,
        //);
        //thread_hdls.extend(tpu.thread_hdls);

        Server { thread_hdls }
    }
}
