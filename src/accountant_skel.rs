//! The `accountant_skel` module is a microservice that exposes the high-level
//! Accountant API to the network. Its message encoding is currently
//! in flux. Clients should use AccountantStub to interact with it.

use accountant::Accountant;
use bincode::{deserialize, serialize};
use ecdsa;
use entry::Entry;
use event::Event;
use hash::Hash;
use historian::Historian;
use packet;
use packet::SharedPackets;
use rayon::prelude::*;
use recorder::Signal;
use result::Result;
use serde_json;
use signature::PublicKey;
use std::cmp::max;
use std::collections::VecDeque;
use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use streamer;
use transaction::Transaction;

pub struct AccountantSkel<W: Write + Send + 'static> {
    acc: Accountant,
    last_id: Hash,
    writer: W,
    historian: Historian,
}

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    Transaction(Transaction),
    GetBalance { key: PublicKey },
    GetLastId,
}

impl Request {
    /// Verify the request is valid.
    pub fn verify(&self) -> bool {
        match *self {
            Request::Transaction(ref tr) => tr.verify_plan(),
            _ => true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Balance { key: PublicKey, val: Option<i64> },
    Entries { entries: Vec<Entry> },
    LastId { id: Hash },
}

impl<W: Write + Send + 'static> AccountantSkel<W> {
    /// Create a new AccountantSkel that wraps the given Accountant.
    pub fn new(acc: Accountant, last_id: Hash, writer: W, historian: Historian) -> Self {
        AccountantSkel {
            acc,
            last_id,
            writer,
            historian,
        }
    }

    /// Process any Entry items that have been published by the Historian.
    pub fn sync(&mut self) -> Hash {
        while let Ok(entry) = self.historian.receiver.try_recv() {
            self.last_id = entry.id;
            self.acc.register_entry_id(&self.last_id);
            writeln!(self.writer, "{}", serde_json::to_string(&entry).unwrap()).unwrap();
        }
        self.last_id
    }

    /// Process Request items sent by clients.
    pub fn process_request(
        &mut self,
        msg: Request,
        rsp_addr: SocketAddr,
    ) -> Option<(Response, SocketAddr)> {
        match msg {
            Request::GetBalance { key } => {
                let val = self.acc.get_balance(&key);
                Some((Response::Balance { key, val }, rsp_addr))
            }
            Request::GetLastId => Some((Response::LastId { id: self.sync() }, rsp_addr)),
            Request::Transaction(_) => unreachable!(),
        }
    }

    fn recv_batch(recvr: &streamer::PacketReceiver) -> Result<Vec<SharedPackets>> {
        let timer = Duration::new(1, 0);
        let msgs = recvr.recv_timeout(timer)?;
        trace!("got msgs");
        let mut batch = vec![msgs];
        while let Ok(more) = recvr.try_recv() {
            trace!("got more msgs");
            batch.push(more);
        }
        info!("batch len {}", batch.len());
        Ok(batch)
    }

    fn verify_batch(batch: Vec<SharedPackets>) -> Vec<Vec<(SharedPackets, Vec<u8>)>> {
        let chunk_size = max(1, (batch.len() + 3) / 4);
        let batches: Vec<_> = batch.chunks(chunk_size).map(|x| x.to_vec()).collect();
        batches
            .into_par_iter()
            .map(|batch| {
                let r = ecdsa::ed25519_verify(&batch);
                batch.into_iter().zip(r).collect()
            })
            .collect()
    }

    fn verifier(
        recvr: &streamer::PacketReceiver,
        sendr: &Sender<Vec<(SharedPackets, Vec<u8>)>>,
    ) -> Result<()> {
        let batch = Self::recv_batch(recvr)?;
        let verified_batches = Self::verify_batch(batch);
        for xs in verified_batches {
            sendr.send(xs)?;
        }
        Ok(())
    }

    pub fn deserialize_packets(p: &packet::Packets) -> Vec<Option<(Request, SocketAddr)>> {
        p.packets
            .par_iter()
            .map(|x| {
                deserialize(&x.data[0..x.meta.size])
                    .map(|req| (req, x.meta.addr()))
                    .ok()
            })
            .collect()
    }

    /// Split Request list into verified transactions and the rest
    fn partition_requests(
        req_vers: Vec<(Request, SocketAddr, u8)>,
    ) -> (Vec<Transaction>, Vec<(Request, SocketAddr)>) {
        let mut trs = vec![];
        let mut reqs = vec![];
        for (msg, rsp_addr, verify) in req_vers {
            match msg {
                Request::Transaction(tr) => {
                    if verify != 0 {
                        trs.push(tr);
                    }
                }
                _ => reqs.push((msg, rsp_addr)),
            }
        }
        (trs, reqs)
    }

    fn process_packets(
        &mut self,
        req_vers: Vec<(Request, SocketAddr, u8)>,
    ) -> Result<Vec<(Response, SocketAddr)>> {
        let (trs, reqs) = Self::partition_requests(req_vers);

        // Process the transactions in parallel and then log the successful ones.
        for result in self.acc.process_verified_transactions(trs) {
            if let Ok(tr) = result {
                self.historian
                    .sender
                    .send(Signal::Event(Event::Transaction(tr)))?;
            }
        }

        // Let validators know they should not attempt to process additional
        // transactions in parallel.
        self.historian.sender.send(Signal::Tick)?;

        // Process the remaining requests serially.
        let rsps = reqs.into_iter()
            .filter_map(|(req, rsp_addr)| self.process_request(req, rsp_addr))
            .collect();

        Ok(rsps)
    }

    fn serialize_response(
        resp: Response,
        rsp_addr: SocketAddr,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<packet::SharedBlob> {
        let blob = blob_recycler.allocate();
        {
            let mut b = blob.write().unwrap();
            let v = serialize(&resp)?;
            let len = v.len();
            b.data[..len].copy_from_slice(&v);
            b.meta.size = len;
            b.meta.set_addr(&rsp_addr);
        }
        Ok(blob)
    }

    fn serialize_responses(
        rsps: Vec<(Response, SocketAddr)>,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<VecDeque<packet::SharedBlob>> {
        let mut blobs = VecDeque::new();
        for (resp, rsp_addr) in rsps {
            blobs.push_back(Self::serialize_response(resp, rsp_addr, blob_recycler)?);
        }
        Ok(blobs)
    }

    fn process(
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        verified_receiver: &Receiver<Vec<(SharedPackets, Vec<u8>)>>,
        blob_sender: &streamer::BlobSender,
        packet_recycler: &packet::PacketRecycler,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let mms = verified_receiver.recv_timeout(timer)?;
        for (msgs, vers) in mms {
            let reqs = Self::deserialize_packets(&msgs.read().unwrap());
            let req_vers = reqs.into_iter()
                .zip(vers)
                .filter_map(|(req, ver)| req.map(|(msg, addr)| (msg, addr, ver)))
                .filter(|x| x.0.verify())
                .collect();
            let rsps = obj.lock().unwrap().process_packets(req_vers)?;
            let blobs = Self::serialize_responses(rsps, blob_recycler)?;
            if !blobs.is_empty() {
                //don't wake up the other side if there is nothing
                blob_sender.send(blobs)?;
            }
            packet_recycler.recycle(msgs);
        }
        Ok(())
    }
    /// Process verified blobs, already in order
    /// Respond with a signed hash of the state
    fn replicate_state(
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        verified_receiver: &BlobReceiver,
        blob_sender: &streamer::BlobSender,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let blobs = verified_receiver.recv_timeout(timer)?;
        for msgs in blobs {
            let entries:Vec<Entry> = b.read().unwrap().data.deserialize()?;
            for e in entries {
                obj.lock().unwrap().acc.process_verified_events(e.events)?;
            }
            //TODO respond back to leader with hash of the state
        }
        blob_recycler.recycle(msgs);
        Ok(())
    }


    /// Create a UDP microservice that forwards messages the given AccountantSkel.
    /// This service is the network leader
    /// Set `exit` to shutdown its threads.
    pub fn serve(
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        addr: &str,
        exit: Arc<AtomicBool>,
    ) -> Result<Vec<JoinHandle<()>>> {
        let read = UdpSocket::bind(addr)?;
        // make sure we are on the same interface
        let mut local = read.local_addr()?;
        local.set_port(0);
        let write = UdpSocket::bind(local)?;

        let packet_recycler = packet::PacketRecycler::default();
        let blob_recycler = packet::BlobRecycler::default();
        let (packet_sender, packet_receiver) = channel();
        let t_receiver =
            streamer::receiver(read, exit.clone(), packet_recycler.clone(), packet_sender)?;
        let (blob_sender, blob_receiver) = channel();
        let t_responder =
            streamer::responder(write, exit.clone(), blob_recycler.clone(), blob_receiver);
        let (verified_sender, verified_receiver) = channel();

        let exit_ = exit.clone();
        let t_verifier = spawn(move || loop {
            let e = Self::blob_verifier(&blob_receiver, &verified_sender);
            if e.is_err() && exit_.load(Ordering::Relaxed) {
                break;
            }
        });

        let skel = obj.clone();
        let t_server = spawn(move || loop {
            let e = Self::process(
                &skel,
                &verified_receiver,
                &blob_sender,
                &packet_recycler,
                &blob_recycler,
            );
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        });
        Ok(vec![t_receiver, t_responder, t_server, t_verifier])
    }

    /// This service receives messages from a leader in the network and processes the transactions
    /// on the accountant state.
    /// # Arguments
    /// * `obj` - The accountant state.
    /// * `rsubs` - The subscribers.
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
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        rsubs: Subscribers,
        exit: Arc<AtomicBool>,
    ) -> Result<Vec<JoinHandle<()>>> {
        let read = UdpSocket::bind(rsubs.me.addr)?;
        // make sure we are on the same interface
        let mut local = read.local_addr()?;
        local.set_port(0);
        let write = UdpSocket::bind(local)?;

        let blob_recycler = packet::BlobRecycler::default();
        let (blob_sender, blob_receiver) = channel();
        let t_blob_receiver =
            streamer::blob_receiver(exit.clone(), blob_recycler.clone(), read, blob_sender)?;
        let (window_sender, window_receiver) = channel();
        let (retransmit_sender, retransmit_receiver) = channel();

        let subs = Arc::new(RwLock::new(rsubs));
        let t_retransmit = retransmitter(
            write,
            exit.clone(),
            subs,
            blob_recycler.clone(),
            retransmit_receiver,
        );
        //TODO
        //the packets comming out of blob_receiver need to be sent to the GPU and verified
        //then sent to the window, which does the erasure coding reconstruction
        let t_window = streamer::window(
            exit.clone(),
            subs,
            blob_recycler.clone(),
            blob_receiver,
            window_sender,
            retransmit_sender,
        );

        let skel = obj.clone();
        let t_server = spawn(move || loop {
            let e = Self::replicate_state(&skel, &window_receiver, &blob_sender, &blob_recycler);
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        });
        Ok(vec![t_blob_receiver, t_retransmit, t_window, t_server])
    }
}

#[cfg(test)]
pub fn to_packets(r: &packet::PacketRecycler, reqs: Vec<Request>) -> Vec<SharedPackets> {
    let mut out = vec![];
    for rrs in reqs.chunks(packet::NUM_PACKETS) {
        let p = r.allocate();
        p.write()
            .unwrap()
            .packets
            .resize(rrs.len(), Default::default());
        for (i, o) in rrs.iter().zip(p.write().unwrap().packets.iter_mut()) {
            let v = serialize(&i).expect("serialize request");
            let len = v.len();
            o.data[..len].copy_from_slice(&v);
            o.meta.size = len;
        }
        out.push(p);
    }
    return out;
}

#[cfg(test)]
mod tests {
    use accountant_skel::{to_packets, Request};
    use bincode::serialize;
    use ecdsa;
    use packet::{PacketRecycler, NUM_PACKETS};
    use transaction::{memfind, test_tx};

    use accountant::Accountant;
    use accountant_skel::AccountantSkel;
    use accountant_stub::AccountantStub;
    use entry::Entry;
    use futures::Future;
    use historian::Historian;
    use mint::Mint;
    use plan::Plan;
    use recorder::Signal;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::net::{SocketAddr, UdpSocket};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::time::Duration;
    use transaction::Transaction;

    #[test]
    fn test_layout() {
        let tr = test_tx();
        let tx = serialize(&tr).unwrap();
        let packet = serialize(&Request::Transaction(tr)).unwrap();
        assert_matches!(memfind(&packet, &tx), Some(ecdsa::TX_OFFSET));
        assert_matches!(memfind(&packet, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }
    #[test]
    fn test_to_packets() {
        let tr = Request::Transaction(test_tx());
        let re = PacketRecycler::default();
        let rv = to_packets(&re, vec![tr.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), 1);

        let rv = to_packets(&re, vec![tr.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);

        let rv = to_packets(&re, vec![tr.clone(); NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].read().unwrap().packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].read().unwrap().packets.len(), 1);
    }

    #[test]
    fn test_accounting_sequential_consistency() {
        // In this attack we'll demonstrate that a verifier can interpret the ledger
        // differently if either the server doesn't signal the ledger to add an
        // Entry OR if the verifier tries to parallelize across multiple Entries.
        let mint = Mint::new(2);
        let acc = Accountant::new(&mint);
        let rsp_addr: SocketAddr = "0.0.0.0:0".parse().expect("socket address");
        let historian = Historian::new(&mint.last_id(), None);
        let mut skel = AccountantSkel::new(acc, mint.last_id(), sink(), historian);

        // Process a batch that includes a transaction that receives two tokens.
        let alice = KeyPair::new();
        let tr = Transaction::new(&mint.keypair(), alice.pubkey(), 2, mint.last_id());
        let req_vers = vec![(Request::Transaction(tr), rsp_addr, 1_u8)];
        assert!(skel.process_packets(req_vers).is_ok());

        // Process a second batch that spends one of those tokens.
        let tr = Transaction::new(&alice, mint.pubkey(), 1, mint.last_id());
        let req_vers = vec![(Request::Transaction(tr), rsp_addr, 1_u8)];
        assert!(skel.process_packets(req_vers).is_ok());

        // Collect the ledger and feed it to a new accountant.
        skel.historian.sender.send(Signal::Tick).unwrap();
        drop(skel.historian.sender);
        let entries: Vec<Entry> = skel.historian.receiver.iter().collect();

        // Assert the user holds one token, not two. If the server only output one
        // entry, then the second transaction will be rejected, because it drives
        // the account balance below zero before the credit is added.
        let acc = Accountant::new(&mint);
        for entry in entries {
            acc.process_verified_events(entry.events).unwrap();
        }
        assert_eq!(acc.get_balance(&alice.pubkey()), Some(1));
    }

    #[test]
    fn test_accountant_bad_sig() {
        let serve_port = 9002;
        let send_port = 9003;
        let addr = format!("127.0.0.1:{}", serve_port);
        let send_addr = format!("127.0.0.1:{}", send_port);
        let alice = Mint::new(10_000);
        let acc = Accountant::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let historian = Historian::new(&alice.last_id(), Some(30));
        let acc = Arc::new(Mutex::new(AccountantSkel::new(
            acc,
            alice.last_id(),
            sink(),
            historian,
        )));
        let _threads = AccountantSkel::serve(&acc, &addr, exit.clone()).unwrap();
        sleep(Duration::from_millis(300));

        let socket = UdpSocket::bind(send_addr).unwrap();
        socket.set_read_timeout(Some(Duration::new(5, 0))).unwrap();

        let acc = AccountantStub::new(&addr, socket);
        let last_id = acc.get_last_id().wait().unwrap();

        let tr = Transaction::new(&alice.keypair(), bob_pubkey, 500, last_id);

        let _sig = acc.transfer_signed(tr).unwrap();

        let last_id = acc.get_last_id().wait().unwrap();

        let mut tr2 = Transaction::new(&alice.keypair(), bob_pubkey, 501, last_id);
        tr2.data.tokens = 502;
        tr2.data.plan = Plan::new_payment(502, bob_pubkey);
        let _sig = acc.transfer_signed(tr2).unwrap();

        assert_eq!(acc.get_balance(&bob_pubkey).wait().unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
    }

}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use accountant::{Accountant, MAX_ENTRY_IDS};
    use accountant_skel::*;
    use bincode::serialize;
    use hash::hash;
    use mint::Mint;
    use signature::{KeyPair, KeyPairUtil};
    use std::collections::HashSet;
    use std::io::sink;
    use std::time::Instant;
    use transaction::Transaction;

    #[bench]
    fn process_packets_bench(_bencher: &mut Bencher) {
        let mint = Mint::new(100_000_000);
        let acc = Accountant::new(&mint);
        let rsp_addr: SocketAddr = "0.0.0.0:0".parse().expect("socket address");
        // Create transactions between unrelated parties.
        let txs = 100_000;
        let last_ids: Mutex<HashSet<Hash>> = Mutex::new(HashSet::new());
        let transactions: Vec<_> = (0..txs)
            .into_par_iter()
            .map(|i| {
                // Seed the 'to' account and a cell for its signature.
                let dummy_id = i % (MAX_ENTRY_IDS as i32);
                let last_id = hash(&serialize(&dummy_id).unwrap()); // Semi-unique hash
                {
                    let mut last_ids = last_ids.lock().unwrap();
                    if !last_ids.contains(&last_id) {
                        last_ids.insert(last_id);
                        acc.register_entry_id(&last_id);
                    }
                }

                // Seed the 'from' account.
                let rando0 = KeyPair::new();
                let tr = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, last_id);
                acc.process_verified_transaction(&tr).unwrap();

                let rando1 = KeyPair::new();
                let tr = Transaction::new(&rando0, rando1.pubkey(), 2, last_id);
                acc.process_verified_transaction(&tr).unwrap();

                // Finally, return a transaction that's unique
                Transaction::new(&rando0, rando1.pubkey(), 1, last_id)
            })
            .collect();

        let req_vers = transactions
            .into_iter()
            .map(|tr| (Request::Transaction(tr), rsp_addr, 1_u8))
            .collect();

        let historian = Historian::new(&mint.last_id(), None);
        let mut skel = AccountantSkel::new(acc, mint.last_id(), sink(), historian);

        let now = Instant::now();
        assert!(skel.process_packets(req_vers).is_ok());
        let duration = now.elapsed();
        let sec = duration.as_secs() as f64 + duration.subsec_nanos() as f64 / 1_000_000_000.0;
        let tps = txs as f64 / sec;

        // Ensure that all transactions were successfully logged.
        drop(skel.historian.sender);
        let entries: Vec<Entry> = skel.historian.receiver.iter().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].events.len(), txs as usize);

        println!("{} tps", tps);
    }
}
