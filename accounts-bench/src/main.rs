use clap::{value_t, App, Arg};
use rayon::prelude::*;
use solana_measure::measure::Measure;
use solana_runtime::{
    accounts::{create_test_accounts, update_accounts, Accounts},
    accounts_index::Ancestors,
};
use solana_sdk::pubkey::Pubkey;
use std::fs;
use std::path::PathBuf;

fn main() {
    solana_logger::setup();

    let matches = App::new("crate")
        .about("about")
        .version("version")
        .arg(
            Arg::with_name("num_slots")
                .long("num_slots")
                .takes_value(true)
                .value_name("SLOTS")
                .help("Number of slots to store to."),
        )
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
        .arg(
            Arg::with_name("clean")
                .long("clean")
                .takes_value(false)
                .help("Run clean"),
        )
        .get_matches();

    let num_slots = value_t!(matches, "num_slots", usize).unwrap_or(4);
    let num_accounts = value_t!(matches, "num_accounts", usize).unwrap_or(10_000);
    let iterations = value_t!(matches, "iterations", usize).unwrap_or(20);
    let clean = matches.is_present("clean");
    println!("clean: {:?}", clean);

    let path = PathBuf::from("farf/accounts-bench");
    if fs::remove_dir_all(path.clone()).is_err() {
        println!("Warning: Couldn't remove {:?}", path);
    }
    let accounts = Accounts::new(vec![path]);
    println!("Creating {} accounts", num_accounts);
    let mut create_time = Measure::start("create accounts");
    let pubkeys: Vec<_> = (0..num_slots)
        .into_par_iter()
        .map(|slot| {
            let mut pubkeys: Vec<Pubkey> = vec![];
            create_test_accounts(
                &accounts,
                &mut pubkeys,
                num_accounts / num_slots,
                slot as u64,
            );
            pubkeys
        })
        .collect();
    let pubkeys: Vec<_> = pubkeys.into_iter().flatten().collect();
    create_time.stop();
    println!(
        "created {} accounts in {} slots {}",
        (num_accounts / num_slots) * num_slots,
        num_slots,
        create_time
    );
    let mut ancestors: Ancestors = vec![(0, 0)].into_iter().collect();
    for i in 1..num_slots {
        ancestors.insert(i as u64, i - 1);
        accounts.add_root(i as u64);
    }
    for x in 0..iterations {
        if clean {
            let mut time = Measure::start("clean");
            accounts.accounts_db.clean_accounts();
            time.stop();
            println!("{}", time);
            for slot in 0..num_slots {
                update_accounts(&accounts, &pubkeys, ((x + 1) * num_slots + slot) as u64);
                accounts.add_root((x * num_slots + slot) as u64);
            }
        } else {
            let mut pubkeys: Vec<Pubkey> = vec![];
            let mut time = Measure::start("hash");
            let hash = accounts.accounts_db.update_accounts_hash(0, &ancestors);
            time.stop();
            println!("hash: {} {}", hash, time);
            create_test_accounts(&accounts, &mut pubkeys, 1, 0);
        }
    }
}
