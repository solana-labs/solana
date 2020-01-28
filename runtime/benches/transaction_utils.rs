#![feature(test)]

extern crate test;

use rand::{seq::SliceRandom, thread_rng};
use solana_runtime::transaction_utils::OrderedIterator;
use test::Bencher;

#[bench]
fn bench_ordered_iterator_with_order_shuffling(bencher: &mut Bencher) {
    let vec: Vec<usize> = (0..100_usize).collect();
    bencher.iter(|| {
        let mut order: Vec<usize> = (0..100_usize).collect();
        order.shuffle(&mut thread_rng());
        let _ordered_iterator_resp: Vec<&usize> =
            OrderedIterator::new(&vec, Some(&order)).collect();
    });
}
