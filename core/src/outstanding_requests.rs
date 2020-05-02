use crate::request_response::RequestResponse;
use solana_sdk::{clock::Slot, timing::timestamp};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};

pub const DEFAULT_REQUEST_EXPIRATION_MS: u64 = 10_000;
pub const NONCE_BYTES: usize = 4;

pub type Nonce = u32;

pub struct NodeOutstandingRequests<T, S>
where
    T: RequestResponse<Response = S>,
{
    requests: HashMap<Nonce, RequestStatus<T>>,
    nonce: Nonce,
}

impl<T, S> NodeOutstandingRequests<T, S>
where
    T: RequestResponse<Response = S>,
{
    // Returns boolean indicating whether sufficient time has passed for a request with
    // the given timestamp to be made
    fn add_request(&mut self, request: T) -> Nonce {
        let num_expected_responses = request.num_expected_responses();
        let nonce = self.nonce;
        self.requests.insert(
            nonce,
            RequestStatus {
                expire_timestamp: timestamp() + DEFAULT_REQUEST_EXPIRATION_MS,
                num_expected_responses,
                request,
            },
        );
        self.nonce = (self.nonce + 1) % std::u32::MAX;
        nonce
    }

    fn register_response(&mut self, nonce: u32, response: &S) -> bool {
        let (is_valid, should_delete) = self
            .requests
            .get_mut(&nonce)
            .map(|status| {
                let now = timestamp();
                if status.num_expected_responses > 0
                    && now < status.expire_timestamp
                    && status.request.verify_response(response)
                {
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

impl<T, S> Default for NodeOutstandingRequests<T, S>
where
    T: RequestResponse<Response = S>,
{
    fn default() -> Self {
        Self {
            requests: HashMap::new(),
            nonce: 0,
        }
    }
}

pub struct RequestStatus<T> {
    expire_timestamp: u64,
    num_expected_responses: u32,
    request: T,
}

pub struct OutstandingRequests<T, S>
where
    T: RequestResponse<Response = S>,
{
    requests: HashMap<IpAddr, NodeOutstandingRequests<T, S>>,
}

impl<T, S> OutstandingRequests<T, S>
where
    T: RequestResponse<Response = S>,
{
    pub fn add_request(&mut self, socket_addr: &SocketAddr, request: T) -> Nonce {
        let node_outstanding_requests = self.requests.entry(socket_addr.ip()).or_default();
        node_outstanding_requests.add_request(request)
    }

    pub fn register_response(
        &mut self,
        socket_addr: &SocketAddr,
        nonce: u32,
        response: &S,
    ) -> bool {
        self.requests
            .get_mut(&socket_addr.ip())
            .map(|node_outstanding_requests| {
                node_outstanding_requests.register_response(nonce, response)
            })
            .unwrap_or(false)
    }
}

impl<T, S> Default for OutstandingRequests<T, S>
where
    T: RequestResponse<Response = S>,
{
    fn default() -> Self {
        Self {
            requests: HashMap::new(),
        }
    }
}
