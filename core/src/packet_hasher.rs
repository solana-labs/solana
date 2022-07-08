// Get a unique hash value for a packet
// Used in retransmit and shred fetch to prevent dos with same packet data.

use {
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_perf::packet::Packet,
    std::hash::Hasher,
};

#[derive(Clone)]
pub(crate) struct PacketHasher {
    seed1: u128,
    seed2: u128,
}

impl Default for PacketHasher {
    fn default() -> Self {
        Self {
            seed1: thread_rng().gen::<u128>(),
            seed2: thread_rng().gen::<u128>(),
        }
    }
}

impl PacketHasher {
    pub(crate) fn hash_packet(&self, packet: &Packet) -> u64 {
        self.hash_data(packet.data(..).unwrap_or_default())
    }

    pub(crate) fn hash_shred(&self, shred: &[u8]) -> u64 {
        self.hash_data(shred)
    }

    fn hash_data(&self, data: &[u8]) -> u64 {
        let mut hasher = AHasher::new_with_keys(self.seed1, self.seed2);
        hasher.write(data);
        hasher.finish()
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}
