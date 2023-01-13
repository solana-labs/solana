//! The `packet` module defines data structures and methods to pull data from the network.
pub use solana_sdk::packet::{Meta, Packet, PacketFlags, PACKET_DATA_SIZE};
use {
    crate::{cuda_runtime::PinnedVec, recycler::Recycler},
    bincode::config::Options,
    rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, IntoParallelRefMutIterator},
    serde::{de::DeserializeOwned, Serialize},
    std::{
        io::Read,
        net::SocketAddr,
        ops::{Index, IndexMut},
        slice::{Iter, IterMut, SliceIndex},
    },
};

pub const NUM_PACKETS: usize = 1024 * 8;

pub const PACKETS_PER_BATCH: usize = 64;
pub const NUM_RCVMMSGS: usize = 64;

#[derive(Debug, Default, Clone)]
pub struct PacketBatch {
    packets: PinnedVec<Packet>,
}

pub type PacketBatchRecycler = Recycler<PinnedVec<Packet>>;

impl PacketBatch {
    pub fn new(packets: Vec<Packet>) -> Self {
        let packets = PinnedVec::from_vec(packets);
        Self { packets }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let packets = PinnedVec::with_capacity(capacity);
        PacketBatch { packets }
    }

    pub fn new_pinned_with_capacity(capacity: usize) -> Self {
        let mut batch = Self::with_capacity(capacity);
        batch.packets.reserve_and_pin(capacity);
        batch
    }

    pub fn new_unpinned_with_recycler(
        recycler: PacketBatchRecycler,
        capacity: usize,
        name: &'static str,
    ) -> Self {
        let mut packets = recycler.allocate(name);
        packets.reserve(capacity);
        PacketBatch { packets }
    }

    pub fn new_with_recycler(
        recycler: PacketBatchRecycler,
        capacity: usize,
        name: &'static str,
    ) -> Self {
        let mut packets = recycler.allocate(name);
        packets.reserve_and_pin(capacity);
        PacketBatch { packets }
    }

    pub fn new_with_recycler_data(
        recycler: &PacketBatchRecycler,
        name: &'static str,
        mut packets: Vec<Packet>,
    ) -> Self {
        let mut batch = Self::new_with_recycler(recycler.clone(), packets.len(), name);
        batch.packets.append(&mut packets);
        batch
    }

    pub fn new_unpinned_with_recycler_data_and_dests<T: Serialize>(
        recycler: PacketBatchRecycler,
        name: &'static str,
        dests_and_data: &[(SocketAddr, T)],
    ) -> Self {
        let mut batch =
            PacketBatch::new_unpinned_with_recycler(recycler, dests_and_data.len(), name);
        batch
            .packets
            .resize(dests_and_data.len(), Packet::default());

        for ((addr, data), packet) in dests_and_data.iter().zip(batch.packets.iter_mut()) {
            if !addr.ip().is_unspecified() && addr.port() != 0 {
                if let Err(e) = Packet::populate_packet(packet, Some(addr), &data) {
                    // TODO: This should never happen. Instead the caller should
                    // break the payload into smaller messages, and here any errors
                    // should be propagated.
                    error!("Couldn't write to packet {:?}. Data skipped.", e);
                }
            } else {
                trace!("Dropping packet, as destination is unknown");
            }
        }
        batch
    }

    pub fn new_unpinned_with_recycler_data(
        recycler: &PacketBatchRecycler,
        name: &'static str,
        mut packets: Vec<Packet>,
    ) -> Self {
        let mut batch = Self::new_unpinned_with_recycler(recycler.clone(), packets.len(), name);
        batch.packets.append(&mut packets);
        batch
    }

    pub fn resize(&mut self, new_len: usize, value: Packet) {
        self.packets.resize(new_len, value)
    }

    pub fn truncate(&mut self, len: usize) {
        self.packets.truncate(len);
    }

    pub fn push(&mut self, packet: Packet) {
        self.packets.push(packet);
    }

    pub fn set_addr(&mut self, addr: &SocketAddr) {
        for p in self.iter_mut() {
            p.meta_mut().set_socket_addr(addr);
        }
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn capacity(&self) -> usize {
        self.packets.capacity()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    pub fn as_ptr(&self) -> *const Packet {
        self.packets.as_ptr()
    }

    pub fn iter(&self) -> Iter<'_, Packet> {
        self.packets.iter()
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, Packet> {
        self.packets.iter_mut()
    }

    /// See Vector::set_len() for more details
    ///
    /// # Safety
    ///
    /// - `new_len` must be less than or equal to [`self.capacity`].
    /// - The elements at `old_len..new_len` must be initialized. Packet data
    ///   will likely be overwritten when populating the packet, but the meta
    ///   should specifically be initialized to known values.
    pub unsafe fn set_len(&mut self, new_len: usize) {
        self.packets.set_len(new_len);
    }
}

impl<I: SliceIndex<[Packet]>> Index<I> for PacketBatch {
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        &self.packets[index]
    }
}

impl<I: SliceIndex<[Packet]>> IndexMut<I> for PacketBatch {
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.packets[index]
    }
}

impl<'a> IntoIterator for &'a PacketBatch {
    type Item = &'a Packet;
    type IntoIter = Iter<'a, Packet>;

    fn into_iter(self) -> Self::IntoIter {
        self.packets.iter()
    }
}

impl<'a> IntoParallelIterator for &'a PacketBatch {
    type Iter = rayon::slice::Iter<'a, Packet>;
    type Item = &'a Packet;
    fn into_par_iter(self) -> Self::Iter {
        self.packets.par_iter()
    }
}

impl<'a> IntoParallelIterator for &'a mut PacketBatch {
    type Iter = rayon::slice::IterMut<'a, Packet>;
    type Item = &'a mut Packet;
    fn into_par_iter(self) -> Self::Iter {
        self.packets.par_iter_mut()
    }
}

impl From<PacketBatch> for Vec<Packet> {
    fn from(batch: PacketBatch) -> Self {
        batch.packets.into()
    }
}

pub fn to_packet_batches<T: Serialize>(items: &[T], chunk_size: usize) -> Vec<PacketBatch> {
    items
        .chunks(chunk_size)
        .map(|batch_items| {
            let mut batch = PacketBatch::with_capacity(batch_items.len());
            batch.resize(batch_items.len(), Packet::default());
            for (item, packet) in batch_items.iter().zip(batch.packets.iter_mut()) {
                Packet::populate_packet(packet, None, item).expect("serialize request");
            }
            batch
        })
        .collect()
}

#[cfg(test)]
pub fn to_packet_batches_for_tests<T: Serialize>(items: &[T]) -> Vec<PacketBatch> {
    to_packet_batches(items, NUM_PACKETS)
}

pub fn deserialize_from_with_limit<R, T>(reader: R) -> bincode::Result<T>
where
    R: Read,
    T: DeserializeOwned,
{
    // with_limit causes pre-allocation size to be limited
    // to prevent against memory exhaustion attacks.
    bincode::options()
        .with_limit(PACKET_DATA_SIZE as u64)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize_from(reader)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            system_transaction,
        },
    };

    #[test]
    fn test_to_packet_batches() {
        let keypair = Keypair::new();
        let hash = Hash::new(&[1; 32]);
        let tx = system_transaction::transfer(&keypair, &keypair.pubkey(), 1, hash);
        let rv = to_packet_batches_for_tests(&[tx.clone(); 1]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].len(), 1);

        #[allow(clippy::useless_vec)]
        let rv = to_packet_batches_for_tests(&vec![tx.clone(); NUM_PACKETS]);
        assert_eq!(rv.len(), 1);
        assert_eq!(rv[0].len(), NUM_PACKETS);

        #[allow(clippy::useless_vec)]
        let rv = to_packet_batches_for_tests(&vec![tx; NUM_PACKETS + 1]);
        assert_eq!(rv.len(), 2);
        assert_eq!(rv[0].len(), NUM_PACKETS);
        assert_eq!(rv[1].len(), 1);
    }

    #[test]
    fn test_to_packets_pinning() {
        let recycler = PacketBatchRecycler::default();
        for i in 0..2 {
            let _first_packets =
                PacketBatch::new_with_recycler(recycler.clone(), i + 1, "first one");
        }
    }
}
