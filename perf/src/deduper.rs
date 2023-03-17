//! Utility to deduplicate baches of incoming network packets.

use {
    crate::packet::{Packet, PacketBatch},
    ahash::AHasher,
    rand::Rng,
    std::{
        hash::{Hash, Hasher},
        iter::repeat_with,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    },
};

pub struct Deduper<const K: usize> {
    num_bits: u64,
    bits: Vec<AtomicU64>,
    seeds: [(u128, u128); K],
    clock: Instant,
    // Maximum number of one bits before the false positive
    // rate exceeds the specified threshold.
    capacity: u64,
    popcount: AtomicU64, // Number of one bits in self.bits.
}

impl<const K: usize> Deduper<K> {
    pub fn new<R: Rng>(rng: &mut R, false_positive_rate: f64, num_bits: u64) -> Self {
        assert!(0.0 < false_positive_rate && false_positive_rate < 1.0);
        let size = usize::try_from(num_bits.checked_add(63).unwrap() / 64).unwrap();
        let capacity = num_bits as f64 * false_positive_rate.powf(1f64 / K as f64);
        Self {
            num_bits,
            seeds: std::array::from_fn(|_| rng.gen()),
            clock: Instant::now(),
            bits: repeat_with(AtomicU64::default).take(size).collect(),
            capacity: capacity as u64,
            popcount: AtomicU64::default(),
        }
    }

    pub fn maybe_reset<R: Rng>(&mut self, rng: &mut R, reset_cycle: &Duration) {
        let popcount = self.popcount.load(Ordering::Relaxed);
        if popcount >= self.capacity || &self.clock.elapsed() >= reset_cycle {
            self.seeds = std::array::from_fn(|_| rng.gen());
            self.clock = Instant::now();
            self.bits.fill_with(AtomicU64::default);
            self.popcount = AtomicU64::default();
        }
    }

    // Returns true if the packet is duplicate.
    #[must_use]
    #[allow(clippy::integer_arithmetic)]
    pub fn dedup_packet(&self, packet: &Packet) -> bool {
        // Should not dedup packet if already discarded.
        debug_assert!(!packet.meta().discard());
        let mut out = true;
        for seed in self.seeds {
            let mut hasher = AHasher::new_with_keys(seed.0, seed.1);
            packet.data(..).unwrap_or_default().hash(&mut hasher);
            let hash: u64 = hasher.finish() % self.num_bits;
            let index = (hash >> 6) as usize;
            let mask: u64 = 1u64 << (hash & 63);
            let old = self.bits[index].fetch_or(mask, Ordering::Relaxed);
            if old & mask == 0u64 {
                self.popcount.fetch_add(1, Ordering::Relaxed);
                out = false;
            }
        }
        out
    }

    pub fn dedup_packets_and_count_discards(
        &self,
        batches: &mut [PacketBatch],
        mut process_received_packet: impl FnMut(&mut Packet, bool, bool),
    ) -> u64 {
        batches
            .iter_mut()
            .flat_map(PacketBatch::iter_mut)
            .map(|packet| {
                if packet.meta().discard() {
                    process_received_packet(packet, true, false);
                } else if self.dedup_packet(packet) {
                    packet.meta_mut().set_discard(true);
                    process_received_packet(packet, false, true);
                } else {
                    process_received_packet(packet, false, false);
                }
                u64::from(packet.meta().discard())
            })
            .sum()
    }
}

#[cfg(test)]
#[allow(clippy::integer_arithmetic)]
mod tests {
    use {
        super::*,
        crate::{packet::to_packet_batches, sigverify, test_tx::test_tx},
        rand::SeedableRng,
        rand_chacha::ChaChaRng,
        solana_sdk::packet::{Meta, PACKET_DATA_SIZE},
        test_case::test_case,
    };

    #[test]
    fn test_dedup_same() {
        let tx = test_tx();

        let mut batches =
            to_packet_batches(&std::iter::repeat(tx).take(1024).collect::<Vec<_>>(), 128);
        let packet_count = sigverify::count_packets_in_batches(&batches);
        let mut rng = rand::thread_rng();
        let filter = Deduper::<2>::new(
            &mut rng, /*false_positive_rate:*/ 0.001, /*num_bits:*/ 63_999_979,
        );
        let mut num_deduped = 0;
        let discard = filter.dedup_packets_and_count_discards(
            &mut batches,
            |_deduped_packet, _removed_before_sigverify_stage, _is_dup| {
                num_deduped += 1;
            },
        ) as usize;
        assert_eq!(num_deduped, discard + 1);
        assert_eq!(packet_count, discard + 1);
    }

    #[test]
    fn test_dedup_diff() {
        let mut rng = rand::thread_rng();
        let mut filter = Deduper::<2>::new(
            &mut rng, /*false_positive_rate:*/ 0.001, /*num_bits:*/ 63_999_979,
        );
        let mut batches = to_packet_batches(&(0..1024).map(|_| test_tx()).collect::<Vec<_>>(), 128);
        let discard = filter.dedup_packets_and_count_discards(&mut batches, |_, _, _| ()) as usize;
        // because dedup uses a threadpool, there maybe up to N threads of txs that go through
        assert_eq!(discard, 0);
        filter.maybe_reset(&mut rng, /*reset_cycle:*/ &Duration::from_millis(0));
        for i in filter.bits {
            assert_eq!(i.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    #[ignore]
    fn test_dedup_saturated() {
        let mut rng = rand::thread_rng();
        let filter = Deduper::<2>::new(
            &mut rng, /*false_positive_rate:*/ 0.001, /*num_bits:*/ 63_999_979,
        );
        let mut discard = 0;
        assert!(filter.popcount.load(Ordering::Relaxed) < filter.capacity);
        for i in 0..1000 {
            let mut batches =
                to_packet_batches(&(0..1000).map(|_| test_tx()).collect::<Vec<_>>(), 128);
            discard += filter.dedup_packets_and_count_discards(&mut batches, |_, _, _| ()) as usize;
            trace!("{} {}", i, discard);
            if filter.popcount.load(Ordering::Relaxed) >= filter.capacity {
                break;
            }
        }
        assert!(filter.popcount.load(Ordering::Relaxed) >= filter.capacity);
    }

    #[test]
    fn test_dedup_false_positive() {
        let mut rng = rand::thread_rng();
        let filter = Deduper::<2>::new(
            &mut rng, /*false_positive_rate:*/ 0.001, /*num_bits:*/ 63_999_979,
        );
        let mut discard = 0;
        for i in 0..10 {
            let mut batches =
                to_packet_batches(&(0..1024).map(|_| test_tx()).collect::<Vec<_>>(), 128);
            discard += filter.dedup_packets_and_count_discards(&mut batches, |_, _, _| ()) as usize;
            debug!("false positive rate: {}/{}", discard, i * 1024);
        }
        //allow for 1 false positive even if extremely unlikely
        assert!(discard < 2);
    }

    #[test_case(63_999_979, 0.001, 2_023_857)]
    #[test_case(622_401_961, 0.001, 19_682_078)]
    #[test_case(622_401_979, 0.001, 19_682_078)]
    #[test_case(629_145_593, 0.001, 19_895_330)]
    #[test_case(632_455_543, 0.001, 20_000_000)]
    #[test_case(637_534_199, 0.001, 20_160_601)]
    #[test_case(622_401_961, 0.0001, 6_224_019)]
    #[test_case(622_401_979, 0.0001, 6_224_019)]
    #[test_case(629_145_593, 0.0001, 6_291_455)]
    #[test_case(632_455_543, 0.0001, 6_324_555)]
    #[test_case(637_534_199, 0.0001, 6_375_341)]
    fn test_dedup_capacity(num_bits: u64, false_positive_rate: f64, capacity: u64) {
        let mut rng = rand::thread_rng();
        let deduper = Deduper::<2>::new(&mut rng, false_positive_rate, num_bits);
        assert_eq!(deduper.capacity, capacity);
    }

    #[test_case([0xf9; 32],  3_199_997, 101_192,  51_414,  70, 101_125)]
    #[test_case([0xdc; 32],  3_200_003, 101_192,  51_414,  71, 101_132)]
    #[test_case([0xa5; 32],  6_399_971, 202_384, 102_828, 127, 202_157)]
    #[test_case([0xdb; 32],  6_400_013, 202_386, 102_828, 145, 202_277)]
    #[test_case([0xcd; 32], 12_799_987, 404_771, 205_655, 289, 404_434)]
    #[test_case([0xc3; 32], 12_800_009, 404_771, 205_656, 309, 404_278)]
    fn test_dedup_seeded(
        seed: [u8; 32],
        num_bits: u64,
        capacity: u64,
        num_packets: usize,
        num_dups: usize,
        popcount: u64,
    ) {
        let mut rng = ChaChaRng::from_seed(seed);
        let deduper = Deduper::<2>::new(&mut rng, /*false_positive_rate:*/ 0.001, num_bits);
        assert_eq!(deduper.capacity, capacity);
        let mut packet = Packet::new([0u8; PACKET_DATA_SIZE], Meta::default());
        let mut dup_count = 0usize;
        for _ in 0..num_packets {
            let size = rng.gen_range(0, PACKET_DATA_SIZE);
            packet.meta_mut().size = size;
            rng.fill(&mut packet.buffer_mut()[0..size]);
            if deduper.dedup_packet(&packet) {
                dup_count += 1;
            }
            assert!(deduper.dedup_packet(&packet));
        }
        assert_eq!(dup_count, num_dups);
        assert_eq!(deduper.popcount.load(Ordering::Relaxed), popcount);
    }
}
