#![feature(test)]
extern crate test;
use {
    solana_cost_model::cost_model::CostModel,
    solana_sdk::{
        feature_set::FeatureSet,
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        system_instruction,
        transaction::{SanitizedTransaction, Transaction},
    },
    test::Bencher,
};

struct BenchSetup {
    transactions: Vec<SanitizedTransaction>,
    feature_set: FeatureSet,
}

const NUM_TRANSACTIONS_PER_ITER: usize = 1024;

fn setup(num_transactions: usize) -> BenchSetup {
    let transactions = (0..num_transactions)
        .map(|_| {
            // As many transfer instructions as is possible in a regular packet.
            let from_keypair = Keypair::new();
            let to_lamports =
                Vec::from_iter(std::iter::repeat_with(|| (Pubkey::new_unique(), 1)).take(24));
            let ixs = system_instruction::transfer_many(&from_keypair.pubkey(), &to_lamports);
            let message = Message::new(&ixs, Some(&from_keypair.pubkey()));
            let transaction = Transaction::new(&[from_keypair], message, Hash::default());
            SanitizedTransaction::from_transaction_for_tests(transaction)
        })
        .collect();

    let feature_set = FeatureSet::default();

    BenchSetup {
        transactions,
        feature_set,
    }
}

#[bench]
fn bench_cost_model(bencher: &mut Bencher) {
    let BenchSetup {
        transactions,
        feature_set,
    } = setup(NUM_TRANSACTIONS_PER_ITER);

    bencher.iter(|| {
        for transaction in &transactions {
            let _ = CostModel::calculate_cost(test::black_box(transaction), &feature_set);
        }
    });
}

#[bench]
fn bench_cost_model_requested_write_locks(bencher: &mut Bencher) {
    let BenchSetup {
        transactions,
        mut feature_set,
    } = setup(NUM_TRANSACTIONS_PER_ITER);
    feature_set.activate(
        &solana_sdk::feature_set::cost_model_requested_write_lock_cost::id(),
        0,
    );

    bencher.iter(|| {
        for transaction in &transactions {
            let _ = CostModel::calculate_cost(test::black_box(transaction), &feature_set);
        }
    });
}
