//! The `tvu` module implements the Transaction Validation Unit, a
//! 5-stage transaction validation pipeline in software.
//! 1. streamer
//! - Incoming blobs are picked up from the replicate socket.
//! 2. verifier
//! - TODO Blobs are sent to the GPU, and while the memory is there the PoH stream is verified
//! along with the ecdsa signature for the blob and each signature in all the transactions.  Blobs
//! with errors are dropped, or marked for slashing.
//! 3.a retransmit
//! - Blobs originating from the parent (leader atm is the only parent), are retransmit to all the
//! peers in the crdt.  Peers is everyone who is not me or the leader that has a known replicate
//! address.
//! 3.b window
//! - Verified blobs are placed into a window, indexed by the counter set by the leader.sockets. This could
//! be the PoH counter if its monitonically increasing in each blob.  Easure coding is used to
//! recover any missing packets, and requests are made at random to peers and parents to retransmit
//! a missing packet.
//! 4. accountant
//! - Contigous blobs are sent to the accountant for processing transactions
//! 5. validator
//! - TODO Validation messages are sent back to the leader

use bank::Bank;
use crdt::{Crdt, ReplicatedData};
use packet;
use replicate_stage::ReplicateStage;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use streamer;

pub struct Tvu {
    pub thread_hdls: Vec<JoinHandle<()>>,
}

impl Tvu {
    /// This service receives messages from a leader in the network and processes the transactions
    /// on the bank state.
    /// # Arguments
    /// * `bank` - The bank state.
    /// * `me` - my configuration
    /// * `gossip` - my gosisp socket
    /// * `replicte` - my replicte socket
    /// * `leader` - leader configuration
    /// * `exit` - The exit signal.
    pub fn new(
        bank: Arc<Bank>,
        me: ReplicatedData,
        gossip: UdpSocket,
        replicate: UdpSocket,
        leader: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Self {
        //replicate pipeline
        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        crdt.write()
            .expect("'crdt' write lock in pub fn replicate")
            .set_leader(leader.id);
        crdt.write()
            .expect("'crdt' write lock before insert() in pub fn replicate")
            .insert(&leader);
        let t_gossip = Crdt::gossip(crdt.clone(), exit.clone());
        let window = streamer::default_window();
        let t_listen = Crdt::listen(crdt.clone(), window.clone(), gossip, exit.clone());

        // TODO pull this socket out through the public interface
        // make sure we are on the same interface
        let mut local = replicate.local_addr().expect("tvu: get local address");
        local.set_port(0);
        let write = UdpSocket::bind(local).expect("tvu: bind to local socket");

        let blob_recycler = packet::BlobRecycler::default();
        let (blob_sender, blob_receiver) = channel();
        let t_blob_receiver = streamer::blob_receiver(
            exit.clone(),
            blob_recycler.clone(),
            replicate,
            blob_sender.clone(),
        ).expect("tvu: blob receiver creation");
        let (window_sender, window_receiver) = channel();
        let (retransmit_sender, retransmit_receiver) = channel();

        let t_retransmit = streamer::retransmitter(
            write,
            exit.clone(),
            crdt.clone(),
            blob_recycler.clone(),
            retransmit_receiver,
        );

        //TODO
        //the packets coming out of blob_receiver need to be sent to the GPU and verified
        //then sent to the window, which does the erasure coding reconstruction
        let t_window = streamer::window(
            exit.clone(),
            crdt.clone(),
            window,
            blob_recycler.clone(),
            blob_receiver,
            window_sender,
            retransmit_sender,
        );

        let replicate_stage = ReplicateStage::new(
            bank.clone(),
            exit.clone(),
            window_receiver,
            blob_recycler.clone(),
        );

        let threads = vec![
            //replicate threads
            t_blob_receiver,
            t_retransmit,
            t_window,
            replicate_stage.thread_hdl,
            t_gossip,
            t_listen,
        ];
        Tvu {
            thread_hdls: threads,
        }
    }
}

#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
pub fn test_node() -> (ReplicatedData, UdpSocket, UdpSocket, UdpSocket, UdpSocket) {
    use signature::{KeyPair, KeyPairUtil};

    let events_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
    let replicate = UdpSocket::bind("127.0.0.1:0").unwrap();
    let requests_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();
    let pubkey = KeyPair::new().pubkey();
    let d = ReplicatedData::new(
        pubkey,
        gossip.local_addr().unwrap(),
        replicate.local_addr().unwrap(),
        requests_socket.local_addr().unwrap(),
        events_socket.local_addr().unwrap(),
    );
    (d, gossip, replicate, requests_socket, events_socket)
}

#[cfg(test)]
pub mod tests {
    use bank::Bank;
    use bincode::serialize;
    use chrono::prelude::*;
    use crdt::Crdt;
    use crdt::ReplicatedData;
    use entry::Entry;
    use event::Event;
    use hash::{hash, Hash};
    use logger;
    use mint::Mint;
    use packet::BlobRecycler;
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::VecDeque;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer;
    use tvu::Tvu;

    /// Test that mesasge sent from leader to target1 and repliated to target2
    #[test]
    fn test_replicate() {
        logger::setup();
        let leader = TestNode::new();
        let target1 = TestNode::new();
        let target2 = TestNode::new();
        let exit = Arc::new(AtomicBool::new(false));

        //start crdt_leader
        let mut crdt_l = Crdt::new(leader.data.clone());
        crdt_l.set_leader(leader.data.id);

        let cref_l = Arc::new(RwLock::new(crdt_l));
        let t_l_gossip = Crdt::gossip(cref_l.clone(), exit.clone());
        let window1 = streamer::default_window();
        let t_l_listen = Crdt::listen(cref_l, window1, leader.sockets.gossip, exit.clone());

        //start crdt2
        let mut crdt2 = Crdt::new(target2.data.clone());
        crdt2.insert(&leader.data);
        crdt2.set_leader(leader.data.id);
        let leader_id = leader.data.id;
        let cref2 = Arc::new(RwLock::new(crdt2));
        let t2_gossip = Crdt::gossip(cref2.clone(), exit.clone());
        let window2 = streamer::default_window();
        let t2_listen = Crdt::listen(cref2, window2, target2.sockets.gossip, exit.clone());

        // setup some blob services to send blobs into the socket
        // to simulate the source peer and get blobs out of the socket to
        // simulate target peer
        let recv_recycler = BlobRecycler::default();
        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver = streamer::blob_receiver(
            exit.clone(),
            recv_recycler.clone(),
            target2.sockets.replicate,
            s_reader,
        ).unwrap();

        // simulate leader sending messages
        let (s_responder, r_responder) = channel();
        let t_responder = streamer::responder(
            leader.sockets.requests,
            exit.clone(),
            resp_recycler.clone(),
            r_responder,
        );

        let starting_balance = 10_000;
        let mint = Mint::new(starting_balance);
        let replicate_addr = target1.data.replicate_addr;
        let bank = Arc::new(Bank::new(&mint));
        let tvu = Tvu::new(
            bank.clone(),
            target1.data,
            target1.sockets.gossip,
            target1.sockets.replicate,
            leader.data,
            exit.clone(),
        );

        let mut alice_ref_balance = starting_balance;
        let mut msgs = VecDeque::new();
        let mut cur_hash = Hash::default();
        let num_blobs = 10;
        let transfer_amount = 501;
        let bob_keypair = KeyPair::new();
        for i in 0..num_blobs {
            let b = resp_recycler.allocate();
            let b_ = b.clone();
            let mut w = b.write().unwrap();
            w.set_index(i).unwrap();
            w.set_id(leader_id).unwrap();

            let tr0 = Event::new_timestamp(&bob_keypair, Utc::now());
            let entry0 = Entry::new(&cur_hash, i, vec![tr0]);
            bank.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);

            let tr1 = Event::new_transaction(
                &mint.keypair(),
                bob_keypair.pubkey(),
                transfer_amount,
                cur_hash,
            );
            bank.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);
            let entry1 = Entry::new(&cur_hash, i + num_blobs, vec![tr1]);
            bank.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);

            alice_ref_balance -= transfer_amount;

            let serialized_entry = serialize(&vec![entry0, entry1]).unwrap();

            w.data_mut()[..serialized_entry.len()].copy_from_slice(&serialized_entry);
            w.set_size(serialized_entry.len());
            w.meta.set_addr(&replicate_addr);
            drop(w);
            msgs.push_back(b_);
        }

        // send the blobs into the socket
        s_responder.send(msgs).expect("send");

        // receive retransmitted messages
        let timer = Duration::new(1, 0);
        let mut msgs: Vec<_> = Vec::new();
        while let Ok(msg) = r_reader.recv_timeout(timer) {
            trace!("msg: {:?}", msg);
            msgs.push(msg);
        }

        let alice_balance = bank.get_balance(&mint.keypair().pubkey()).unwrap();
        assert_eq!(alice_balance, alice_ref_balance);

        let bob_balance = bank.get_balance(&bob_keypair.pubkey()).unwrap();
        assert_eq!(bob_balance, starting_balance - alice_ref_balance);

        exit.store(true, Ordering::Relaxed);
        for t in tvu.thread_hdls {
            t.join().expect("join");
        }
        t2_gossip.join().expect("join");
        t2_listen.join().expect("join");
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_l_gossip.join().expect("join");
        t_l_listen.join().expect("join");
    }
    pub struct Sockets {
        pub gossip: UdpSocket,
        pub requests: UdpSocket,
        pub replicate: UdpSocket,
        pub event: UdpSocket,
        pub respond: UdpSocket,
        pub broadcast: UdpSocket,
    }
    pub struct TestNode {
        pub data: ReplicatedData,
        pub sockets: Sockets,
    }
    impl TestNode {
        pub fn new() -> TestNode {
            let gossip = UdpSocket::bind("0.0.0.0:0").unwrap();
            let requests = UdpSocket::bind("0.0.0.0:0").unwrap();
            let event = UdpSocket::bind("0.0.0.0:0").unwrap();
            let replicate = UdpSocket::bind("0.0.0.0:0").unwrap();
            let respond = UdpSocket::bind("0.0.0.0:0").unwrap();
            let broadcast = UdpSocket::bind("0.0.0.0:0").unwrap();
            let pubkey = KeyPair::new().pubkey();
            let data = ReplicatedData::new(
                pubkey,
                gossip.local_addr().unwrap(),
                replicate.local_addr().unwrap(),
                requests.local_addr().unwrap(),
                event.local_addr().unwrap(),
            );
            TestNode {
                data: data,
                sockets: Sockets {
                    gossip,
                    requests,
                    replicate,
                    event,
                    respond,
                    broadcast,
                },
            }
        }
    }
}
