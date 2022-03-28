#![feature(test)]

extern crate test;

use {
    blstrs::Scalar,
    kzg::{
        coeff_form::KZGProver, coeff_form::KZGVerifier, polynomial::Polynomial, setup, KZGParams,
    },
    lazy_static::lazy_static,
    rayon::prelude::*,
    rayon::ThreadPool,
    sha2::{Digest, Sha256},
    solana_kzg::kzg::{hash_packets, test_create_random_packet_data, test_create_trusted_setup},
    solana_rayon_threadlimit::get_thread_count,
    std::time::Instant,
    test::Bencher,
};

const FEC_SET_SIZE: usize = 96;

lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_thread_count())
        .thread_name(|ix| format!("kzg_{}", ix))
        .build()
        .unwrap();
}

fn create_fec_set_packets(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|_| test_create_random_packet_data())
        .collect()
}

fn fec_set_to_xy_points(packets: &Vec<Vec<u8>>) -> (Vec<Scalar>, Vec<Scalar>) {
    let hashes = hash_packets(packets);
    let mut xs = Vec::default();
    for i in 0..hashes.len() {
        xs.push(Scalar::from_u64s_le(&[0, 0, 0, i as u64]).unwrap());
    }
    let ys: Vec<_> = hashes
        .iter()
        .map(|h| Scalar::from_bytes_le(&h).unwrap())
        .collect();
    (xs, ys)
}

fn create_test_setup(fec_set_size: usize) -> (KZGParams, Vec<Scalar>, Vec<Scalar>) {
    let fec_set = create_fec_set_packets(fec_set_size);
    let (xs, ys) = fec_set_to_xy_points(&fec_set);
    let params = test_create_trusted_setup(fec_set_size);
    (params, xs, ys)
}

#[bench]
fn bench_create_interpolation(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    b.iter(|| {
        let _interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    });
}

#[bench]
fn bench_create_commitment(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    b.iter(|| {
        let prover = KZGProver::new(&params);
        let _commitment = prover.commit(&interpolation);
    });
}

#[bench]
fn bench_create_witness(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let prover = KZGProver::new(&params);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    b.iter(|| {
        prover
            .create_witness(&interpolation, (xs[0], ys[0]))
            .unwrap();
    });
}

#[bench]
#[ignore]
fn bench_create_fec_set_witnesses(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let prover = KZGProver::new(&params);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    b.iter(|| {
        let _witnesses: Vec<_> = xs
            .iter()
            .zip(ys.iter())
            .map(|(x, y)| prover.create_witness(&interpolation, (*x, *y)).unwrap())
            .collect();
    });
}

#[bench]
fn bench_create_fec_set_witnesses_rayon(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let prover = KZGProver::new(&params);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    let points: Vec<_> = xs.into_iter().zip(ys.into_iter()).collect();
    b.iter(|| {
        PAR_THREAD_POOL.install(|| {
            let _witnesses: Vec<_> = points
                .par_iter()
                .map(|(x, y)| prover.create_witness(&interpolation, (*x, *y)).unwrap())
                .collect();
        });
    });
}

#[bench]
fn bench_verify_witness(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let prover = KZGProver::new(&params);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    let commitment = prover.commit(&interpolation);
    let witnesses: Vec<_> = xs
        .iter()
        .zip(ys.iter())
        .map(|(x, y)| prover.create_witness(&interpolation, (*x, *y)).unwrap())
        .collect();
    let verifier = KZGVerifier::new(&params);
    b.iter(|| {
        assert!(verifier.verify_eval((xs[0], ys[0]), &commitment, &witnesses[0]));
    });
}

#[bench]
#[ignore]
fn bench_verify_fec_set_witnesses(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let prover = KZGProver::new(&params);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    let commitment = prover.commit(&interpolation);
    let witnesses: Vec<_> = xs
        .iter()
        .zip(ys.iter())
        .map(|(x, y)| prover.create_witness(&interpolation, (*x, *y)).unwrap())
        .collect();
    b.iter(|| {
        let verifier = KZGVerifier::new(&params);
        for i in 0..witnesses.len() {
            assert!(verifier.verify_eval((xs[i], ys[i]), &commitment, &witnesses[i]));
        }
    });
}

#[bench]
fn bench_verify_fec_set_witnesses_rayon(b: &mut Bencher) {
    let (params, xs, ys) = create_test_setup(FEC_SET_SIZE);
    let prover = KZGProver::new(&params);
    let interpolation = Polynomial::lagrange_interpolation(&xs, &ys);
    let commitment = prover.commit(&interpolation);
    let witnesses: Vec<_> = xs
        .iter()
        .zip(ys.iter())
        .map(|(x, y)| prover.create_witness(&interpolation, (*x, *y)).unwrap())
        .collect();
    b.iter(|| {
        let verifier = KZGVerifier::new(&params);
        (0..witnesses.len()).into_par_iter().for_each(|i| {
            assert!(verifier.verify_eval((xs[i], ys[i]), &commitment, &witnesses[i]));
        });
    });
}
