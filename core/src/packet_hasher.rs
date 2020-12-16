// Get a unique hash value for a packet
// Used in retransmit and shred fetch to prevent dos with same packet data.

use ahash::AHasher;
use rand::{thread_rng, Rng};
use solana_perf::packet::Packet;
use std::hash::Hasher;

#[derive(Clone)]
pub struct PacketHasher {
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
    pub fn hash_packet(&self, packet: &Packet) -> u64 {
        let mut hasher = AHasher::new_with_keys(self.seed1, self.seed2);
        hasher.write(&packet.data[0..packet.meta.size]);
        hasher.finish()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
