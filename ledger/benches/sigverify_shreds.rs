#![feature(test)]

extern crate test;
use {
    solana_ledger::{
        shred::{Shred, ShredFlags, SIZE_OF_DATA_SHRED_PAYLOAD},
        sigverify_shreds::{sign_shreds_cpu, sign_shreds_gpu, sign_shreds_gpu_pinned_keypair},
    },
    solana_perf::{
        packet::{Packet, PacketBatch},
        recycler_cache::RecyclerCache,
    },
    solana_sdk::signature::Keypair,
    std::sync::Arc,
    test::Bencher,
};

const NUM_PACKETS: usize = 256;
const NUM_BATCHES: usize = 1;
#[bench]
fn bench_sigverify_shreds_sign_gpu(bencher: &mut Bencher) {
    let recycler_cache = RecyclerCache::default();

    let mut packet_batch = PacketBatch::default();
    packet_batch.packets.set_pinnable();
    let slot = 0xdead_c0de;
    // need to pin explicitly since the resize will not cause re-allocation
    packet_batch.packets.reserve_and_pin(NUM_PACKETS);
    packet_batch.packets.resize(NUM_PACKETS, Packet::default());
    for p in packet_batch.packets.iter_mut() {
        let shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            &[5; SIZE_OF_DATA_SHRED_PAYLOAD],
            ShredFlags::LAST_SHRED_IN_SLOT,
            1,
            2,
            0,
        );
        shred.copy_to_packet(p);
    }
    let mut batches = vec![packet_batch; NUM_BATCHES];
    let keypair = Keypair::new();
    let pinned_keypair = sign_shreds_gpu_pinned_keypair(&keypair, &recycler_cache);
    let pinned_keypair = Some(Arc::new(pinned_keypair));
    //warmup
    for _ in 0..100 {
        sign_shreds_gpu(&keypair, &pinned_keypair, &mut batches, &recycler_cache);
    }
    bencher.iter(|| {
        sign_shreds_gpu(&keypair, &pinned_keypair, &mut batches, &recycler_cache);
    })
}

#[bench]
fn bench_sigverify_shreds_sign_cpu(bencher: &mut Bencher) {
    let mut packet_batch = PacketBatch::default();
    let slot = 0xdead_c0de;
    packet_batch.packets.resize(NUM_PACKETS, Packet::default());
    for p in packet_batch.packets.iter_mut() {
        let shred = Shred::new_from_data(
            slot,
            0xc0de,
            0xdead,
            &[5; SIZE_OF_DATA_SHRED_PAYLOAD],
            ShredFlags::LAST_SHRED_IN_SLOT,
            1,
            2,
            0,
        );
        shred.copy_to_packet(p);
    }
    let mut batches = vec![packet_batch; NUM_BATCHES];
    let keypair = Keypair::new();
    bencher.iter(|| {
        sign_shreds_cpu(&keypair, &mut batches);
    })
}
