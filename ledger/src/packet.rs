//! The `packet` module defines data structures and methods to pull data from the network.
use solana_perf::{
    cuda_runtime::PinnedVec,
    recycler::{Recycler, Reset},
};
pub use solana_sdk::packet::{Meta, Packet, PACKET_DATA_SIZE};
use std::{mem, net::SocketAddr};

pub const NUM_PACKETS: usize = 1024 * 8;

pub const PACKETS_PER_BATCH: usize = 256;
pub const NUM_RCVMMSGS: usize = 128;
pub const PACKETS_BATCH_SIZE: usize = (PACKETS_PER_BATCH * PACKET_DATA_SIZE);

#[derive(Debug, Clone)]
pub struct Packets {
    pub packets: PinnedVec<Packet>,

    recycler: Option<PacketsRecycler>,
}

impl Drop for Packets {
    fn drop(&mut self) {
        if let Some(ref recycler) = self.recycler {
            let old = mem::replace(&mut self.packets, PinnedVec::default());
            recycler.recycle(old)
        }
    }
}

impl Reset for Packets {
    fn reset(&mut self) {
        self.packets.resize(0, Packet::default());
    }
}

//auto derive doesn't support large arrays
impl Default for Packets {
    fn default() -> Packets {
        let packets = PinnedVec::with_capacity(NUM_RCVMMSGS);
        Packets {
            packets,
            recycler: None,
        }
    }
}

pub type PacketsRecycler = Recycler<PinnedVec<Packet>>;

impl Packets {
    pub fn new(packets: Vec<Packet>) -> Self {
        let packets = PinnedVec::from_vec(packets);
        Self {
            packets,
            recycler: None,
        }
    }

    pub fn new_with_recycler(recycler: PacketsRecycler, size: usize, name: &'static str) -> Self {
        let mut packets = recycler.allocate(name);
        packets.reserve_and_pin(size);
        Packets {
            packets,
            recycler: Some(recycler),
        }
    }

    pub fn set_addr(&mut self, addr: &SocketAddr) {
        for m in self.packets.iter_mut() {
            m.meta.set_addr(&addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packets_reset() {
        let mut packets = Packets::default();
        packets.packets.resize(10, Packet::default());
        assert_eq!(packets.packets.len(), 10);
        packets.reset();
        assert_eq!(packets.packets.len(), 0);
    }
}
