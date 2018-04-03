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
use std::default::Default;
use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, SendError};
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
        r_reader: &streamer::Receiver,
        s_responder: &streamer::Responder,
        packet_recycler: &streamer::PacketRecycler,
        response_recycler: &streamer::ResponseRecycler,
    ) -> Result<()> {
        let timer = Duration::new(1, 0);
        let msgs = r_reader.recv_timeout(timer)?;
        let msgs_ = msgs.clone();
        let rsps = streamer::allocate(response_recycler);
        let rsps_ = rsps.clone();
        {
            let mut reqs = vec![];
            for packet in &msgs.read().unwrap().packets {
                let rsp_addr = packet.meta.get_addr();
                let sz = packet.meta.size;
                let req = deserialize(&packet.data[0..sz])?;
                reqs.push((req, rsp_addr));
            }
            let reqs = filter_valid_requests(reqs);

            let mut num = 0;
            let mut ursps = rsps.write().unwrap();
            for (req, rsp_addr) in reqs {
                if let Some(resp) = obj.lock().unwrap().log_verified_request(req) {
                    if ursps.responses.len() <= num {
                        ursps
                            .responses
                            .resize((num + 1) * 2, streamer::Response::default());
                    }
                    let rsp = &mut ursps.responses[num];
                    let v = serialize(&resp)?;
                    let len = v.len();
                    rsp.data[..len].copy_from_slice(&v);
                    rsp.meta.size = len;
                    rsp.meta.set_addr(&rsp_addr);
                    num += 1;
                }
            }
            ursps.responses.resize(num, streamer::Response::default());
        }
        s_responder.send(rsps_)?;
        streamer::recycle(packet_recycler, msgs_);
        Ok(())
    }

    /// Create a UDP microservice that forwards messages the given AccountantSkel.
    /// Set `exit` to shutdown its threads.
    pub fn serve(
        obj: Arc<Mutex<AccountantSkel<W>>>,
        addr: &str,
        exit: Arc<AtomicBool>,
    ) -> Result<Vec<JoinHandle<()>>> {
        let read = UdpSocket::bind(addr)?;
        // make sure we are on the same interface
        let mut local = read.local_addr()?;
        local.set_port(0);
        let write = UdpSocket::bind(local)?;

        let packet_recycler = Arc::new(Mutex::new(Vec::new()));
        let response_recycler = Arc::new(Mutex::new(Vec::new()));
        let (s_reader, r_reader) = channel();
        let t_receiver = streamer::receiver(read, exit.clone(), packet_recycler.clone(), s_reader)?;

        let (s_responder, r_responder) = channel();
        let t_responder =
            streamer::responder(write, exit.clone(), response_recycler.clone(), r_responder);

        let skel = obj.clone();
        let t_server = spawn(move || loop {
            let e = AccountantSkel::process(
                &skel,
                &r_reader,
                &s_responder,
                &packet_recycler,
                &response_recycler,
            );
            if e.is_err() && exit.load(Ordering::Relaxed) {
                break;
            }
        });

        Ok(vec![t_receiver, t_responder, t_server])
    }
}
