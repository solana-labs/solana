use solana_sdk::{clock::Slot, timing::timestamp};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    ops::Range,
};

pub const DEFAULT_REQUEST_EXPIRATION_MS: u64 = 10_000;
pub const NONCE_BYTES: usize = 4;

pub type Nonce = u32;

#[derive(Default)]
pub struct NodeOutstandingRequests {
    requests: HashMap<Nonce, RequestStatus>,
    nonce: Nonce,
}

impl NodeOutstandingRequests {
    // Returns boolean indicating whether sufficient time has passed for a request with
    // the given timestamp to be made
    fn add_request(&mut self, num_expected_responses: usize) -> Nonce {
        let nonce = self.nonce;
        self.requests.insert(
            nonce,
            RequestStatus {
                expire_timestamp: timestamp() + DEFAULT_REQUEST_EXPIRATION_MS,
                num_expected_responses,
                slot_range: 0..0,
                index_range: 0..0,
            },
        );
        self.nonce = (self.nonce + 1) % std::u32::MAX;
        nonce
    }

    fn register_response(&mut self, nonce: u32) -> bool {
        let (is_valid, should_delete) = self
            .requests
            .get_mut(&nonce)
            .map(|status| {
                let now = timestamp();
                if status.num_expected_responses > 0 && now < status.expire_timestamp {
                    status.num_expected_responses -= 1;
                    (true, false)
                } else {
                    (false, true)
                }
            })
            .unwrap_or((false, false));

        if should_delete {
            self.requests
                .remove(&nonce)
                .expect("Delete must delete existing object");
        }

        is_valid
    }
}

pub struct RequestStatus {
    expire_timestamp: u64,
    num_expected_responses: usize,
    slot_range: Range<Slot>,
    index_range: Range<Slot>,
}

#[derive(Default)]
pub struct OutstandingRequests {
    requests: HashMap<IpAddr, NodeOutstandingRequests>,
}

impl OutstandingRequests {
    pub fn add_request(
        &mut self,
        socket_addr: &SocketAddr,
        num_expected_responses: usize,
    ) -> Nonce {
        let node_outstanding_requests = self.requests.entry(socket_addr.ip()).or_default();
        node_outstanding_requests.add_request(num_expected_responses)
    }

    pub fn register_response(&mut self, socket_addr: &SocketAddr, nonce: u32) -> bool {
        self.requests
            .get_mut(&socket_addr.ip())
            .map(|node_outstanding_requests| node_outstanding_requests.register_response(nonce))
            .unwrap_or(false)
    }
}
