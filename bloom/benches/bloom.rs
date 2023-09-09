#![feature(test)]

extern crate test;
use {
    bv::BitVec,
    fnv::FnvHasher,
    rand::Rng,
    solana_bloom::bloom::{AtomicBloom, Bloom, BloomHashIndex},
    solana_sdk::{
        hash::{hash, Hash},
        signature::Signature,
    },
    std::{collections::HashSet, hash::Hasher},
    test::Bencher,
};

#[bench]
#[ignore]
fn bench_bits_set(bencher: &mut Bencher) {
    let mut bits: BitVec<u8> = BitVec::new_fill(false, 38_340_234_u64);
    let mut hasher = FnvHasher::default();

    bencher.iter(|| {
        let idx = hasher.finish() % bits.len();
        bits.set(idx, true);
        hasher.write_u64(idx);
    });
    // subtract the next bencher result from this one to get a number for raw
    //  bits.set()
}

#[bench]
#[ignore]
fn bench_bits_set_hasher(bencher: &mut Bencher) {
    let bits: BitVec<u8> = BitVec::new_fill(false, 38_340_234_u64);
    let mut hasher = FnvHasher::default();

    bencher.iter(|| {
        let idx = hasher.finish() % bits.len();
        hasher.write_u64(idx);
    });
}

#[bench]
#[ignore]
fn bench_sigs_bloom(bencher: &mut Bencher) {
    // 1M TPS * 1s (length of block in sigs) == 1M items in filter
    // 1.0E-8 false positive rate
    // https://hur.st/bloomfilter/?n=1000000&p=1.0E-8&m=&k=
    let blockhash = hash(Hash::default().as_ref());
    //    info!("blockhash = {:?}", blockhash);
    let keys = (0..27).map(|i| blockhash.hash_at_index(i)).collect();
    let mut sigs: Bloom<Signature> = Bloom::new(38_340_234, keys);

    let mut id = blockhash;
    let mut falses = 0;
    let mut iterations = 0;
    bencher.iter(|| {
        id = hash(id.as_ref());
        let mut sigbytes = Vec::from(id.as_ref());
        id = hash(id.as_ref());
        sigbytes.extend(id.as_ref());

        let sig = Signature::try_from(sigbytes).unwrap();
        if sigs.contains(&sig) {
            falses += 1;
        }
        sigs.add(&sig);
        sigs.contains(&sig);
        iterations += 1;
    });
    assert_eq!(falses, 0);
}

#[bench]
#[ignore]
fn bench_sigs_hashmap(bencher: &mut Bencher) {
    // same structure as above, new
    let blockhash = hash(Hash::default().as_ref());
    //    info!("blockhash = {:?}", blockhash);
    let mut sigs: HashSet<Signature> = HashSet::new();

    let mut id = blockhash;
    let mut falses = 0;
    let mut iterations = 0;
    bencher.iter(|| {
        id = hash(id.as_ref());
        let mut sigbytes = Vec::from(id.as_ref());
        id = hash(id.as_ref());
        sigbytes.extend(id.as_ref());

        let sig = Signature::try_from(sigbytes).unwrap();
        if sigs.contains(&sig) {
            falses += 1;
        }
        sigs.insert(sig);
        sigs.contains(&sig);
        iterations += 1;
    });
    assert_eq!(falses, 0);
}

#[bench]
fn bench_add_hash(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let hash_values: Vec<_> = std::iter::repeat_with(Hash::new_unique)
        .take(1200)
        .collect();
    let mut fail = 0;
    bencher.iter(|| {
        let mut bloom = Bloom::random(1287, 0.1, 7424);
        for hash_value in &hash_values {
            bloom.add(hash_value);
        }
        let index = rng.gen_range(0..hash_values.len());
        if !bloom.contains(&hash_values[index]) {
            fail += 1;
        }
    });
    assert_eq!(fail, 0);
}

#[bench]
fn bench_add_hash_atomic(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let hash_values: Vec<_> = std::iter::repeat_with(Hash::new_unique)
        .take(1200)
        .collect();
    let mut fail = 0;
    bencher.iter(|| {
        let bloom: AtomicBloom<_> = Bloom::random(1287, 0.1, 7424).into();
        // Intentionally not using parallelism here, so that this and above
        // benchmark only compare the bit-vector ops.
        // For benchmarking the parallel code, change bellow for loop to:
        //     hash_values.par_iter().for_each(|v| bloom.add(v));
        for hash_value in &hash_values {
            bloom.add(hash_value);
        }
        let index = rng.gen_range(0..hash_values.len());
        if !bloom.contains(&hash_values[index]) {
            fail += 1;
        }
    });
    assert_eq!(fail, 0);
}
