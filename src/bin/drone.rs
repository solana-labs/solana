extern crate bincode;
extern crate clap;
extern crate env_logger;
extern crate serde_json;
extern crate solana;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_io;

use bincode::deserialize;
use clap::{App, Arg};
use solana::crdt::NodeInfo;
use solana::drone::{Drone, DroneRequest, DRONE_PORT};
use solana::fullnode::Config;
use solana::metrics::set_panic_hook;
use solana::signature::read_keypair;
use std::fs::File;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio_codec::{BytesCodec, Decoder};

fn main() {
    env_logger::init();
    set_panic_hook("drone");
    let matches = App::new("drone")
        .arg(
            Arg::with_name("leader")
                .short("l")
                .long("leader")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/leader.json"),
        )
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("/path/to/mint.json"),
        )
        .arg(
            Arg::with_name("time")
                .short("t")
                .long("time")
                .value_name("SECONDS")
                .takes_value(true)
                .help("time slice over which to limit requests to drone"),
        )
        .arg(
            Arg::with_name("cap")
                .short("c")
                .long("cap")
                .value_name("NUMBER")
                .takes_value(true)
                .help("request limit for time slice"),
        )
        .get_matches();

    let leader: NodeInfo;
    if let Some(l) = matches.value_of("leader") {
        leader = read_leader(l).node_info;
    } else {
        let server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        leader = NodeInfo::new_leader(&server_addr);
    };

    let mint_keypair =
        read_keypair(matches.value_of("keypair").expect("keypair")).expect("client keypair");

    let time_slice: Option<u64>;
    if let Some(t) = matches.value_of("time") {
        time_slice = Some(t.to_string().parse().expect("integer"));
    } else {
        time_slice = None;
    }
    let request_cap: Option<u64>;
    if let Some(c) = matches.value_of("cap") {
        request_cap = Some(c.to_string().parse().expect("integer"));
    } else {
        request_cap = None;
    }

    let drone_addr: SocketAddr = format!("0.0.0.0:{}", DRONE_PORT).parse().unwrap();

    let drone = Arc::new(Mutex::new(Drone::new(
        mint_keypair,
        drone_addr,
        leader.contact_info.tpu,
        leader.contact_info.rpu,
        time_slice,
        request_cap,
    )));

    let drone1 = drone.clone();
    thread::spawn(move || loop {
        let time = drone1.lock().unwrap().time_slice;
        thread::sleep(time);
        drone1.lock().unwrap().clear_request_count();
    });

    let socket = TcpListener::bind(&drone_addr).unwrap();
    println!("Drone started. Listening on: {}", drone_addr);
    let done = socket
        .incoming()
        .map_err(|e| println!("failed to accept socket; error = {:?}", e))
        .for_each(move |socket| {
            let drone2 = drone.clone();
            // let client_ip = socket.peer_addr().expect("drone peer_addr").ip();
            let framed = BytesCodec::new().framed(socket);
            let (_writer, reader) = framed.split();

            let processor = reader
                .for_each(move |bytes| {
                    let req: DroneRequest =
                        deserialize(&bytes).expect("deserialize packet in drone");
                    println!("Airdrop requested...");
                    // let res = drone2.lock().unwrap().check_rate_limit(client_ip);
                    let res1 = drone2.lock().unwrap().send_airdrop(req);
                    match res1 {
                        Ok(_) => println!("Airdrop sent!"),
                        Err(_) => println!("Request limit reached for this time slice"),
                    }
                    Ok(())
                })
                .and_then(|()| {
                    println!("Socket received FIN packet and closed connection");
                    Ok(())
                })
                .or_else(|err| {
                    println!("Socket closed with error: {:?}", err);
                    Err(err)
                })
                .then(|result| {
                    println!("Socket closed with result: {:?}", result);
                    Ok(())
                });
            tokio::spawn(processor)
        });
    tokio::run(done);
}
fn read_leader(path: &str) -> Config {
    let file = File::open(path).unwrap_or_else(|_| panic!("file not found: {}", path));
    serde_json::from_reader(file).unwrap_or_else(|_| panic!("failed to parse {}", path))
}
