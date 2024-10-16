#![feature(test)]
extern crate test;
use {
    itertools::Itertools,
    solana_cost_model::{
        cost_tracker::CostTracker,
        transaction_cost::{TransactionCost, UsageCostDetails, WritableKeysTransaction},
    },
    solana_sdk::{message::TransactionSignatureDetails, pubkey::Pubkey},
    test::Bencher,
};

struct BenchSetup {
    cost_tracker: CostTracker,
    transactions: Vec<WritableKeysTransaction>,
}

fn setup(num_transactions: usize, contentious_transactions: bool) -> BenchSetup {
    let mut cost_tracker = CostTracker::default();
    // set cost_tracker with max limits to stretch testing
    cost_tracker.set_limits(u64::MAX, u64::MAX, u64::MAX);

    let max_accounts_per_tx = 128;
    let pubkey = Pubkey::new_unique();
    let transactions = (0..num_transactions)
        .map(|_| {
            let mut writable_accounts = Vec::with_capacity(max_accounts_per_tx);
            (0..max_accounts_per_tx).for_each(|_| {
                let writable_account_key = if contentious_transactions {
                    pubkey
                } else {
                    Pubkey::new_unique()
                };
                writable_accounts.push(writable_account_key)
            });
            WritableKeysTransaction(writable_accounts)
        })
        .collect_vec();

    BenchSetup {
        cost_tracker,
        transactions,
    }
}

fn get_costs(
    transactions: &[WritableKeysTransaction],
) -> Vec<TransactionCost<WritableKeysTransaction>> {
    transactions
        .iter()
        .map(|transaction| {
            TransactionCost::Transaction(UsageCostDetails {
                transaction,
                signature_cost: 0,
                write_lock_cost: 0,
                data_bytes_cost: 0,
                programs_execution_cost: 9999,
                loaded_accounts_data_size_cost: 0,
                allocated_accounts_data_size: 0,
                signature_details: TransactionSignatureDetails::new(0, 0, 0),
            })
        })
        .collect_vec()
}

#[bench]
fn bench_cost_tracker_non_contentious_transaction(bencher: &mut Bencher) {
    let BenchSetup {
        mut cost_tracker,
        transactions,
    } = setup(1024, false);
    let tx_costs = get_costs(&transactions);

    bencher.iter(|| {
        for tx_cost in tx_costs.iter() {
            if cost_tracker.try_add(tx_cost).is_err() {
                break;
            } // stop when hit limits
            cost_tracker.update_execution_cost(tx_cost, 0, 0); // update execution cost down to zero
        }
    });
}

#[bench]
fn bench_cost_tracker_contentious_transaction(bencher: &mut Bencher) {
    let BenchSetup {
        mut cost_tracker,
        transactions,
    } = setup(1024, true);
    let tx_costs = get_costs(&transactions);

    bencher.iter(|| {
        for tx_cost in tx_costs.iter() {
            if cost_tracker.try_add(tx_cost).is_err() {
                break;
            } // stop when hit limits
            cost_tracker.update_execution_cost(tx_cost, 0, 0); // update execution cost down to zero
        }
    });
}
