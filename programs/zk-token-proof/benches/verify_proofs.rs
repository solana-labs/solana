use {
    criterion::{criterion_group, criterion_main, Criterion},
    solana_zk_token_sdk::{
        encryption::{elgamal::ElGamalKeypair, pedersen::Pedersen},
        instruction::{
            pubkey_validity::PubkeyValidityData, range_proof::RangeProofU64Data, ZkProofData,
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

criterion_group!(benches, bench_pubkey_validity, bench_range_proof_u64);
criterion_main!(benches);
