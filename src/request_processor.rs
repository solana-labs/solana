//! The `request_processor` processes thin client Request messages.

use bank::Bank;
use request::{Request, Response};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

pub struct RequestProcessor {
    bank: Arc<RwLock<Bank>>,
}

impl RequestProcessor {
    /// Create a new Tpu that wraps the given Bank.
    pub fn new(bank: Arc<RwLock<Bank>>) -> Self {
        RequestProcessor { bank }
    }

    /// Process Request items sent by clients.
    fn process_request(
        &self,
        msg: Request,
        rsp_addr: SocketAddr,
    ) -> Option<(Response, SocketAddr)> {
        match msg {
            Request::GetAccount { key } => {
                let account = self.bank.read().unwrap().get_account(&key);
                let rsp = (Response::Account { key, account }, rsp_addr);
                info!("Response::Account {:?}", rsp);
                Some(rsp)
            }
            Request::GetLastId => {
                let id = self.bank.read().unwrap().last_id();
                let rsp = (Response::LastId { id }, rsp_addr);
                info!("Response::LastId {:?}", rsp);
                Some(rsp)
            }
            Request::GetTransactionCount => {
                let transaction_count = self.bank.read().unwrap().transaction_count() as u64;
                let rsp = (Response::TransactionCount { transaction_count }, rsp_addr);
                info!("Response::TransactionCount {:?}", rsp);
                Some(rsp)
            }
            Request::GetSignature { signature } => {
                let signature_status = self.bank.read().unwrap().has_signature(&signature);
                let rsp = (Response::SignatureStatus { signature_status }, rsp_addr);
                info!("Response::Signature {:?}", rsp);
                Some(rsp)
            }
            Request::GetFinality => {
                let time = self.bank.read().unwrap().finality();
                let rsp = (Response::Finality { time }, rsp_addr);
                info!("Response::Finality {:?}", rsp);
                Some(rsp)
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
}
