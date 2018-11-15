extern crate bincode;
extern crate byteorder;
extern crate bytes;
#[macro_use]
extern crate clap;
extern crate log;
extern crate serde_json;
extern crate solana;
extern crate tokio;
extern crate tokio_codec;

use bincode::{deserialize, serialize};
use byteorder::{ByteOrder, LittleEndian};
use bytes::Bytes;
use clap::{App, Arg};
use solana::drone::{Drone, DroneRequest, DRONE_PORT};
use solana::logger;
use solana::metrics::set_panic_hook;
use solana::signature::read_keypair;
use std::error;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio_codec::{BytesCodec, Decoder};

macro_rules! socketaddr {
    ($ip:expr, $port:expr) => {
        SocketAddr::from((Ipv4Addr::from($ip), $port))
    };
    ($str:expr) => {{
        let a: SocketAddr = $str.parse().unwrap();
        a
    }};
}

fn main() -> Result<(), Box<error::Error>> {
    logger::setup();
    set_panic_hook("drone");
    let matches = App::new("drone")
        .version(crate_version!())
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("File from which to read the mint's keypair"),
        ).arg(
            Arg::with_name("slice")
                .long("slice")
                .value_name("SECS")
                .takes_value(true)
                .help("Time slice over which to limit requests to drone"),
        ).arg(
            Arg::with_name("cap")
                .long("cap")
                .value_name("NUM")
                .takes_value(true)
                .help("Request limit for time slice"),
        ).get_matches();

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

    let socket = TcpListener::bind(&drone_addr).unwrap();
    println!("Drone started. Listening on: {}", drone_addr);
    let done = socket
        .incoming()
        .map_err(|e| println!("failed to accept socket; error = {:?}", e))
        .for_each(move |socket| {
            let drone2 = drone.clone();
            // let client_ip = socket.peer_addr().expect("drone peer_addr").ip();
            let framed = BytesCodec::new().framed(socket);
            let (writer, reader) = framed.split();

            let processor = reader.and_then(move |bytes| {
                let req: DroneRequest = deserialize(&bytes).or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("deserialize packet in drone: {:?}", err),
                    ))
                })?;

                println!("Airdrop transaction requested...{:?}", req);
                // let res = drone2.lock().unwrap().check_rate_limit(client_ip);
                let res = drone2.lock().unwrap().build_airdrop_transaction(req);
                match res {
                    Ok(tx) => {
                        let response_vec = serialize(&tx).or_else(|err| {
                            Err(io::Error::new(
                                io::ErrorKind::Other,
                                format!("deserialize packet in drone: {:?}", err),
                            ))
                        })?;

                        let mut response_vec_with_length = vec![0; 2];
                        LittleEndian::write_u16(
                            &mut response_vec_with_length,
                            response_vec.len() as u16,
                        );
                        response_vec_with_length.extend_from_slice(&response_vec);

                        let response_bytes = Bytes::from(response_vec_with_length);
                        println!("Airdrop transaction granted");
                        Ok(response_bytes)
                    }
                    Err(err) => {
                        println!("Airdrop transaction failed: {:?}", err);
                        Err(err)
                    }
                }
            });
            let server = writer
                .send_all(processor.or_else(|err| {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Drone response: {:?}", err),
                    ))
                })).then(|_| Ok(()));
            tokio::spawn(server)
        });
    tokio::run(done);
    Ok(())
}
