//! The `packet` module defines data structures and methods to pull data from the network.
use crate::{
    cuda_runtime::PinnedVec,
    recycler::{Recycler, Reset},
};
use serde::Serialize;
pub use solana_sdk::packet::{Meta, Packet, PACKET_DATA_SIZE};
use std::{io, mem, net::SocketAddr};

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

pub fn to_packets_chunked<T: Serialize>(xs: &[T], chunks: usize) -> Vec<Packets> {
    let mut out = vec![];
    for x in xs.chunks(chunks) {
        let mut p = Packets::default();
        p.packets.resize(x.len(), Packet::default());
        for (i, o) in x.iter().zip(p.packets.iter_mut()) {
            let mut wr = io::Cursor::new(&mut o.data[..]);
            bincode::serialize_into(&mut wr, &i).expect("serialize request");
            let len = wr.position() as usize;
            o.meta.size = len;
        }
        out.push(p);
    }
    out
}

pub fn to_packets<T: Serialize>(xs: &[T]) -> Vec<Packets> {
    to_packets_chunked(xs, NUM_PACKETS)
}

pub fn limited_deserialize<T>(data: &[u8]) -> bincode::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    bincode::config()
        .limit(PACKET_DATA_SIZE as u64)
        .deserialize(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;

    #[test]
    fn test_packets_reset() {
        let mut packets = Packets::default();
        packets.packets.resize(10, Packet::default());
        assert_eq!(packets.packets.len(), 10);
        packets.reset();
        assert_eq!(packets.packets.len(), 0);
    }

    #[test]
    fn test_to_packets() {
        let keypair = Keypair::new();
        let hash = Hash::new(&[1; 32]);
        let tx = system_transaction::transfer(&keypair, &keypair.pubkey(), 1, hash);
        let rv = to_packets(&vec![tx.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].packets.len(), 1);

        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].packets.len(), NUM_PACKETS);

        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].packets.len(), 1);
    }
}
