use clap::{crate_description, crate_name, crate_version, App, Arg};
use solana_drone::drone::{run_drone, Drone, DRONE_PORT};
use solana_drone::socketaddr;
use solana_sdk::signature::read_keypair;
use std::error;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread;

fn main() -> Result<(), Box<error::Error>> {
    solana_logger::setup_with_filter("solana=info");
    solana_metrics::set_panic_hook("drone");
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
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
                .help("Time slice over which to limit requests to drone"),
        )
        .arg(
            Arg::with_name("cap")
                .long("cap")
                .value_name("NUM")
                .takes_value(true)
                .help("Request limit for time slice"),
        )
        .get_matches();

    let mint_keypair =
        read_keypair(matches.value_of("keypair").unwrap()).expect("failed to read client keypair");

    let time_slice: Option<u64>;
    if let Some(secs) = matches.value_of("slice") {
        time_slice = Some(secs.to_string().parse().expect("failed to parse slice"));
    } else {
        time_slice = None;
    }
    let request_cap: Option<u64>;
    if let Some(c) = matches.value_of("cap") {
        request_cap = Some(c.to_string().parse().expect("failed to parse cap"));
    } else {
        request_cap = None;
    }

    let drone_addr = socketaddr!(0, DRONE_PORT);

    let drone = Arc::new(Mutex::new(Drone::new(
        mint_keypair,
        time_slice,
        request_cap,
    )));

    let drone1 = drone.clone();
    thread::spawn(move || loop {
        let time = drone1.lock().unwrap().time_slice;
        thread::sleep(time);
        drone1.lock().unwrap().clear_request_count();
    });

    run_drone(drone, drone_addr, None);
    Ok(())
}
