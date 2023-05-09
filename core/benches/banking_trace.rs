#![feature(test)]

extern crate test;

use {
    solana_core::banking_trace::{
        for_test::{
            drop_and_clean_temp_dir_unless_suppressed, sample_packet_batch, terminate_tracer,
        },
        receiving_loop_with_minimized_sender_overhead, BankingPacketBatch, BankingTracer,
        TraceError, TracerThreadResult, BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
    },
    std::{
        path::PathBuf,
        sync::{atomic::AtomicBool, Arc},
        thread,
    },
    tempfile::TempDir,
    test::Bencher,
};

fn ensure_fresh_setup_to_benchmark(path: &PathBuf) {
    // make sure fresh setup; otherwise banking tracer appends and rotates
    // trace files created by prior bench iterations, slightly skewing perf
    // result...
    BankingTracer::ensure_cleanup_path(path).unwrap();
}

fn black_box_packet_batch(packet_batch: BankingPacketBatch) -> TracerThreadResult {
    test::black_box(packet_batch);
    Ok(())
}

#[bench]
fn bench_banking_tracer_main_thread_overhead_noop_baseline(bencher: &mut Bencher) {
    let exit = Arc::<AtomicBool>::default();
    let tracer = BankingTracer::new_disabled();
    let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

    let exit_for_dummy_thread = exit.clone();
    let dummy_main_thread = thread::spawn(move || {
        receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
            exit_for_dummy_thread,
            non_vote_receiver,
            black_box_packet_batch,
        )
    });

    let packet_batch = sample_packet_batch();
    bencher.iter(|| {
        non_vote_sender.send(packet_batch.clone()).unwrap();
    });
    terminate_tracer(tracer, None, dummy_main_thread, non_vote_sender, Some(exit));
}

#[bench]
fn bench_banking_tracer_main_thread_overhead_under_peak_write(bencher: &mut Bencher) {
    let temp_dir = TempDir::new().unwrap();

    let exit = Arc::<AtomicBool>::default();
    let (tracer, tracer_thread) = BankingTracer::new(Some((
        &temp_dir.path().join("banking-trace"),
        exit.clone(),
        BANKING_TRACE_DIR_DEFAULT_BYTE_LIMIT,
    )))
    .unwrap();
    let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

    let exit_for_dummy_thread = exit.clone();
    let dummy_main_thread = thread::spawn(move || {
        receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
            exit_for_dummy_thread,
            non_vote_receiver,
            black_box_packet_batch,
        )
    });

    let packet_batch = sample_packet_batch();
    bencher.iter(|| {
        non_vote_sender.send(packet_batch.clone()).unwrap();
    });

    terminate_tracer(
        tracer,
        tracer_thread,
        dummy_main_thread,
        non_vote_sender,
        Some(exit),
    );
    drop_and_clean_temp_dir_unless_suppressed(temp_dir);
}

#[bench]
fn bench_banking_tracer_main_thread_overhead_under_sustained_write(bencher: &mut Bencher) {
    let temp_dir = TempDir::new().unwrap();

    let exit = Arc::<AtomicBool>::default();
    let (tracer, tracer_thread) = BankingTracer::new(Some((
        &temp_dir.path().join("banking-trace"),
        exit.clone(),
        1024 * 1024, // cause more frequent trace file rotation
    )))
    .unwrap();
    let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

    let exit_for_dummy_thread = exit.clone();
    let dummy_main_thread = thread::spawn(move || {
        receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
            exit_for_dummy_thread,
            non_vote_receiver,
            black_box_packet_batch,
        )
    });

    let packet_batch = sample_packet_batch();
    bencher.iter(|| {
        non_vote_sender.send(packet_batch.clone()).unwrap();
    });

    terminate_tracer(
        tracer,
        tracer_thread,
        dummy_main_thread,
        non_vote_sender,
        Some(exit),
    );
    drop_and_clean_temp_dir_unless_suppressed(temp_dir);
}

#[bench]
fn bench_banking_tracer_background_thread_throughput(bencher: &mut Bencher) {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path();

    let packet_batch = sample_packet_batch();

    bencher.iter(move || {
        let path = base_path.join("banking-trace");
        ensure_fresh_setup_to_benchmark(&path);

        let exit = Arc::<AtomicBool>::default();

        let (tracer, tracer_thread) =
            BankingTracer::new(Some((&path, exit.clone(), 50 * 1024 * 1024))).unwrap();
        let (non_vote_sender, non_vote_receiver) = tracer.create_channel_non_vote();

        let dummy_main_thread = thread::spawn(move || {
            receiving_loop_with_minimized_sender_overhead::<_, TraceError, 0>(
                exit.clone(),
                non_vote_receiver,
                black_box_packet_batch,
            )
        });

        for _ in 0..1000 {
            non_vote_sender.send(packet_batch.clone()).unwrap();
        }

        terminate_tracer(
            tracer,
            tracer_thread,
            dummy_main_thread,
            non_vote_sender,
            None,
        );
    });

    drop_and_clean_temp_dir_unless_suppressed(temp_dir);
}
