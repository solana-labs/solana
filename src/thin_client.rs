//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use bincode::{deserialize, serialize};
use hash::Hash;
use request::{Request, Response};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use transaction::Transaction;
use std::sync::mpsc::{Sender, RecvError, TryRecvError, channel};
use std::thread;
use mio::net::UdpSocket;
use mio::{Events, Ready, Poll, PollOpt, Token};
use std::time::Duration;

const SENDER: Token = Token(0);
const RECEIVER: Token = Token(1);

pub enum Message {
    Requests(Request),
    Transactions(Transaction),
}

pub struct ThinClient {
    last_id: Option<Hash>,
    transaction_count: u64,
    balances: HashMap<PublicKey, Option<i64>>,
    sender: Sender<(Message, Sender<Response>)>,
}

impl ThinClient {
    /// Create a new ThinClient that will interface with Rpu
    /// over `requests_socket` and `events_socket`.
    pub fn new(
        requests_server_addr: SocketAddr,
        requests_addr: SocketAddr,
        transactions_server_addr: SocketAddr,
        transactions_addr: SocketAddr,
    ) -> Self {
        let (sender, receiver) = channel();
        let client = ThinClient {
            last_id: None,
            transaction_count: 0,
            balances: HashMap::new(),
            sender,
        };
        thread::spawn(move || {
            loop {
                let client_addr = ThinClientAddr::new(
                    requests_server_addr,
                    requests_addr,
                    transactions_server_addr,
                    transactions_addr,
                );
                match receiver.try_recv() {
                    Ok(tuple) => {
                        let (message, tx) = tuple;
                        Self::process_message(client_addr, message, tx).expect("thin client process message");
                    },
                    Err(TryRecvError::Empty) => {},
                    Err(TryRecvError::Disconnected) => {},
                };
            }
        });
        client
    }

    fn process_message(mut client_addr: ThinClientAddr, message: Message, response_sender: Sender<Response>) -> io::Result<usize> {
        match message {
            Message::Requests(request) => {
                let resp = client_addr.poll_for_data(request).expect("transaction_count response");
                // Need error resp (timeout) handling here
                info!("recv_response {:?}", resp);
                response_sender.send(resp).expect("send response from thread");
                Ok(0) // CLEAN THIS UP
            }
            Message::Transactions(tx) => {
                client_addr.send_event_data(tx)
            }
        }
    }

    // Process data returned from requests_socket, populating the ThinClient struct
    pub fn process_response(&mut self, resp: Response) {
        match resp {
            Response::Balance { key, val } => {
                trace!("Response balance {:?} {:?}", key, val);
                self.balances.insert(key, val);
            }
            Response::LastId { id } => {
                info!("Response last_id {:?}", id);
                self.last_id = Some(id);
            }
            Response::TransactionCount { transaction_count } => {
                info!("Response transaction count {:?}", transaction_count);
                self.transaction_count = transaction_count;
            }
        }
    }

    /// Send a signed Transaction to the server for processing.
    pub fn transfer_signed(&self, tx: Transaction) -> io::Result<()> {
        let (response_sender, _response_receiver) = channel();
        self.sender.send((Message::Transactions(tx), response_sender)).expect("send request to thread");
        Ok(()) // CLEAN THIS UP
    }

    /// Creates, signs, and processes a Transaction. Useful for writing unit-tests.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: &Hash,
    ) -> io::Result<Signature> {
        let tx = Transaction::new(keypair, to, n, *last_id);
        let sig = tx.sig;
        self.transfer_signed(tx).map(|_| sig)
    }

    /// Request the balance of the user holding `pubkey`.
    pub fn get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        info!("get_balance");
        let req = Request::GetBalance { key: *pubkey };
        let (response_sender, response_receiver) = channel();
        self.sender.send((Message::Requests(req), response_sender)).expect("send request to thread");
        let handle = thread::spawn(move || {
            loop{
                match response_receiver.recv() {
                    Ok(response) => {
                        info!("recv_response {:?}", response);
                        return response;
                    },
                    Err(RecvError) =>{},
                };
            }
        });
        let response = handle.join().expect("response data from thread");
        self.process_response(response);
        self.balances[pubkey].ok_or(io::Error::new(io::ErrorKind::Other, "nokey"))
    }

    /// Request the transaction count.
    pub fn get_transaction_count(&mut self) -> u64 {
        info!("transaction_count");
        let req = Request::GetTransactionCount;
        let (response_sender, response_receiver) = channel();
        self.sender.send((Message::Requests(req), response_sender)).expect("send request to thread");
        let handle = thread::spawn(move || {
            loop{
                match response_receiver.recv() {
                    Ok(response) => {
                        info!("recv_response {:?}", response);
                        return response;
                    },
                    Err(RecvError) =>{},
                };
            }
        });
        let response = handle.join().expect("response data from thread");
        self.process_response(response);
        self.transaction_count
    }

    /// Request the last Entry ID from the server.
    pub fn get_last_id(&mut self) -> Hash {
        info!("get_last_id");
        let req = Request::GetLastId;
        let (response_sender, response_receiver) = channel();
        self.sender.send((Message::Requests(req), response_sender)).expect("send request to thread");
        let handle = thread::spawn(move || {
            loop{
                match response_receiver.recv() {
                    Ok(response) => {
                        info!("recv_response {:?}", response);
                        return response;
                    },
                    Err(RecvError) =>{},
                };
            }
        });
        let response = handle.join().expect("response data from thread");
        self.process_response(response);
        self.last_id.expect("some last_id")
    }

    pub fn poll_get_balance(&mut self, pubkey: &PublicKey) -> io::Result<i64> {
        use std::time::Instant;

        let mut balance;
        let now = Instant::now();
        loop {
            balance = self.get_balance(pubkey);
            if balance.is_ok() || now.elapsed().as_secs() > 1 {
                break;
            }
        }
        balance
    }
}

pub struct ThinClientAddr {
    requests_server_addr: SocketAddr,
    requests_addr: SocketAddr,
    transactions_server_addr: SocketAddr,
    transactions_addr: SocketAddr,
}

impl ThinClientAddr {
    pub fn new(
        requests_server_addr: SocketAddr,
        requests_addr: SocketAddr,
        transactions_server_addr: SocketAddr,
        transactions_addr: SocketAddr,
    ) -> Self {
        ThinClientAddr{
            requests_server_addr,
            requests_addr,
            transactions_server_addr,
            transactions_addr,
        }
    }

    /// Send Request to UDP `requests_socket`, and poll for Response
    /// `timeout` and `num_tries` prevent infinite blocking
    pub fn poll_for_data(&mut self, req: Request) -> io::Result<Response> {
        let timeout = 3_000;
        let mut num_tries = 3;

        let requests_socket = UdpSocket::bind(&self.requests_addr)?;

        let poll = Poll::new()?;
        poll.register(&requests_socket, SENDER, Ready::writable(), PollOpt::edge())?;

        let data = serialize(&req).expect("serialize request data in send_request_data");

        let mut events = Events::with_capacity(8);
        loop {
            poll.poll(&mut events, Some(Duration::from_millis(timeout)))?;
            for event in events.iter() {
                trace!("thin client poll event: {:?}",event);
                match event.token() {
                    SENDER => {
                        trace!("sending thin_client request to socket");
                        let bytes_sent = requests_socket.send_to(&data,&self.requests_server_addr)?;
                        trace!("{:?} bytes sent", bytes_sent);
                        poll.reregister(&requests_socket, RECEIVER, Ready::readable(), PollOpt::edge())?;
                    },
                    RECEIVER => {
                        let mut buf = vec![0u8; 1024];
                        info!("data received from socket to thin_client");
                        let num_recv = requests_socket.recv(&mut buf)?;
                        let resp: Response = deserialize(&buf).expect("deserialize balance in thin_client");
                        trace!("Response data num {:?}, {:?}", num_recv, resp);
                        return Ok(resp);
                    }
                    _ => unreachable!()
                }
            }
            if events.is_empty() {
                debug!("thin_client recv timeout reached");
                if num_tries > 0 {
                    debug!("thin_client retry poll for data, {:?} attempts remaining", num_tries);
                    num_tries -= 1;
                    poll.reregister(&requests_socket, SENDER, Ready::writable(), PollOpt::edge())?;
                }
                else {
                    debug!("thin_client recv attempt max reached");
                    let custom_error = io::Error::new(io::ErrorKind::TimedOut, "thin client receive timed out");
                    return Err(custom_error);
                }
            }
        }
    }

    /// Send Event to UDP `events_socket`
    /// Does not wait for a response
    pub fn send_event_data(&self, tx: Transaction) -> io::Result<usize> {
        let transactions_socket = UdpSocket::bind(&self.transactions_addr)?;

        let poll = Poll::new()?;
        poll.register(&transactions_socket, SENDER, Ready::writable(), PollOpt::edge())?;

        let data = serialize(&tx).expect("serialize request data in send_request_data");

        let mut events = Events::with_capacity(8);
        loop {
            poll.poll(&mut events, Some(Duration::from_millis(1000)))?;
            for event in events.iter() {
                match event.token() {
                    SENDER => {
                        trace!("sending thin_client event to socket");
                        let bytes_sent = transactions_socket.send_to(&data,&self.transactions_server_addr)?;
                        trace!("{:?} bytes sent", bytes_sent);
                        return Ok(bytes_sent);
                    }
                    _ => unreachable!()
                }
            }
            // Need timeout and error handling?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use budget::Budget;
    use crdt::TestNode;
    use logger;
    use mint::Mint;
    use server::Server;
    use signature::{KeyPair, KeyPairUtil};
    use std::io::sink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use transaction::{Instruction, Plan};

    #[test]
    fn test_thin_client() {
        logger::setup();
        let leader = TestNode::new();

        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let server = Server::new_leader(
            bank,
            Some(Duration::from_millis(30)),
            leader.data.clone(),
            leader.sockets.requests,
            leader.sockets.transaction,
            leader.sockets.broadcast,
            leader.sockets.respond,
            leader.sockets.gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(900));

        let requests_addr = "0.0.0.0:0".parse().unwrap();
        let transactions_addr = "0.0.0.0:0".parse().unwrap();

        let mut client = ThinClient::new(
            leader.data.requests_addr,
            requests_addr,
            leader.data.transactions_addr,
            transactions_addr,
        );
        let last_id = client.get_last_id();
        let _sig = client
            .transfer(500, &alice.keypair(), bob_pubkey, &last_id)
            .unwrap();
        let balance = client.poll_get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }

    #[test]
    fn test_bad_sig() {
        logger::setup();
        let leader = TestNode::new();
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));

        let server = Server::new_leader(
            bank,
            Some(Duration::from_millis(30)),
            leader.data.clone(),
            leader.sockets.requests,
            leader.sockets.transaction,
            leader.sockets.broadcast,
            leader.sockets.respond,
            leader.sockets.gossip,
            exit.clone(),
            sink(),
        );
        sleep(Duration::from_millis(300));

        let requests_addr = "0.0.0.0:0".parse().unwrap();
        let transactions_addr = "0.0.0.0:0".parse().unwrap();
        let mut client = ThinClient::new(
            leader.data.requests_addr,
            requests_addr,
            leader.data.transactions_addr,
            transactions_addr,
        );
        let last_id = client.get_last_id();

        let tx = Transaction::new(&alice.keypair(), bob_pubkey, 500, last_id);

        let _sig = client.transfer_signed(tx).unwrap();

        let last_id = client.get_last_id();

        let mut tr2 = Transaction::new(&alice.keypair(), bob_pubkey, 501, last_id);
        if let Instruction::NewContract(contract) = &mut tr2.instruction {
            contract.tokens = 502;
            contract.plan = Plan::Budget(Budget::new_payment(502, bob_pubkey));
        }
        let _sig = client.transfer_signed(tr2).unwrap();

        let balance = client.poll_get_balance(&bob_pubkey);
        assert_eq!(balance.unwrap(), 500);
        exit.store(true, Ordering::Relaxed);
        for t in server.thread_hdls {
            t.join().unwrap();
        }
    }
}
