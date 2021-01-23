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
    let hashes: Vec<_> = (0..num_accounts)
        .map(|_| (Pubkey::new_unique(), Hash::new_unique(), 1))
        .collect();
    let elapsed: Vec<_> = (0..iterations)
        .map(|_| {
            let hashes = hashes.clone(); // done outside timing
            let mut time = Measure::start("compute_merkle_root_and_capitalization");
            let fanout = 16;
            AccountsDB::compute_merkle_root_and_capitalization(hashes, fanout);
            time.stop();
            time.as_us()
        })
        .collect();

    for result in &elapsed {
        println!("compute_merkle_root_and_capitalization(us),{}", result);
    }
    println!(
        "compute_merkle_root_and_capitalization(us) avg: {}",
        elapsed.into_iter().sum::<u64>() as f64 / iterations as f64
    );
}
