#![feature(test)]

extern crate test;
use {
    rand0_7::thread_rng,
    solana_sdk::{
        feature_set::FeatureSet,
        hash::Hash,
        secp256k1_instruction::new_secp256k1_instruction,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
    test::Bencher,
};

fn create_test_transaction(message_length: u16) -> Transaction {
    let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
    let message_arr = vec![1u8; message_length as usize];
    let secp_instruction = new_secp256k1_instruction(&secp_privkey, &message_arr);
    let mint_keypair = Keypair::new();

    Transaction::new_signed_with_payer(
        &[secp_instruction.clone()],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        Hash::default(),
    )
}

#[bench]
fn bench_secp256k1_len_032(b: &mut Bencher) {
    let tx = create_test_transaction(32);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}

#[bench]
fn bench_secp256k1_len_256(b: &mut Bencher) {
    let tx = create_test_transaction(256);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}

#[bench]
fn bench_secp256k1_len_32k(b: &mut Bencher) {
    let tx = create_test_transaction(32 * 1024);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}

#[bench]
fn bench_secp256k1_len_max(b: &mut Bencher) {
    let required_extra_space = 113_u16; // len for pubkey, sig, and offsets
    let tx = create_test_transaction(u16::MAX - required_extra_space);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}
