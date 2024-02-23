#![feature(test)]

extern crate test;
use {
    rand0_7::thread_rng,
    solana_sdk::{
        ed25519_instruction::new_ed25519_instruction,
        feature_set::FeatureSet,
        hash::Hash,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
    test::Bencher,
};

fn create_test_transaction(message_length: u16) -> Transaction {
    let privkey = ed25519_dalek::Keypair::generate(&mut thread_rng());
    let message_arr = vec![1u8; message_length as usize];
    let instruction = new_ed25519_instruction(&privkey, &message_arr);
    let mint_keypair = Keypair::new();

    Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&mint_keypair.pubkey()),
        &[&mint_keypair],
        Hash::default(),
    )
}

#[bench]
fn bench_ed25519_len_032(b: &mut Bencher) {
    let tx = create_test_transaction(32);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}

#[bench]
fn bench_ed25519_len_128(b: &mut Bencher) {
    let tx = create_test_transaction(128);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}

#[bench]
fn bench_ed25519_len_32k(b: &mut Bencher) {
    let tx = create_test_transaction(32 * 1024);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}

#[bench]
fn bench_ed25519_len_max(b: &mut Bencher) {
    let required_extra_space = 113_u16; // len for pubkey, sig, and offsets
    let tx = create_test_transaction(u16::MAX - required_extra_space);
    let feature_set = FeatureSet::all_enabled();

    b.iter(|| {
        tx.verify_precompiles(&feature_set).unwrap();
    });
}
