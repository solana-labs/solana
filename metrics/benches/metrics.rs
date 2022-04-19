#![feature(test)]

extern crate test;

use {
    log::*,
    rand::distributions::{Distribution, Uniform},
    solana_metrics::{
        counter::CounterPoint,
        datapoint::DataPoint,
        metrics::{test_mocks::MockMetricsWriter, MetricsAgent},
    },
    std::{sync::Arc, time::Duration},
    test::Bencher,
};

#[bench]
fn bench_datapoint_submission(bencher: &mut Bencher) {
    let writer = Arc::new(MockMetricsWriter::new());
    let agent = MetricsAgent::new(writer, Duration::from_secs(10), 1000);

    bencher.iter(|| {
        for i in 0..1000 {
            agent.submit(
                DataPoint::new("measurement")
                    .add_field_i64("i", i)
                    .to_owned(),
                Level::Info,
            );
        }
        agent.flush();
    })
}

#[bench]
fn bench_counter_submission(bencher: &mut Bencher) {
    let writer = Arc::new(MockMetricsWriter::new());
    let agent = MetricsAgent::new(writer, Duration::from_secs(10), 1000);

    bencher.iter(|| {
        for i in 0..1000 {
            agent.submit_counter(CounterPoint::new("counter 1"), Level::Info, i);
        }
        agent.flush();
    })
}

#[bench]
fn bench_random_submission(bencher: &mut Bencher) {
    let writer = Arc::new(MockMetricsWriter::new());
    let agent = MetricsAgent::new(writer, Duration::from_secs(10), 1000);
    let mut rng = rand::thread_rng();
    let die = Uniform::<i32>::from(1..7);

    bencher.iter(|| {
        for i in 0..1000 {
            let dice = die.sample(&mut rng);

            if dice == 6 {
                agent.submit_counter(CounterPoint::new("counter 1"), Level::Info, i);
            } else {
                agent.submit(
                    DataPoint::new("measurement")
                        .add_field_i64("i", i as i64)
                        .to_owned(),
                    Level::Info,
                );
            }
        }
        agent.flush();
    })
}
