#![allow(clippy::integer_arithmetic)]
#![feature(test)]

extern crate test;

use {
    log::*,
    rand::prelude::*,
    solana_perf::{
        data_budget::DataBudget,
        packet::{to_packet_batches, PacketBatch},
        qos,
        test_tx::test_tx,
    },
    std::net::{IpAddr, Ipv4Addr, SocketAddr},
    test::Bencher,
};

fn do_bench_qos_packets(bencher: &mut Bencher, mut batches: Vec<PacketBatch>) {
    let mut ip_to_budget = qos::StakeMap::default();

    let mut total_stake = 0;
    let num_senders = 2000;
    for _ in 0..num_senders {
        let stake = thread_rng().gen_range(10, 1000);
        total_stake += stake;
        let from_a = thread_rng().gen_range(0, 255);
        let from_b = thread_rng().gen_range(0, 255);
        let from_c = thread_rng().gen_range(0, 255);
        let ip = IpAddr::V4(Ipv4Addr::new(127, from_a, from_b, from_c));
        ip_to_budget.insert(ip, (stake, DataBudget::default()));
    }
    info!("staked: {}", ip_to_budget.len());
    let max_packets_per_second = 1000;

    batches.iter_mut().for_each(|batch| {
        batch.packets.iter_mut().for_each(|p| {
            if thread_rng().gen_ratio(1, 10) {
                let sender_index = thread_rng().gen_range(0, num_senders);
                let sender = ip_to_budget.keys().nth(sender_index).unwrap();
                let socket = SocketAddr::new(*sender, 0);
                p.meta.set_addr(&socket);
            }
        })
    });

    let qos = qos::Qos::new(1024, max_packets_per_second);

    // verify packets
    bencher.iter(|| {
        // bench
        let _discarded = qos::qos_packets(&qos, total_stake, &ip_to_budget, &mut batches);
        batches.iter_mut().for_each(|batch| {
            batch
                .packets
                .iter_mut()
                .for_each(|p| p.meta.set_discard(false))
        });
    })
}

const NUM: usize = 10 * 1024;

#[bench]
#[ignore]
fn bench_qos_same_batch(bencher: &mut Bencher) {
    solana_logger::setup();
    let batches = to_packet_batches(&(0..NUM).map(|_| test_tx()).collect::<Vec<_>>(), 128);

    do_bench_qos_packets(bencher, batches);
}
