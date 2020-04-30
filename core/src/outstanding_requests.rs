use solana_sdk::{clock::Slot, timing::timestamp};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

pub const REQUEST_EXPIRATION_MS: u64 = 1000;
pub const MAX_OUTSTANDING_REQUESTS_PER_SHRED: usize = 2;
// 4 bytes for the u32 representing the nonce
pub const NONCE_BYTES: usize = 4;

#[derive(Default)]
pub struct OutstandingShredRequests {
    requests: [RequestStatus; MAX_OUTSTANDING_REQUESTS_PER_SHRED],
    index: usize,
}

impl OutstandingShredRequests {
    // Returns boolean indicating whether sufficient time has passed for a request with
    // the given timestamp to be made
    fn add_request(&mut self, socket_addr: SocketAddr, nonce: u32) -> bool {
        let now = timestamp();
        if now.saturating_sub(self.requests[self.index].timestamp) >= REQUEST_EXPIRATION_MS {
            self.requests[self.index] = RequestStatus {
                socket_addr,
                timestamp: now,
                nonce,
            };
            self.index = self.index + 1 % MAX_OUTSTANDING_REQUESTS_PER_SHRED;
            true
        } else {
            false
        }
    }

    fn is_valid_request(&self, socket_addr: SocketAddr, nonce: u32) -> bool {
        let now = timestamp();
        for req in self.requests.iter() {
            if req.socket_addr == socket_addr
                && req.nonce == nonce
                && now.saturating_sub(req.timestamp) < REQUEST_EXPIRATION_MS
            {
                return true;
            }
        }
        false
    }
}

pub struct RequestStatus {
    socket_addr: SocketAddr,
    timestamp: u64,
    nonce: u32,
}

impl Default for RequestStatus {
    fn default() -> Self {
        Self {
            socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            timestamp: 0,
            nonce: 0,
        }
    }
}

#[derive(Default)]
pub struct OutstandingRequests {
    requests: HashMap<Slot, HashMap<u32, OutstandingShredRequests>>,
    nonces: HashMap<SocketAddr, u32>,
}

impl OutstandingRequests {
    pub fn add_request(&mut self, slot: Slot, shred_index: u32, socket_addr: SocketAddr) -> bool {
        let slot_outstanding_requests = self.requests.entry(slot).or_default();
        let shred_outstanding_requests = slot_outstanding_requests.entry(shred_index).or_default();
        let nonce = self.nonces.entry(socket_addr).or_default();
        if shred_outstanding_requests.add_request(socket_addr, *nonce) {
            *nonce += 1;
            true
        } else {
            false
        }
    }

    pub fn is_valid_request(
        &mut self,
        slot: Slot,
        index: u32,
        socket_addr: SocketAddr,
        nonce: u32,
    ) -> bool {
        self.requests
            .get(&slot)
            .and_then(|slot_outstanding_requests| {
                slot_outstanding_requests
                    .get(&index)
                    .map(|shred_outstanding_requests| {
                        shred_outstanding_requests.is_valid_request(socket_addr, nonce)
                    })
            })
            .unwrap_or(false)
    }
}
