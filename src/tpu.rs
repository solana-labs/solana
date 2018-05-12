//! The `tpu` module implements the Transaction Processing Unit, a
//! 5-stage transaction processing pipeline in software.

use accounting_stage::AccountingStage;
use crdt::{Crdt, ReplicatedData};
use entry_writer::EntryWriter;
use ledger;
use packet;
use result::Result;
use sig_verify_stage::SigVerifyStage;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use streamer;
use thin_client_service::{RequestProcessor, ThinClientService};

pub struct Tpu {
    accounting_stage: Arc<AccountingStage>,
    request_processor: Arc<RequestProcessor>,
}

type SharedTpu = Arc<Tpu>;

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
        obj: &SharedTpu,
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
        let thin_client_service = ThinClientService::new(
            obj.request_processor.clone(),
            obj.accounting_stage.clone(),
            exit.clone(),
            sig_verify_stage.output,
            packet_recycler.clone(),
            blob_recycler.clone(),
        );

        let (broadcast_sender, broadcast_receiver) = channel();
        let t_write = Self::write_service(
            obj.accounting_stage.clone(),
            obj.request_processor.clone(),
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
            thin_client_service.output,
        );

        let mut threads = vec![
            t_receiver,
            t_responder,
            thin_client_service.thread_hdl,
            t_write,
            t_gossip,
            t_listen,
            t_broadcast,
        ];
        threads.extend(sig_verify_stage.thread_hdls.into_iter());
        Ok(threads)
    }

    /// Process verified blobs, already in order
    /// Respond with a signed hash of the state
    fn replicate_state(
        obj: &Tpu,
        verified_receiver: &streamer::BlobReceiver,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = verified_receiver.recv_timeout(timer)?;
        trace!("replicating blobs {}", blobs.len());
        let entries = ledger::reconstruct_entries_from_blobs(&blobs);
        obj.accounting_stage
            .accountant
            .process_verified_entries(entries)?;
        for blob in blobs {
            blob_recycler.recycle(blob);
        }
        Ok(())
    }

    /// This service receives messages from a leader in the network and processes the transactions
    /// on the accountant state.
    /// # Arguments
    /// * `obj` - The accountant state.
    /// * `me` - my configuration
    /// * `leader` - leader configuration
    /// * `exit` - The exit signal.
    /// # Remarks
    /// The pipeline is constructed as follows:
    /// 1. receive blobs from the network, these are out of order
    /// 2. verify blobs, PoH, signatures (TODO)
    /// 3. reconstruct contiguous window
    ///     a. order the blobs
    ///     b. use erasure coding to reconstruct missing blobs
    ///     c. ask the network for missing blobs, if erasure coding is insufficient
    ///     d. make sure that the blobs PoH sequences connect (TODO)
    /// 4. process the transaction state machine
    /// 5. respond with the hash of the state back to the leader
    pub fn replicate(
        obj: &SharedTpu,
        me: ReplicatedData,
        gossip: UdpSocket,
        requests_socket: UdpSocket,
        replicate: UdpSocket,
        leader: ReplicatedData,
        exit: Arc<AtomicBool>,
    ) -> Result<Vec<JoinHandle<()>>> {
        //replicate pipeline
        let crdt = Arc::new(RwLock::new(Crdt::new(me)));
        crdt.write()
            .expect("'crdt' write lock in pub fn replicate")
            .set_leader(leader.id);
        crdt.write()
            .expect("'crdt' write lock before insert() in pub fn replicate")
            .insert(leader);
        let t_gossip = Crdt::gossip(crdt.clone(), exit.clone());
        let t_listen = Crdt::listen(crdt.clone(), gossip, exit.clone());

        // make sure we are on the same interface
        let mut local = replicate.local_addr()?;
        local.set_port(0);
        let write = UdpSocket::bind(local)?;

        let blob_recycler = packet::BlobRecycler::default();
        let (blob_sender, blob_receiver) = channel();
        let t_blob_receiver = streamer::blob_receiver(
            exit.clone(),
            blob_recycler.clone(),
            replicate,
            blob_sender.clone(),
        )?;
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
            blob_recycler.clone(),
            blob_receiver,
            window_sender,
            retransmit_sender,
        );

        let tpu = obj.clone();
        let s_exit = exit.clone();
        let t_replicator = spawn(move || loop {
            let e = Self::replicate_state(&tpu, &window_receiver, &blob_recycler);
            if e.is_err() && s_exit.load(Ordering::Relaxed) {
                break;
            }
        });

        //serve pipeline
        // make sure we are on the same interface
        let mut local = requests_socket.local_addr()?;
        local.set_port(0);
        let respond_socket = UdpSocket::bind(local.clone())?;

        let packet_recycler = packet::PacketRecycler::default();
        let blob_recycler = packet::BlobRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_packet_receiver = streamer::receiver(
            requests_socket,
            exit.clone(),
            packet_recycler.clone(),
            packet_sender,
        )?;

        let sig_verify_stage = SigVerifyStage::new(exit.clone(), packet_receiver);

        let thin_client_service = ThinClientService::new(
            obj.request_processor.clone(),
            obj.accounting_stage.clone(),
            exit.clone(),
            sig_verify_stage.output,
            packet_recycler.clone(),
            blob_recycler.clone(),
        );

        let t_write = Self::drain_service(
            obj.accounting_stage.clone(),
            obj.request_processor.clone(),
            exit.clone(),
        );

        let t_responder = streamer::responder(
            respond_socket,
            exit.clone(),
            blob_recycler.clone(),
            thin_client_service.output,
        );

        let mut threads = vec![
            //replicate threads
            t_blob_receiver,
            t_retransmit,
            t_window,
            t_replicator,
            t_gossip,
            t_listen,
            //serve threads
            t_packet_receiver,
            t_responder,
            thin_client_service.thread_hdl,
            t_write,
        ];
        threads.extend(sig_verify_stage.thread_hdls.into_iter());
        Ok(threads)
    }
}

#[cfg(test)]
pub fn test_node() -> (ReplicatedData, UdpSocket, UdpSocket, UdpSocket, UdpSocket) {
    use signature::{KeyPair, KeyPairUtil};

    let events_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
    let replicate = UdpSocket::bind("127.0.0.1:0").unwrap();
    let requests_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let pubkey = KeyPair::new().pubkey();
    let d = ReplicatedData::new(
        pubkey,
        gossip.local_addr().unwrap(),
        replicate.local_addr().unwrap(),
        requests_socket.local_addr().unwrap(),
    );
    (d, gossip, replicate, requests_socket, events_socket)
}

#[cfg(test)]
mod tests {
    use accountant::Accountant;
    use accounting_stage::AccountingStage;
    use bincode::serialize;
    use chrono::prelude::*;
    use crdt::Crdt;
    use entry;
    use event::Event;
    use hash::{hash, Hash};
    use logger;
    use mint::Mint;
    use packet::BlobRecycler;
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer;
    use tpu::{test_node, Tpu};
    use transaction::Transaction;

    /// Test that mesasge sent from leader to target1 and repliated to target2
    #[test]
    #[ignore]
    fn test_replicate() {
        logger::setup();
        let (leader_data, leader_gossip, _, leader_serve, _) = test_node();
        let (target1_data, target1_gossip, target1_replicate, target1_serve, _) = test_node();
        let (target2_data, target2_gossip, target2_replicate, _, _) = test_node();
        let exit = Arc::new(AtomicBool::new(false));

        //start crdt_leader
        let mut crdt_l = Crdt::new(leader_data.clone());
        crdt_l.set_leader(leader_data.id);

        let cref_l = Arc::new(RwLock::new(crdt_l));
        let t_l_gossip = Crdt::gossip(cref_l.clone(), exit.clone());
        let t_l_listen = Crdt::listen(cref_l, leader_gossip, exit.clone());

        //start crdt2
        let mut crdt2 = Crdt::new(target2_data.clone());
        crdt2.insert(leader_data.clone());
        crdt2.set_leader(leader_data.id);
        let leader_id = leader_data.id;
        let cref2 = Arc::new(RwLock::new(crdt2));
        let t2_gossip = Crdt::gossip(cref2.clone(), exit.clone());
        let t2_listen = Crdt::listen(cref2, target2_gossip, exit.clone());

        // setup some blob services to send blobs into the socket
        // to simulate the source peer and get blobs out of the socket to
        // simulate target peer
        let recv_recycler = BlobRecycler::default();
        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver = streamer::blob_receiver(
            exit.clone(),
            recv_recycler.clone(),
            target2_replicate,
            s_reader,
        ).unwrap();

        // simulate leader sending messages
        let (s_responder, r_responder) = channel();
        let t_responder = streamer::responder(
            leader_serve,
            exit.clone(),
            resp_recycler.clone(),
            r_responder,
        );

        let starting_balance = 10_000;
        let alice = Mint::new(starting_balance);
        let accountant = Accountant::new(&alice);
        let accounting_stage = AccountingStage::new(accountant, &alice.last_id(), Some(30));
        let tpu = Arc::new(Tpu::new(accounting_stage));
        let replicate_addr = target1_data.replicate_addr;
        let threads = Tpu::replicate(
            &tpu,
            target1_data,
            target1_gossip,
            target1_serve,
            target1_replicate,
            leader_data,
            exit.clone(),
        ).unwrap();

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

            let accountant = &tpu.accounting_stage.accountant;

            let tr0 = Event::new_timestamp(&bob_keypair, Utc::now());
            let entry0 = entry::create_entry(&cur_hash, i, vec![tr0]);
            accountant.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);

            let tr1 = Transaction::new(
                &alice.keypair(),
                bob_keypair.pubkey(),
                transfer_amount,
                cur_hash,
            );
            accountant.register_entry_id(&cur_hash);
            cur_hash = hash(&cur_hash);
            let entry1 =
                entry::create_entry(&cur_hash, i + num_blobs, vec![Event::Transaction(tr1)]);
            accountant.register_entry_id(&cur_hash);
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

        let accountant = &tpu.accounting_stage.accountant;
        let alice_balance = accountant.get_balance(&alice.keypair().pubkey()).unwrap();
        assert_eq!(alice_balance, alice_ref_balance);

        let bob_balance = accountant.get_balance(&bob_keypair.pubkey()).unwrap();
        assert_eq!(bob_balance, starting_balance - alice_ref_balance);

        exit.store(true, Ordering::Relaxed);
        for t in threads {
            t.join().expect("join");
        }
        t2_gossip.join().expect("join");
        t2_listen.join().expect("join");
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_l_gossip.join().expect("join");
        t_l_listen.join().expect("join");
    }

}
