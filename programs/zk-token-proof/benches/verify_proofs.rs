use {
    criterion::{criterion_group, criterion_main, Criterion},
    solana_zk_token_sdk::{
        encryption::{
            elgamal::ElGamalKeypair,
            grouped_elgamal::GroupedElGamal,
            pedersen::{Pedersen, PedersenOpening},
        },
        instruction::{
            ciphertext_commitment_equality::CiphertextCommitmentEqualityProofData,
            grouped_ciphertext_validity::GroupedCiphertext2HandlesValidityProofData,
            pubkey_validity::PubkeyValidityData, range_proof::RangeProofU64Data,
            withdraw::WithdrawData, zero_balance::ZeroBalanceProofData, ZkProofData,
        },
    },
};

fn bench_pubkey_validity(c: &mut Criterion) {
    let keypair = ElGamalKeypair::new_rand();
    let proof_data = PubkeyValidityData::new(&keypair).unwrap();

    c.bench_function("pubkey_validity", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

fn bench_range_proof_u64(c: &mut Criterion) {
    let amount = std::u64::MAX;
    let (commitment, opening) = Pedersen::new(amount);
    let proof_data = RangeProofU64Data::new(&commitment, amount, &opening).unwrap();

    c.bench_function("range_proof_u64", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

fn bench_withdraw(c: &mut Criterion) {
    let keypair = ElGamalKeypair::new_rand();
    let current_balance: u64 = 77;
    let current_ciphertext = keypair.public.encrypt(current_balance);
    let withdraw_amount: u64 = 55;
    let proof_data = WithdrawData::new(
        withdraw_amount,
        &keypair,
        current_balance,
        &current_ciphertext,
    )
    .unwrap();

    c.bench_function("withdraw", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

fn bench_zero_balance(c: &mut Criterion) {
    let keypair = ElGamalKeypair::new_rand();
    let ciphertext = keypair.public.encrypt(0_u64);
    let proof_data = ZeroBalanceProofData::new(&keypair, &ciphertext).unwrap();

    c.bench_function("zero_balance", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

fn bench_grouped_ciphertext_validity(c: &mut Criterion) {
    let destination_pubkey = ElGamalKeypair::new_rand().public;
    let auditor_pubkey = ElGamalKeypair::new_rand().public;

    let amount: u64 = 55;
    let opening = PedersenOpening::new_rand();
    let grouped_ciphertext =
        GroupedElGamal::encrypt_with([&destination_pubkey, &auditor_pubkey], amount, &opening);

    let proof_data = GroupedCiphertext2HandlesValidityProofData::new(
        &destination_pubkey,
        &auditor_pubkey,
        &grouped_ciphertext,
        amount,
        &opening,
    )
    .unwrap();

    c.bench_function("grouped_ciphertext_validity", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

fn bench_ciphertext_commitment_equality(c: &mut Criterion) {
    let keypair = ElGamalKeypair::new_rand();
    let amount: u64 = 55;
    let ciphertext = keypair.public.encrypt(amount);
    let (commitment, opening) = Pedersen::new(amount);

    let proof_data = CiphertextCommitmentEqualityProofData::new(
        &keypair,
        &ciphertext,
        &commitment,
        &opening,
        amount,
    )
    .unwrap();

    c.bench_function("ciphertext_commitment_equality", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

criterion_group!(
    benches,
    bench_pubkey_validity,
    bench_range_proof_u64,
    bench_withdraw,
    bench_zero_balance,
    bench_grouped_ciphertext_validity,
    bench_ciphertext_commitment_equality,
);
criterion_main!(benches);
