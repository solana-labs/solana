use std::io;
use accountant::Accountant;
use transaction::Transaction;
use signature::PublicKey;
use hash::Sha256Hash;
use log::Entry;
use std::net::UdpSocket;
use bincode::{deserialize, serialize};

pub struct AccountantSkel {
    pub acc: Accountant,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    Transaction(Transaction<i64>),
    GetBalance { key: PublicKey },
    GetEntries { last_id: Sha256Hash },
    GetId { is_last: bool },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Balance { key: PublicKey, val: Option<i64> },
    Entries { entries: Vec<Entry<i64>> },
    Id { id: Sha256Hash, is_last: bool },
}

impl AccountantSkel {
    pub fn new(acc: Accountant) -> Self {
        AccountantSkel { acc }
    }

    pub fn process_request(self: &mut Self, msg: Request) -> Option<Response> {
        match msg {
            Request::Transaction(tr) => {
                if let Err(err) = self.acc.process_transaction(tr) {
                    eprintln!("Transaction error: {:?}", err);
                }
                None
            }
            Request::GetBalance { key } => {
                let val = self.acc.get_balance(&key);
                Some(Response::Balance { key, val })
            }
            Request::GetEntries { .. } => Some(Response::Entries { entries: vec![] }),
            Request::GetId { is_last } => Some(Response::Id {
                id: if is_last {
                    self.acc.sync();
                    self.acc.last_id
                } else {
                    self.acc.first_id
                },
                is_last,
            }),
        }
    }

    /// UDP Server that forwards messages to Accountant methods.
    pub fn serve(self: &mut Self, addr: &str) -> io::Result<()> {
        let socket = UdpSocket::bind(addr)?;
        let mut buf = vec![0u8; 1024];
        loop {
            //println!("skel: Waiting for incoming packets...");
            let (_sz, src) = socket.recv_from(&mut buf)?;

            // TODO: Return a descriptive error message if deserialization fails.
            let req = deserialize(&buf).expect("deserialize request");

            if let Some(resp) = self.process_request(req) {
                socket.send_to(&serialize(&resp).expect("serialize response"), &src)?;
            }
        }
    }
}
