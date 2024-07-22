use {
    criterion::{criterion_group, criterion_main, Criterion},
    rand::prelude::*,
    rand_chacha::ChaChaRng,
    solana_lattice_hash::lt_hash::LtHash,
};

fn new_random_lt_hash(rng: &mut impl Rng) -> LtHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&rng.gen::<u64>().to_le_bytes());
    LtHash::with(&hasher)
}

fn bench_mix_in(c: &mut Criterion) {
    let mut rng = ChaChaRng::seed_from_u64(11);
    let mut lt_hash1 = new_random_lt_hash(&mut rng);
    let lt_hash2 = new_random_lt_hash(&mut rng);

    c.bench_function("mix_in", |b| {
        b.iter(|| lt_hash1.mix_in(&lt_hash2));
    });
}

fn bench_mix_out(c: &mut Criterion) {
    let mut rng = ChaChaRng::seed_from_u64(22);
    let mut lt_hash1 = new_random_lt_hash(&mut rng);
    let lt_hash2 = new_random_lt_hash(&mut rng);

    c.bench_function("mix_out", |b| {
        b.iter(|| lt_hash1.mix_out(&lt_hash2));
    });
}

fn bench_checksum(c: &mut Criterion) {
    let mut rng = ChaChaRng::seed_from_u64(33);
    let lt_hash = new_random_lt_hash(&mut rng);

    c.bench_function("checksum", |b| {
        b.iter(|| lt_hash.checksum());
    });
}

fn bench_with(c: &mut Criterion) {
    let mut rng = ChaChaRng::seed_from_u64(44);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&rng.gen::<u64>().to_le_bytes());

    c.bench_function("with", |b| {
        b.iter(|| LtHash::with(&hasher));
    });
}

criterion_group!(
    benches,
    bench_mix_in,
    bench_mix_out,
    bench_checksum,
    bench_with
);
criterion_main!(benches);
