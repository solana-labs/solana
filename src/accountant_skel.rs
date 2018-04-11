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
use std::sync::mpsc::{channel, Receiver, SendError, Sender};
use std::sync::{Arc, Mutex};
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
#[derive(Serialize, Deserialize, Debug)]
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
    pub fn log_verified_request(&mut self, msg: Request, verify: u8) -> Option<Response> {
        match msg {
            Request::Transaction(_) if verify == 0 => {
                trace!("Transaction failed sigverify");
                None
            }
            Request::Transaction(tr) => {
                if let Err(err) = self.acc.process_verified_transaction(&tr) {
                    trace!("Transaction error: {:?}", err);
                } else if let Err(SendError(_)) = self.historian
                    .sender
                    .send(Signal::Event(Event::Transaction(tr.clone())))
                {
                    error!("Channel send error");
                }
                None
            }
            Request::GetBalance { key } => {
                let val = self.acc.get_balance(&key);
                Some(Response::Balance { key, val })
            }
            Request::GetLastId => Some(Response::LastId { id: self.sync() }),
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

    fn process_packets(
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        req_vers: Vec<(Request, SocketAddr, u8)>,
    ) -> Vec<(Response, SocketAddr)> {
        req_vers
            .into_iter()
            .filter_map(|(req, rsp_addr, v)| {
                let mut skel = obj.lock().unwrap();
                skel.log_verified_request(req, v)
                    .map(|resp| (resp, rsp_addr))
            })
            .collect()
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
            let rsps = Self::process_packets(obj, req_vers);
            let blobs = Self::serialize_responses(rsps, blob_recycler)?;
            if !blobs.is_empty() {
                //don't wake up the other side if there is nothing
                blob_sender.send(blobs)?;
            }
            packet_recycler.recycle(msgs);
        }
        Ok(())
    }

    /// Create a UDP microservice that forwards messages the given AccountantSkel.
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
            let e = Self::verifier(&packet_receiver, &verified_sender);
            if e.is_err() && exit_.load(Ordering::Relaxed) {
                break;
            }
        });

        let skel = obj.clone();
        let t_server = spawn(move || loop {
            let e = AccountantSkel::process(
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
}

#[cfg(test)]
mod tests {
    use accountant_skel::Request;
    use bincode::serialize;
    use ecdsa;
    use transaction::{memfind, test_tx};
    #[test]
    fn test_layout() {
        let tr = test_tx();
        let tx = serialize(&tr).unwrap();
        let packet = serialize(&Request::Transaction(tr)).unwrap();
        assert_matches!(memfind(&packet, &tx), Some(ecdsa::TX_OFFSET));
    }
}
