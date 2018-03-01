use std::io;
use accountant::Accountant;
use log::{PublicKey, Signature};
//use serde::Serialize;

pub struct AccountantSkel {
    pub obj: Accountant,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    Deposit {
        key: PublicKey,
        val: u64,
        sig: Signature,
    },
    Transfer {
        from: PublicKey,
        to: PublicKey,
        val: u64,
        sig: Signature,
    },
    GetBalance {
        key: PublicKey,
    },
    Wait {
        sig: Signature,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Balance { key: PublicKey, val: u64 },
    Confirmed { sig: Signature },
}

impl AccountantSkel {
    pub fn new(obj: Accountant) -> Self {
        AccountantSkel { obj }
    }

    pub fn process_request(self: &mut Self, msg: Request) -> Option<Response> {
        match msg {
            Request::Deposit { key, val, sig } => {
                let _ = self.obj.deposit_signed(key, val, sig);
                None
            }
            Request::Transfer { from, to, val, sig } => {
                let _ = self.obj.transfer_signed(from, to, val, sig);
                None
            }
            Request::GetBalance { key } => {
                let val = self.obj.get_balance(&key).unwrap();
                Some(Response::Balance { key, val })
            }
            Request::Wait { sig } => {
                self.obj.wait_on_signature(&sig);
                Some(Response::Confirmed { sig })
            }
        }
    }

    /// UDP Server that forwards messages to Accountant methods.
    pub fn serve(self: &mut Self, addr: &str) -> io::Result<()> {
        use std::net::UdpSocket;
        use bincode::{deserialize, serialize};
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
