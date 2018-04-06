//! The `accountant_skel` module is a microservice that exposes the high-level
//! Accountant API to the network. Its message encoding is currently
//! in flux. Clients should use AccountantStub to interact with it.

use accountant::Accountant;
use bincode::{deserialize, serialize};
use entry::Entry;
use event::Event;
use ecdsa;
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

/// Parallel verfication of a batch of requests.
pub fn filter_valid_requests(reqs: Vec<(Request, SocketAddr)>) -> Vec<(Request, SocketAddr)> {
    reqs.into_par_iter().filter({ |x| x.0.verify() }).collect()
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

    fn verifier(
        recvr: &streamer::PacketReceiver,
        sendr: &Sender<(Vec<SharedPackets>, Vec<Vec<u8>>)>,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let msgs = recvr.recv_timeout(timer)?;
        trace!("got msgs");
        let mut v = Vec::new();
        v.push(msgs);
        while let Ok(more) = recvr.try_recv() {
            trace!("got more msgs");
            v.push(more);
        }
        info!("batch {}", v.len());
        let chunk = max(1, (v.len() + 3) / 4);
        let chunks: Vec<_> = v.chunks(chunk).collect();
        let rvs: Vec<_> = chunks
            .into_par_iter()
            .map(|x| ecdsa::ed25519_verify(&x.to_vec()))
            .collect();
        for (v, r) in v.chunks(chunk).zip(rvs) {
            sendr.send((v.to_vec(), r))?;
        }
        Ok(())
    }

    pub fn deserialize_packets(p: &packet::Packets) -> Vec<Option<(Request, SocketAddr)>> {
        // TODO: deserealize in parallel
        let mut r = vec![];
        for x in &p.packets {
            let rsp_addr = x.meta.addr();
            let sz = x.meta.size;
            if let Ok(req) = deserialize(&x.data[0..sz]) {
                r.push(Some((req, rsp_addr)));
            } else {
                r.push(None);
            }
        }
        r
    }

    fn process(
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        verified_receiver: &Receiver<(Vec<SharedPackets>, Vec<Vec<u8>>)>,
        blob_sender: &streamer::BlobSender,
        packet_recycler: &packet::PacketRecycler,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let (mms, vvs) = verified_receiver.recv_timeout(timer)?;
        for (msgs, vers) in mms.into_iter().zip(vvs.into_iter()) {
            let msgs_ = msgs.clone();
            let mut rsps = VecDeque::new();
            {
                let reqs = Self::deserialize_packets(&((*msgs).read().unwrap()));
                for (data, v) in reqs.into_iter().zip(vers.into_iter()) {
                    if let Some((req, rsp_addr)) = data {
                        if !req.verify() {
                            continue;
                        }
                        if let Some(resp) = obj.lock().unwrap().log_verified_request(req, v) {
                            let blob = blob_recycler.allocate();
                            {
                                let mut b = blob.write().unwrap();
                                let v = serialize(&resp)?;
                                let len = v.len();
                                b.data[..len].copy_from_slice(&v);
                                b.meta.size = len;
                                b.meta.set_addr(&rsp_addr);
                            }
                            rsps.push_back(blob);
                        }
                    }
                }
            }
            if !rsps.is_empty() {
                //don't wake up the other side if there is nothing
                blob_sender.send(rsps)?;
            }
            packet_recycler.recycle(msgs_);
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
