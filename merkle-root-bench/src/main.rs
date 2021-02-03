extern crate log;
use clap::{crate_description, crate_name, value_t, App, Arg};
use solana_measure::measure::Measure;
use solana_runtime::accounts_db::AccountsDB;
use solana_sdk::{hash::Hash, pubkey::Pubkey};

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("num_accounts")
                .long("num_accounts")
                .takes_value(true)
                .value_name("NUM_ACCOUNTS")
                .help("Total number of accounts"),
        )
        .arg(
            Arg::with_name("iterations")
                .long("iterations")
                .takes_value(true)
                .value_name("ITERATIONS")
                .help("Number of bench iterations"),
        )
        .get_matches();

    let num_accounts = value_t!(matches, "num_accounts", usize).unwrap_or(10_000);
    let iterations = value_t!(matches, "iterations", usize).unwrap_or(20);
    let mut elapsed: Vec<u64> = vec![0; iterations];
    let mut elapsed_legacy: Vec<u64> = vec![0; iterations];
    let hashes: Vec<_> = (0..num_accounts)
        .map(|_| (Pubkey::new_unique(), Hash::new_unique(), 1))
        .collect();
    for x in 0..iterations {
        let hashes = (hashes.clone(), hashes.clone()); // done outside timing
        let mut time = Measure::start("compute_merkle_root_and_capitalization");
        let fanout = 16;
        let results = AccountsDB::compute_merkle_root_and_capitalization(hashes.0, fanout);
        time.stop();
        let mut time_legacy = Measure::start("hash");
        let results_hash = AccountsDB::compute_merkle_root_legacy(hashes.1, fanout);
        time_legacy.stop();
        assert_eq!(results_hash, results.0);
        elapsed[x] = time.as_us();
        elapsed_legacy[x] = time_legacy.as_us();
    }

    let len = elapsed.len();
    for x in 0..iterations {
        println!(
            "compute_merkle_root_and_capitalization(us),{},legacy(us),{}",
            elapsed[x], elapsed_legacy[x]
        );
    }
    println!(
        "compute_merkle_root_and_capitalization(us) avg: {}",
        elapsed.into_iter().sum::<u64>() as f64 / len as f64
    );
    println!(
        "compute_merkle_root_legacy(us) avg: {}",
        elapsed_legacy.into_iter().sum::<u64>() as f64 / len as f64
    );
}
