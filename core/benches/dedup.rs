#![feature(test)]

extern crate test;

use {
    lru::LruCache,
    rand::prelude::*,
    solana_core::packet_hasher::PacketHasher,
    solana_perf::{packet::PacketBatch, test_tx::test_tx},
    solana_streamer::packet::to_packet_batches,
    std::sync::{Arc, Mutex},
    test::Bencher,
};

const DEFAULT_LRU_SIZE: usize = 200_000;

struct SharedLru(LruCache<u64, ()>);
impl Default for SharedLru {
    fn default() -> Self {
        Self(LruCache::new(DEFAULT_LRU_SIZE))
    }
}

//type SharedLru = Arc<Mutex<LruCache<u64, ()>>>;

pub struct PacketDeduper((SharedLru, PacketHasher));

impl Default for PacketDeduper {
    fn default() -> Self {
        Self((SharedLru::default(), PacketHasher::default()))
    }
}

impl PacketDeduper {
    pub fn dedupe_packets(&mut self, batches: &mut [PacketBatch]) -> usize {
        use rayon::prelude::*;
        let mut total_discard = 0;
        batches.into_iter().for_each(|batch| {
            batch.packets.iter_mut().for_each(|p| {
                if !p.meta.discard() {
                    let h = self.0 .1.hash_packet(p);
                    match self.0 .0 .0.get_mut(&h) {
                        Some(_hash) => {
                            p.meta.set_discard(true);
                            total_discard += 1;
                        },
                        None => {
                            //cache.put(h, ());
                            self.0 .0 .0.put(h, ());
                        }
                    }
                }
            })
        });
        total_discard
    }
}

fn clear_discard(batches: &mut [PacketBatch]) {
    batches.iter_mut().for_each(|batch| {
        batch
            .packets
            .iter_mut()
            .for_each(|p| p.meta.set_discard(false))
    });
}

const SIZE: usize = 32 * 1024;
#[bench]
fn bench_lru_dedup_same_small(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let small_packet = test_packet_with_size(128, &mut rng);

    let mut batches = to_packet_batches(
        &std::iter::repeat(small_packet)
            .take(SIZE)
            .collect::<Vec<_>>(),
        128,
    );

    let mut deduper = PacketDeduper::default();
    bencher.iter(|| {
        assert_eq!(SIZE - 1, deduper.dedupe_packets(&mut batches));
        deduper.0 .0 .0.clear();
        clear_discard(&mut batches);
    });
}

#[bench]
fn bench_lru_dedup_same_big(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let small_packet = test_packet_with_size(1024, &mut rng);

    let mut batches = to_packet_batches(
        &std::iter::repeat(small_packet)
            .take(SIZE)
            .collect::<Vec<_>>(),
        128,
    );

    let mut deduper = PacketDeduper::default();
    bencher.iter(|| {
        assert_eq!(SIZE - 1, deduper.dedupe_packets(&mut batches));
        deduper.0 .0 .0.clear();
        clear_discard(&mut batches);
    });
}

fn test_packet_with_size(size: usize, rng: &mut ThreadRng) -> Vec<u8> {
    // subtract 8 bytes because the length will get serialized as well
    (0..size.checked_sub(8).unwrap())
        .map(|_| rng.gen())
        .collect()
}

#[bench]
fn bench_lru_dedup_diff_small(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let mut batches = to_packet_batches(
        &(0..SIZE)
            .map(|_| test_packet_with_size(128, &mut rng))
            .collect::<Vec<_>>(),
        128,
    );

    let mut deduper = PacketDeduper::default();
    bencher.iter(|| {
        assert_eq!(0, deduper.dedupe_packets(&mut batches));
        deduper.0 .0 .0.clear();
        clear_discard(&mut batches);
    });
}

#[bench]
fn bench_lru_dedup_diff_big(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let mut batches = to_packet_batches(
        &(0..SIZE)
            .map(|_| test_packet_with_size(1024, &mut rng))
            .collect::<Vec<_>>(),
        128,
    );

    let mut deduper = PacketDeduper::default();
    bencher.iter(|| {
        deduper.dedupe_packets(&mut batches);
        deduper.0 .0 .0.clear();
        clear_discard(&mut batches);
    });
}
