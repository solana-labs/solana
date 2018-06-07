//! The `tvu` module implements the Transaction Validation Unit, a
//! 5-stage transaction validation pipeline in software.
//! 1. streamer
//! - Incoming blobs are picked up from the replicate socket.
//! 2. verifier
//! - TODO Blobs are sent to the GPU, and while the memory is there the PoH stream is verified
//! along with the ecdsa signature for the blob and each signature in all the transactions.  Blobs
//! with errors are dropped, or marked for slashing.
//! 3.a retransmit
//! - Blobs originating from the parent (leader, at the moment, is the only parent), are retransmit to all the
//! peers in the crdt.  Peers is everyone who is not me or the leader that has a known replicate
//! address.
//! 3.b window
//! - Verified blobs are placed into a window, indexed by the counter set by the leader.sockets. This could
//! be the PoH counter if its monotonically increasing in each blob.  Erasure coding is used to
//! recover any missing packets, and requests are made at random to peers and parents to retransmit
//! a missing packet.
//! 4. accountant
//! - Contigous blobs are sent to the accountant for processing transactions
//! 5. validator
//! - TODO Validation messages are sent back to the leader

use bank::Bank;
use crdt::{Crdt, ReplicatedData};
use ncp::Ncp;
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
    /// * `gossip` - my gossisp socket
    /// * `replicate` - my replicate socket
    /// * `leader` - leader configuration
    /// * `exit` - The exit signal.
    pub fn new(
        bank: Arc<Bank>,
        me: ReplicatedData,
        gossip_listen_socket: UdpSocket,
        replicate: UdpSocket,
        repair_socket: UdpSocket,
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
        let window = streamer::default_window();
        let gossip_send_socket = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        let ncp = Ncp::new(
            crdt.clone(),
            window.clone(),
            gossip_listen_socket,
            gossip_send_socket,
            exit.clone(),
        ).expect("Ncp::new");

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
        let t_repair_receiver = streamer::blob_receiver(
            exit.clone(),
            blob_recycler.clone(),
            repair_socket,
            blob_sender.clone(),
        ).expect("tvu: blob repair receiver fail");

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

        let mut threads = vec![
            //replicate threads
            t_blob_receiver,
            t_retransmit,
            t_window,
            t_repair_receiver,
            replicate_stage.thread_hdl,
        ];
        threads.extend(ncp.thread_hdls.into_iter());
        Tvu {
            thread_hdls: threads,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use bank::Bank;
    use bincode::serialize;
    use crdt::{Crdt, TestNode};
    use entry::Entry;
    use hash::{hash, Hash};
    use logger;
    use mint::Mint;
    use ncp::Ncp;
    use packet::BlobRecycler;
    use result::Result;
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::VecDeque;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer;
    use transaction::Transaction;
    use tvu::Tvu;

    fn new_replicator(
        crdt: Arc<RwLock<Crdt>>,
        listen: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Result<Ncp> {
        let window = streamer::default_window();
        let send_sock = UdpSocket::bind("0.0.0.0:0").expect("bind 0");
        Ncp::new(crdt, window, listen, send_sock, exit)
    }
    /// Test that message sent from leader to target1 and replicated to target2
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
        let dr_l = new_replicator(cref_l, leader.sockets.gossip, exit.clone()).unwrap();

        //start crdt2
        let mut crdt2 = Crdt::new(target2.data.clone());
        crdt2.insert(&leader.data);
        crdt2.set_leader(leader.data.id);
        let leader_id = leader.data.id;
        let cref2 = Arc::new(RwLock::new(crdt2));
        let dr_2 = new_replicator(cref2, target2.sockets.gossip, exit.clone()).unwrap();

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
            target1.sockets.repair,
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

            let entry0 = Entry::new(&cur_hash, i, vec![]);
            bank.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);

            let tx0 = Transaction::new(
                &mint.keypair(),
                bob_keypair.pubkey(),
                transfer_amount,
                cur_hash,
            );
            bank.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);
            let entry1 = Entry::new(&cur_hash, i + num_blobs, vec![tx0]);
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
        for t in dr_l.thread_hdls {
            t.join().expect("join");
        }
        for t in dr_2.thread_hdls {
            t.join().expect("join");
        }
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }
}
