#![allow(clippy::arithmetic_side_effects)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use solana_entry::entry::{self, create_ticks, init_poh, EntrySlice, VerifyRecyclers};
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
use solana_entry::entry::{create_ticks, init_poh, EntrySlice, VerifyRecyclers};
use {
    clap::{crate_description, crate_name, Arg, Command},
    solana_measure::measure::Measure,
    solana_perf::perf_libs,
    solana_sdk::hash::hash,
};

fn main() {
    solana_logger::setup();

    let matches = Command::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::new("max_num_entries")
                .long("max-num-entries")
                .takes_value(true)
                .value_name("SIZE")
                .help("Number of entries."),
        )
        .arg(
            Arg::new("start_num_entries")
                .long("start-num-entries")
                .takes_value(true)
                .value_name("SIZE")
                .help("Packets per chunk"),
        )
        .arg(
            Arg::new("hashes_per_tick")
                .long("hashes-per-tick")
                .takes_value(true)
                .value_name("SIZE")
                .help("hashes per tick"),
        )
        .arg(
            Arg::new("num_transactions_per_entry")
                .long("num-transactions-per-entry")
                .takes_value(true)
                .value_name("NUM")
                .help("Skip transaction sanity execution"),
        )
        .arg(
            Arg::new("iterations")
                .long("iterations")
                .takes_value(true)
                .help("Number of iterations"),
        )
        .arg(
            Arg::new("num_threads")
                .long("num-threads")
                .takes_value(true)
                .help("Number of threads"),
        )
        .arg(
            Arg::new("cuda")
                .long("cuda")
                .takes_value(false)
                .help("Use cuda"),
        )
        .get_matches();

    let max_num_entries: u64 = matches.value_of_t("max_num_entries").unwrap_or(64);
    let start_num_entries: u64 = matches
        .value_of_t("start_num_entries")
        .unwrap_or(max_num_entries);
    let iterations: usize = matches.value_of_t("iterations").unwrap_or(10);
    let hashes_per_tick: u64 = matches.value_of_t("hashes_per_tick").unwrap_or(10_000);
    let start_hash = hash(&[1, 2, 3, 4]);
    let ticks = create_ticks(max_num_entries, hashes_per_tick, start_hash);
    let mut num_entries = start_num_entries as usize;
    if matches.is_present("cuda") {
        perf_libs::init_cuda();
    }
    init_poh();
    while num_entries <= max_num_entries as usize {
        let mut time = Measure::start("time");
        for _ in 0..iterations {
            assert!(ticks[..num_entries]
                .verify_cpu_generic(&start_hash)
                .finish_verify());
        }
        time.stop();
        println!(
            "{},cpu_generic,{}",
            num_entries,
            time.as_us() / iterations as u64
        );

        // A target_arch check is required here since calling
        // is_x86_feature_detected from a non-x86_64 arch results in a build
        // error.
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if is_x86_feature_detected!("avx2") && entry::api().is_some() {
                let mut time = Measure::start("time");
                for _ in 0..iterations {
                    assert!(ticks[..num_entries]
                        .verify_cpu_x86_simd(&start_hash, 8)
                        .finish_verify());
                }
                time.stop();
                println!(
                    "{},cpu_simd_avx2,{}",
                    num_entries,
                    time.as_us() / iterations as u64
                );
            }

            if is_x86_feature_detected!("avx512f") && entry::api().is_some() {
                let mut time = Measure::start("time");
                for _ in 0..iterations {
                    assert!(ticks[..num_entries]
                        .verify_cpu_x86_simd(&start_hash, 16)
                        .finish_verify());
                }
                time.stop();
                println!(
                    "{},cpu_simd_avx512,{}",
                    num_entries,
                    time.as_us() / iterations as u64
                );
            }
        }

        if perf_libs::api().is_some() {
            let mut time = Measure::start("time");
            let recyclers = VerifyRecyclers::default();
            for _ in 0..iterations {
                assert!(ticks[..num_entries]
                    .start_verify(&start_hash, recyclers.clone())
                    .finish_verify());
            }
            time.stop();
            println!(
                "{},gpu_cuda,{}",
                num_entries,
                time.as_us() / iterations as u64
            );
        }

        println!();
        num_entries *= 2;
    }
}
