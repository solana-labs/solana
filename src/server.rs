//! The `server` module hosts all the server microservices.

use bank::Bank;
use crdt::{Crdt, ReplicatedData};
use ncp::Ncp;
use packet::BlobRecycler;
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
    /// Create a server instance acting as a leader.
    ///
    /// ```text
    ///              .---------------------.
    ///              |  Leader             |
    ///              |                     |
    ///  .--------.  |  .-----.            |
    ///  |        |---->|     |            |
    ///  | Client |  |  | RPU |            |
    ///  |        |<----|     |            |
    ///  `----+---`  |  `-----`            |
    ///       |      |     ^               |
    ///       |      |     |               |
    ///       |      |  .--+---.           |
    ///       |      |  | Bank |           |
    ///       |      |  `------`           |
    ///       |      |     ^               |
    ///       |      |     |               |    .------------.
    ///       |      |  .--+--.   .-----.  |    |            |
    ///       `-------->| TPU +-->| NCP +------>| Validators |
    ///              |  `-----`   `-----`  |    |            |
    ///              |                     |    `------------`
    ///              `---------------------`
    /// ```
    pub fn new_leader<W: Write + Send + 'static>(
        bank: Bank,
        entry_height: u64,
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

        let blob_recycler = BlobRecycler::default();
        let (tpu, blob_receiver) = Tpu::new(
            bank.clone(),
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
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            gossip_socket,
            gossip_send_socket,
            exit.clone(),
        ).expect("Ncp::new");
        thread_hdls.extend(ncp.thread_hdls);

        let t_broadcast = streamer::broadcaster(
            broadcast_socket,
            exit.clone(),
            crdt,
            window,
            entry_height,
            blob_recycler.clone(),
            blob_receiver,
        );
        thread_hdls.extend(vec![t_broadcast]);

        Server { thread_hdls }
    }

    /// Create a server instance acting as a validator.
    ///
    /// ```text
    ///               .-------------------------------.
    ///               | Validator                     |
    ///               |                               |
    ///   .--------.  |            .-----.            |
    ///   |        |-------------->|     |            |
    ///   | Client |  |            | RPU |            |
    ///   |        |<--------------|     |            |
    ///   `--------`  |            `-----`            |
    ///               |               ^               |
    ///               |               |               |
    ///               |            .--+---.           |
    ///               |            | Bank |           |
    ///               |            `------`           |
    ///               |               ^               |
    ///   .--------.  |               |               |    .------------.
    ///   |        |  |            .--+--.            |    |            |
    ///   | Leader |<------------->| TVU +<--------------->|            |
    ///   |        |  |            `-----`            |    | Validators |
    ///   |        |  |               ^               |    |            |
    ///   |        |  |               |               |    |            |
    ///   |        |  |            .--+--.            |    |            |
    ///   |        |<------------->| NCP +<--------------->|            |
    ///   |        |  |            `-----`            |    |            |
    ///   `--------`  |                               |    `------------`
    ///               `-------------------------------`
    /// ```
    pub fn new_validator(
        bank: Bank,
        entry_height: u64,
        me: ReplicatedData,
        requests_socket: UdpSocket,
        respond_socket: UdpSocket,
        replicate_socket: UdpSocket,
        gossip_listen_socket: UdpSocket,
        repair_socket: UdpSocket,
        entry_point: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let bank = Arc::new(bank);
        let mut thread_hdls = vec![];
        let rpu = Rpu::new(bank.clone(), requests_socket, respond_socket, exit.clone());
        thread_hdls.extend(rpu.thread_hdls);

        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        crdt.write()
            .expect("'crdt' write lock before insert() in pub fn replicate")
            .insert(&entry_point);
        let window = streamer::default_window();
        let gossip_send_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let retransmit_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            gossip_listen_socket,
            gossip_send_socket,
            exit.clone(),
        ).expect("Ncp::new");

        let tvu = Tvu::new(
            bank.clone(),
            entry_height,
            crdt.clone(),
            window.clone(),
            replicate_socket,
            repair_socket,
            retransmit_socket,
            exit.clone(),
        );
        thread_hdls.extend(tvu.thread_hdls);
        thread_hdls.extend(ncp.thread_hdls);
        Server { thread_hdls }
    }
}
#[cfg(test)]
mod tests {
    use bank::Bank;
    use crdt::TestNode;
    use mint::Mint;
    use server::Server;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    #[test]
    fn validator_exit() {
        let tn = TestNode::new();
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let exit = Arc::new(AtomicBool::new(false));
        let v = Server::new_validator(
            bank,
            0,
            tn.data.clone(),
            tn.sockets.requests,
            tn.sockets.respond,
            tn.sockets.replicate,
            tn.sockets.gossip,
            tn.sockets.repair,
            tn.data,
            exit.clone(),
        );
        exit.store(true, Ordering::Relaxed);
        for t in v.thread_hdls {
            t.join().unwrap();
        }
    }
}
