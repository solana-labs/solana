//! The `packet` module defines data structures and methods to pull data from the network.
use crate::{cuda_runtime::PinnedVec, recycler::Recycler};
use bincode::config::Options;
use serde::Serialize;
pub use solana_sdk::packet::{Meta, Packet, PACKET_DATA_SIZE};
use std::{net::SocketAddr, time::Instant};

pub const NUM_PACKETS: usize = 1024 * 8;

pub const PACKETS_PER_BATCH: usize = 128;
pub const NUM_RCVMMSGS: usize = 128;

#[derive(Copy, Clone, Debug, Default)]
pub struct PacketTimer {
    // timestamp marking when the first packets in the batch were received from the socket
    incoming_start: Option<Instant>,
    // timestamp marking when the final packets in the batch were received from the socket
    incoming_end: Option<Instant>,
    // timestamp to mark when outgoing packets begin processing
    outgoing_start: Option<Instant>,
    // count of batch structures which have been coalesced into this structure
    num_coalesced: usize,
}

impl PacketTimer {
    pub fn mark_incoming_start(&mut self) {
        if self.incoming_start == None {
            self.incoming_start = Some(Instant::now());
        }
    }

    pub fn mark_incoming_end(&mut self) {
        self.incoming_end = Some(Instant::now());
    }

    pub fn mark_outgoing_start(&mut self) {
        if self.outgoing_start == None {
            self.outgoing_start = Some(Instant::now());
        }
    }

    pub fn coalesce_with(&mut self, pkt_timer: &PacketTimer) {
        if pkt_timer.incoming_start < self.incoming_start {
            self.incoming_start = pkt_timer.incoming_start;
        }
        if pkt_timer.incoming_end > self.incoming_end {
            self.incoming_end = pkt_timer.incoming_end;
        }
        if pkt_timer.outgoing_start < self.outgoing_start {
            self.outgoing_start = pkt_timer.outgoing_start;
        }
        self.num_coalesced = self
            .num_coalesced
            .saturating_add(1)
            .saturating_add(pkt_timer.num_coalesced);
    }

    pub fn incoming_start(&self) -> Option<Instant> {
        self.incoming_start
    }

    pub fn incoming_end(&self) -> Option<Instant> {
        self.incoming_end
    }

    pub fn num_coalesced(&self) -> usize {
        self.num_coalesced
    }
}

#[derive(Debug, Default, Clone)]
pub struct Packets {
    pub packets: PinnedVec<Packet>,
    pub timer: PacketTimer,
}

pub type PacketsRecycler = Recycler<PinnedVec<Packet>>;

impl Packets {
    pub fn new(packets: Vec<Packet>) -> Self {
        let packets = PinnedVec::from_vec(packets);
        Self {
            packets,
            timer: PacketTimer::default(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let packets = PinnedVec::with_capacity(capacity);
        Packets {
            packets,
            timer: PacketTimer::default(),
        }
    }

    pub fn new_unpinned_with_recycler(
        recycler: PacketsRecycler,
        size: usize,
        name: &'static str,
    ) -> Self {
        let mut packets = recycler.allocate(name);
        packets.reserve(size);
        Packets {
            packets,
            timer: PacketTimer::default(),
        }
    }

    pub fn new_with_recycler(recycler: PacketsRecycler, size: usize, name: &'static str) -> Self {
        let mut packets = recycler.allocate(name);
        packets.reserve_and_pin(size);
        Packets {
            packets,
            timer: PacketTimer::default(),
        }
    }

    pub fn new_with_recycler_data(
        recycler: &PacketsRecycler,
        name: &'static str,
        mut packets: Vec<Packet>,
    ) -> Self {
        let mut vec = Self::new_with_recycler(recycler.clone(), packets.len(), name);
        vec.packets.append(&mut packets);
        vec
    }

    pub fn new_unpinned_with_recycler_data(
        recycler: &PacketsRecycler,
        name: &'static str,
        mut packets: Vec<Packet>,
    ) -> Self {
        let mut vec = Self::new_unpinned_with_recycler(recycler.clone(), packets.len(), name);
        vec.packets.append(&mut packets);
        vec
    }

    pub fn set_addr(&mut self, addr: &SocketAddr) {
        for m in self.packets.iter_mut() {
            m.meta.set_addr(addr);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

pub fn to_packets_chunked<T: Serialize>(xs: &[T], chunks: usize) -> Vec<Packets> {
    let mut out = vec![];
    for x in xs.chunks(chunks) {
        let mut p = Packets::with_capacity(x.len());
        p.packets.resize(x.len(), Packet::default());
        for (i, o) in x.iter().zip(p.packets.iter_mut()) {
            Packet::populate_packet(o, None, i).expect("serialize request");
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
pub fn to_packets<T: Serialize>(xs: &[T]) -> Vec<Packets> {
    to_packets_chunked(xs, NUM_PACKETS)
}

pub fn to_packets_with_destination<T: Serialize>(
    recycler: PacketsRecycler,
    dests_and_data: &[(SocketAddr, T)],
) -> Packets {
    let mut out = Packets::new_unpinned_with_recycler(
        recycler,
        dests_and_data.len(),
        "to_packets_with_destination",
    );
    out.packets.resize(dests_and_data.len(), Packet::default());
    for (dest_and_data, o) in dests_and_data.iter().zip(out.packets.iter_mut()) {
        if !dest_and_data.0.ip().is_unspecified() && dest_and_data.0.port() != 0 {
            if let Err(e) = Packet::populate_packet(o, Some(&dest_and_data.0), &dest_and_data.1) {
                // TODO: This should never happen. Instead the caller should
                // break the payload into smaller messages, and here any errors
                // should be propagated.
                error!("Couldn't write to packet {:?}. Data skipped.", e);
            }
        } else {
            trace!("Dropping packet, as destination is unknown");
        }
    }
    out
}

pub fn limited_deserialize<T>(data: &[u8]) -> bincode::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    bincode::options()
        .with_limit(PACKET_DATA_SIZE as u64)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize_from(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, Signer};
    use solana_sdk::system_transaction;

    #[test]
    fn test_to_packets() {
        let keypair = Keypair::new();
        let hash = Hash::new(&[1; 32]);
        let tx = system_transaction::transfer(&keypair, &keypair.pubkey(), 1, hash);
        let rv = to_packets(&[tx.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].packets.len(), 1);

        #[allow(clippy::useless_vec)]
        let rv = to_packets(&vec![tx.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].packets.len(), NUM_PACKETS);

        #[allow(clippy::useless_vec)]
        let rv = to_packets(&vec![tx; NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].packets.len(), NUM_PACKETS);
        assert_eq!(rv[1].packets.len(), 1);
    }

    #[test]
    fn test_to_packets_pinning() {
        let recycler = PacketsRecycler::default();
        for i in 0..2 {
            let _first_packets = Packets::new_with_recycler(recycler.clone(), i + 1, "first one");
        }
    }
}
