//! The `accountant_skel` module is a microservice that exposes the high-level
//! Accountant API to the network. Its message encoding is currently
//! in flux. Clients should use AccountantStub to interact with it.

use accountant::Accountant;
use bincode::{deserialize, serialize};
use entry::Entry;
use event::Event;
use hash::Hash;
use historian::Historian;
use rayon::prelude::*;
use recorder::Signal;
use result::Result;
use serde_json;
use signature::PublicKey;
use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, SendError};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use streamer;
use packet;
use std::sync::{Arc, Mutex};
use transaction::Transaction;
use std::collections::VecDeque;

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
            Request::Transaction(ref tr) => tr.verify(),
            _ => true,
        }
    }
}

/// Parallel verfication of a batch of requests.
fn filter_valid_requests(reqs: Vec<(Request, SocketAddr)>) -> Vec<(Request, SocketAddr)> {
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
            writeln!(self.writer, "{}", serde_json::to_string(&entry).unwrap()).unwrap();
        }
        self.last_id
    }

    /// Process Request items sent by clients.
    pub fn log_verified_request(&mut self, msg: Request) -> Option<Response> {
        match msg {
            Request::Transaction(tr) => {
                if let Err(err) = self.acc.process_verified_transaction(&tr) {
                    eprintln!("Transaction error: {:?}", err);
                } else if let Err(SendError(_)) = self.historian
                    .sender
                    .send(Signal::Event(Event::Transaction(tr)))
                {
                    eprintln!("Channel send error");
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

    fn process(
        obj: &Arc<Mutex<AccountantSkel<W>>>,
        packet_receiver: &streamer::PacketReceiver,
        blob_sender: &streamer::BlobSender,
        packet_recycler: &packet::PacketRecycler,
        blob_recycler: &packet::BlobRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let msgs = packet_receiver.recv_timeout(timer)?;
        let msgs_ = msgs.clone();
        let mut rsps = VecDeque::new();
        {
            let mut reqs = vec![];
            for packet in &msgs.read().unwrap().packets {
                let rsp_addr = packet.meta.addr();
                let sz = packet.meta.size;
                let req = deserialize(&packet.data[0..sz])?;
                reqs.push((req, rsp_addr));
            }
            let reqs = filter_valid_requests(reqs);
            for (req, rsp_addr) in reqs {
                if let Some(resp) = obj.lock().unwrap().log_verified_request(req) {
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
        if !rsps.is_empty() {
            //don't wake up the other side if there is nothing
            blob_sender.send(rsps)?;
        }
        packet_recycler.recycle(msgs_);
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
        let skel = obj.clone();
        let t_server = spawn(move || loop {
            let e = AccountantSkel::process(
                &skel,
                &packet_receiver,
                &blob_sender,
                &packet_recycler,
                &blob_recycler,
            );
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        });
        Ok(vec![t_receiver, t_responder, t_server])
    }
}
