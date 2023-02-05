//! Utility to deduplicate baches of incoming network packets.

use {
    crate::packet::{Packet, PacketBatch},
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_sdk::saturating_add_assign,
    std::{
        convert::TryFrom,
        hash::Hasher,
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        time::{Duration, Instant},
    },
};

pub struct Deduper {
    filter: Vec<AtomicU64>,
    seed: (u128, u128),
    age: Instant,
    max_age: Duration,
    pub saturated: AtomicBool,
}

impl Deduper {
    pub fn new(size: u32, max_age: Duration) -> Self {
        let mut filter: Vec<AtomicU64> = Vec::with_capacity(size as usize);
        filter.resize_with(size as usize, Default::default);
        let seed = thread_rng().gen();
        Self {
            filter,
            seed,
            age: Instant::now(),
            max_age,
            saturated: AtomicBool::new(false),
        }
    }

    pub fn reset(&mut self) {
        let now = Instant::now();
        //this should reset every 500k unique packets per 1m sized deduper
        //false positive rate is 1/1000 at that point
        let saturated = self.saturated.load(Ordering::Relaxed);
        if saturated || now.duration_since(self.age) > self.max_age {
            let len = self.filter.len();
            self.filter.clear();
            self.filter.resize_with(len, AtomicU64::default);
            self.seed = thread_rng().gen();
            self.age = now;
            self.saturated.store(false, Ordering::Relaxed);
        }
    }

    /// Compute hash from packet data, returns (hash, bin_pos).
    fn compute_hash(&self, packet: &Packet) -> (u64, usize) {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(packet.data(..).unwrap_or_default());
        let h = hasher.finish();
        let len = self.filter.len();
        let pos = (usize::try_from(h).unwrap()).wrapping_rem(len);
        (h, pos)
    }

    // Deduplicates packets and returns 1 if packet is to be discarded. Else, 0.
    fn dedup_packet(&self, packet: &mut Packet) -> u64 {
        // If this packet was already marked as discard, drop it
        if packet.meta().discard() {
            return 1;
        }
        let (hash, pos) = self.compute_hash(packet);
        // saturate each position with or
        let prev = self.filter[pos].fetch_or(hash, Ordering::Relaxed);
        if prev == u64::MAX {
            self.saturated.store(true, Ordering::Relaxed);
            //reset this value
            self.filter[pos].store(hash, Ordering::Relaxed);
        }
        if hash == prev & hash {
            packet.meta_mut().set_discard(true);
            return 1;
        }
        0
    }

    pub fn dedup_packets_and_count_discards(
        &self,
        batches: &mut [PacketBatch],
        mut process_received_packet: impl FnMut(&mut Packet, bool, bool),
    ) -> u64 {
        let mut num_removed: u64 = 0;
        batches.iter_mut().for_each(|batch| {
            batch.iter_mut().for_each(|p| {
                let removed_before_sigverify = p.meta().discard();
                let is_duplicate = self.dedup_packet(p);
                if is_duplicate == 1 {
                    saturating_add_assign!(num_removed, 1);
                }
                process_received_packet(p, removed_before_sigverify, is_duplicate == 1);
            })
        });
        num_removed
    }
}

#[cfg(test)]
#[allow(clippy::integer_arithmetic)]
mod tests {
    use {
        super::*,
        crate::{packet::to_packet_batches, sigverify, test_tx::test_tx},
    };

    #[test]
    fn test_dedup_same() {
        let tx = test_tx();

        let mut batches =
            to_packet_batches(&std::iter::repeat(tx).take(1024).collect::<Vec<_>>(), 128);
        let packet_count = sigverify::count_packets_in_batches(&batches);
        let filter = Deduper::new(1_000_000, Duration::from_millis(0));
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
        let mut filter = Deduper::new(1_000_000, Duration::from_millis(0));
        let mut batches = to_packet_batches(&(0..1024).map(|_| test_tx()).collect::<Vec<_>>(), 128);
        let discard = filter.dedup_packets_and_count_discards(&mut batches, |_, _, _| ()) as usize;
        // because dedup uses a threadpool, there maybe up to N threads of txs that go through
        assert_eq!(discard, 0);
        filter.reset();
        for i in filter.filter {
            assert_eq!(i.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    #[ignore]
    fn test_dedup_saturated() {
        let filter = Deduper::new(1_000_000, Duration::from_millis(0));
        let mut discard = 0;
        assert!(!filter.saturated.load(Ordering::Relaxed));
        for i in 0..1000 {
            let mut batches =
                to_packet_batches(&(0..1000).map(|_| test_tx()).collect::<Vec<_>>(), 128);
            discard += filter.dedup_packets_and_count_discards(&mut batches, |_, _, _| ()) as usize;
            trace!("{} {}", i, discard);
            if filter.saturated.load(Ordering::Relaxed) {
                break;
            }
        }
        assert!(filter.saturated.load(Ordering::Relaxed));
    }

    #[test]
    fn test_dedup_false_positive() {
        let filter = Deduper::new(1_000_000, Duration::from_millis(0));
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
}
