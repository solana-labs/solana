use clap::{crate_description, crate_name, App, Arg};
use solana_clap_utils::input_parsers::{lamports_of_sol, value_of};
use solana_faucet::{
    faucet::{run_faucet, Faucet, FAUCET_PORT},
    socketaddr,
};
use solana_sdk::signature::read_keypair_file;
use std::{
    error,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    thread,
};

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup_with_default("solana=info");
    solana_metrics::set_panic_hook("faucet");
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("File from which to read the mint's keypair"),
        )
        .arg(
            Arg::with_name("slice")
                .long("slice")
                .value_name("SECS")
                .takes_value(true)
                .help("Time slice over which to limit requests to faucet"),
        )
        .arg(
            Arg::with_name("per_time_cap")
                .long("per-time-cap")
                .alias("cap")
                .value_name("NUM")
                .takes_value(true)
                .help("Request limit for time slice, in SOL"),
        )
        .arg(
            Arg::with_name("per_request_cap")
                .long("per-request-cap")
                .value_name("NUM")
                .takes_value(true)
                .help("Request limit for a single request, in SOL"),
        )
        .get_matches();

    let mint_keypair = read_keypair_file(matches.value_of("keypair").unwrap())
        .expect("failed to read client keypair");

    let time_slice = value_of(&matches, "slice");
    let per_time_cap = lamports_of_sol(&matches, "per_time_cap");
    let per_request_cap = lamports_of_sol(&matches, "per_request_cap");

    let faucet_addr = socketaddr!(0, FAUCET_PORT);

    let faucet = Arc::new(Mutex::new(Faucet::new(
        mint_keypair,
        time_slice,
        per_time_cap,
        per_request_cap,
    )));

    let faucet1 = faucet.clone();
    thread::spawn(move || loop {
        let time = faucet1.lock().unwrap().time_slice;
        thread::sleep(time);
        faucet1.lock().unwrap().clear_request_count();
    });

    run_faucet(faucet, faucet_addr, None);
    Ok(())
}
