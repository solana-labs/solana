//! The `request_processor` processes thin client Request messages.

use bank::Bank;
use request::{Request, Response};
use std::net::SocketAddr;
use std::sync::Arc;

pub struct RequestProcessor {
    bank: Arc<Bank>,
}

impl RequestProcessor {
    /// Create a new Tpu that wraps the given Bank.
    pub fn new(bank: Arc<Bank>) -> Self {
        RequestProcessor { bank }
    }

    /// Process Request items sent by clients.
    fn process_request(
        &self,
        msg: Request,
        rsp_addr: SocketAddr,
    ) -> Option<(Response, SocketAddr)> {
        match msg {
            Request::GetBalance { key } => {
                let val = self.bank.get_balance(&key);
                let rsp = (Response::Balance { key, val }, rsp_addr);
                info!("Response::Balance {:?}", rsp);
                Some(rsp)
            }
            Request::GetLastId => {
                let id = self.bank.last_id();
                let rsp = (Response::LastId { id }, rsp_addr);
                info!("Response::LastId {:?}", rsp);
                Some(rsp)
            }
            Request::GetTransactionCount => {
                let transaction_count = self.bank.transaction_count() as u64;
                let rsp = (Response::TransactionCount { transaction_count }, rsp_addr);
                info!("Response::TransactionCount {:?}", rsp);
                Some(rsp)
            }
            Request::GetSignature { signature } => {
                let signature_status = self.bank.check_signature(&signature);
                let rsp = (Response::SignatureStatus { signature_status }, rsp_addr);
                info!("Response::Signature {:?}", rsp);
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
