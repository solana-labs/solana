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

criterion_group!(benches, bench_pubkey_validity);
criterion_main!(benches);
