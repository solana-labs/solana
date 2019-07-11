#![feature(test)]

extern crate test;

use test::Bencher;
use solana_runtime::transaction_utils::OrderedIterator;
use rand::seq::SliceRandom;
use rand::thread_rng;

#[bench]
fn bench_ordered_iterator_with_order_shuffling(bencher: &mut Bencher) {
    let vec: Vec<usize> = (0..100_usize).collect();
    bencher.iter(|| {
        let mut order: Vec<usize> = (0..100_usize).collect();
        order.shuffle(&mut thread_rng());
        let ordered_iterator_resp: Vec<&usize> = OrderedIterator::new(&vec, Some(&order)).collect();
    });
}