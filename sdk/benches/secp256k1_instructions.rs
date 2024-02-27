#![feature(test)]

extern crate test;
use {
    rand0_7::{thread_rng, Rng},
    solana_sdk::{
        feature_set::FeatureSet,
        hash::Hash,
        secp256k1_instruction::new_secp256k1_instruction,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
    test::Bencher,
};

// 5K transactions should be enough for benching loop
const TX_COUNT: u16 = 5120;

// prepare a bunch of unique txs
fn create_test_transactions(message_length: u16) -> Vec<Transaction> {
    (0..TX_COUNT)
        .map(|_| {
            let mut rng = thread_rng();
            let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
            let message: Vec<u8> = (0..message_length).map(|_| rng.gen_range(0, 255)).collect();
            let secp_instruction = new_secp256k1_instruction(&secp_privkey, &message);
            let mint_keypair = Keypair::new();

            Transaction::new_signed_with_payer(
                &[secp_instruction.clone()],
                Some(&mint_keypair.pubkey()),
                &[&mint_keypair],
                Hash::default(),
            )
        })
        .collect()
}

#[bench]
fn bench_secp256k1_len_032(b: &mut Bencher) {
    let feature_set = FeatureSet::all_enabled();
    let txs = create_test_transactions(32);
    let mut tx_iter = txs.iter().cycle();
    b.iter(|| {
        tx_iter
            .next()
            .unwrap()
            .verify_precompiles(&feature_set)
            .unwrap();
    });
}

#[bench]
fn bench_secp256k1_len_256(b: &mut Bencher) {
    let feature_set = FeatureSet::all_enabled();
    let txs = create_test_transactions(256);
    let mut tx_iter = txs.iter().cycle();
    b.iter(|| {
        tx_iter
            .next()
            .unwrap()
            .verify_precompiles(&feature_set)
            .unwrap();
    });
}

#[bench]
fn bench_secp256k1_len_32k(b: &mut Bencher) {
    let feature_set = FeatureSet::all_enabled();
    let txs = create_test_transactions(32 * 1024);
    let mut tx_iter = txs.iter().cycle();
    b.iter(|| {
        tx_iter
            .next()
            .unwrap()
            .verify_precompiles(&feature_set)
            .unwrap();
    });
}

#[bench]
fn bench_secp256k1_len_max(b: &mut Bencher) {
    let required_extra_space = 113_u16; // len for pubkey, sig, and offsets
    let feature_set = FeatureSet::all_enabled();
    let txs = create_test_transactions(u16::MAX - required_extra_space);
    let mut tx_iter = txs.iter().cycle();
    b.iter(|| {
        tx_iter
            .next()
            .unwrap()
            .verify_precompiles(&feature_set)
            .unwrap();
    });
}
