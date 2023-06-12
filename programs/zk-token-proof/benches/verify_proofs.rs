use {
    criterion::{criterion_group, criterion_main, Criterion},
    solana_zk_token_sdk::{
        encryption::{
            elgamal::ElGamalKeypair,
            grouped_elgamal::GroupedElGamal,
            pedersen::{Pedersen, PedersenOpening},
        },
        instruction::{
            batched_grouped_ciphertext_validity::BatchedGroupedCiphertext2HandlesValidityProofData,
            ciphertext_ciphertext_equality::CiphertextCiphertextEqualityProofData,
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

fn bench_ciphertext_ciphertext_equality(c: &mut Criterion) {
    let source_keypair = ElGamalKeypair::new_rand();
    let destination_keypair = ElGamalKeypair::new_rand();

    let amount: u64 = 0;
    let source_ciphertext = source_keypair.public.encrypt(amount);

    let destination_opening = PedersenOpening::new_rand();
    let destination_ciphertext = destination_keypair
        .public
        .encrypt_with(amount, &destination_opening);

    let proof_data = CiphertextCiphertextEqualityProofData::new(
        &source_keypair,
        &destination_keypair.public,
        &source_ciphertext,
        &destination_ciphertext,
        &destination_opening,
        amount,
    )
    .unwrap();

    c.bench_function("ciphertext_ciphertext_equality", |b| {
        b.iter(|| {
            proof_data.verify_proof().unwrap();
        })
    });
}

fn bench_batched_grouped_ciphertext_validity(c: &mut Criterion) {
    let destination_pubkey = ElGamalKeypair::new_rand().public;
    let auditor_pubkey = ElGamalKeypair::new_rand().public;

    let amount_lo: u64 = 11;
    let amount_hi: u64 = 22;

    let opening_lo = PedersenOpening::new_rand();
    let opening_hi = PedersenOpening::new_rand();

    let grouped_ciphertext_lo = GroupedElGamal::encrypt_with(
        [&destination_pubkey, &auditor_pubkey],
        amount_lo,
        &opening_lo,
    );

    let grouped_ciphertext_hi = GroupedElGamal::encrypt_with(
        [&destination_pubkey, &auditor_pubkey],
        amount_hi,
        &opening_hi,
    );

    let proof_data = BatchedGroupedCiphertext2HandlesValidityProofData::new(
        &destination_pubkey,
        &auditor_pubkey,
        &grouped_ciphertext_lo,
        &grouped_ciphertext_hi,
        amount_lo,
        amount_hi,
        &opening_lo,
        &opening_hi,
    )
    .unwrap();

    c.bench_function("batched_grouped_ciphertext_validity", |b| {
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
    bench_ciphertext_ciphertext_equality,
    bench_batched_grouped_ciphertext_validity,
);
criterion_main!(benches);
