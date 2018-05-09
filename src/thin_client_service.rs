//! The `thin_client_service` sits alongside the TPU and queries it for information
//! on behalf of thing clients.

use accountant::Accountant;
use bincode::serialize;
use entry::Entry;
use hash::Hash;
use signature::PublicKey;
use std::net::{SocketAddr, UdpSocket};
//use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use transaction::Transaction;

pub struct ThinClientService {
    //pub output: Mutex<Receiver<Response>>,
    //response_sender: Mutex<Sender<Response>>,
    pub acc: Arc<Accountant>,
    entry_info_subscribers: Mutex<Vec<SocketAddr>>,
}

impl ThinClientService {
    /// Create a new Tpu that wraps the given Accountant.
    pub fn new(acc: Arc<Accountant>) -> Self {
        //let (response_sender, output) = channel();
        ThinClientService {
            //output: Mutex::new(output),
            //response_sender: Mutex::new(response_sender),
            acc,
            entry_info_subscribers: Mutex::new(vec![]),
        }
    }

    /// Process Request items sent by clients.
    fn process_request(
        &self,
        msg: Request,
        rsp_addr: SocketAddr,
    ) -> Option<(Response, SocketAddr)> {
        match msg {
            Request::GetBalance { key } => {
                let val = self.acc.get_balance(&key);
                let rsp = (Response::Balance { key, val }, rsp_addr);
                info!("Response::Balance {:?}", rsp);
                Some(rsp)
            }
            Request::Transaction(_) => unreachable!(),
            Request::Subscribe { subscriptions } => {
                for subscription in subscriptions {
                    match subscription {
                        Subscription::EntryInfo => {
                            self.entry_info_subscribers.lock().unwrap().push(rsp_addr)
                        }
                    }
                }
                None
            }
        }
    }

    pub fn process_requests(
        &self,
        reqs: Vec<(Request, SocketAddr)>,
    ) -> Vec<(Response, SocketAddr)> {
        reqs.into_iter()
            .filter_map(|(req, rsp_addr)| self.process_request(req, rsp_addr))
            .collect()
    }

    pub fn notify_entry_info_subscribers(&self, entry: &Entry) {
        // TODO: No need to bind().
        let socket = UdpSocket::bind("0.0.0.0:0").expect("bind");

        // copy subscribers to avoid taking lock while doing io
        let addrs = self.entry_info_subscribers.lock().unwrap().clone();
        trace!("Sending to {} addrs", addrs.len());
        for addr in addrs {
            let entry_info = EntryInfo {
                id: entry.id,
                num_hashes: entry.num_hashes,
                num_events: entry.events.len() as u64,
            };
            let data = serialize(&Response::EntryInfo(entry_info)).expect("serialize EntryInfo");
            trace!("sending {} to {}", data.len(), addr);
            //TODO dont do IO here, this needs to be on a separate channel
            let res = socket.send_to(&data, addr);
            if res.is_err() {
                eprintln!("couldn't send response: {:?}", res);
            }
        }
    }
}

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
    Transaction(Transaction),
    GetBalance { key: PublicKey },
    Subscribe { subscriptions: Vec<Subscription> },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Subscription {
    EntryInfo,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntryInfo {
    pub id: Hash,
    pub num_hashes: u64,
    pub num_events: u64,
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
    EntryInfo(EntryInfo),
}
