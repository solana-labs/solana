#![feature(test)]
extern crate test;
use {
    itertools::Itertools,
    solana_cost_model::{
        cost_tracker::CostTracker,
        transaction_cost::{TransactionCost, UsageCostDetails},
    },
    solana_sdk::pubkey::Pubkey,
    test::Bencher,
};

struct BenchSetup {
    cost_tracker: CostTracker,
    tx_costs: Vec<TransactionCost>,
}

fn setup(num_transactions: usize, contentious_transactions: bool) -> BenchSetup {
    let mut cost_tracker = CostTracker::default();
    // set cost_tracker with max limits to stretch testing
    cost_tracker.set_limits(u64::MAX, u64::MAX, u64::MAX);

    let max_accounts_per_tx = 128;
    let pubkey = Pubkey::new_unique();
    let tx_costs = (0..num_transactions)
        .map(|_| {
            let mut usage_cost_details = UsageCostDetails::default();
            (0..max_accounts_per_tx).for_each(|_| {
                let writable_account_key = if contentious_transactions {
                    pubkey
                } else {
                    Pubkey::new_unique()
                };
                usage_cost_details
                    .writable_accounts
                    .push(writable_account_key)
            });
            usage_cost_details.programs_execution_cost = 9999;
            TransactionCost::Transaction(usage_cost_details)
        })
        .collect_vec();

    BenchSetup {
        cost_tracker,
        tx_costs,
    }
}

#[bench]
fn bench_cost_tracker_non_contentious_transaction(bencher: &mut Bencher) {
    let BenchSetup {
        mut cost_tracker,
        tx_costs,
    } = setup(1024, false);

    bencher.iter(|| {
        for tx_cost in tx_costs.iter() {
            if cost_tracker.try_add(tx_cost).is_err() {
                break;
            } // stop when hit limits
            cost_tracker.update_execution_cost(tx_cost, 0); // update execution cost down to zero
        }
    });
}

#[bench]
fn bench_cost_tracker_contentious_transaction(bencher: &mut Bencher) {
    let BenchSetup {
        mut cost_tracker,
        tx_costs,
    } = setup(1024, true);

    bencher.iter(|| {
        for tx_cost in tx_costs.iter() {
            if cost_tracker.try_add(tx_cost).is_err() {
                break;
            } // stop when hit limits
            cost_tracker.update_execution_cost(tx_cost, 0); // update execution cost down to zero
        }
    });
}
