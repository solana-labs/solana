use clap::{value_t, value_t_or_exit, App, Arg};
use log::*;
use rand::{thread_rng, Rng};
use solana_core::gossip_service::discover_cluster;
use std::net::{SocketAddr, UdpSocket};
use std::process::exit;
use std::time::Instant;

fn main() {
    solana_logger::setup();
    let matches = App::new("crate")
        .about("about")
        .version("version")
        .arg(
            Arg::with_name("entrypoint")
                .long("entrypoint")
                .takes_value(true)
                .value_name("HOST:PORT")
                .help("Gossip entrypoint address. Usually <ip>:8001"),
        )
        .arg(
            Arg::with_name("num_nodes")
                .long("num_nodes")
                .takes_value(true)
                .value_name("NUM")
                .help("Number of gossip nodes to look for."),
        )
        .arg(
            Arg::with_name("mode")
                .long("mode")
                .takes_value(true)
                .value_name("DOS_MODE")
                .help(
                    "Interface to dos.\n\
                       Valid values: gossip, tvu, tvu_forwards, tpu,\n\
                       tpu_forwards, repair, serve_repair",
                ),
        )
        .arg(
            Arg::with_name("data_size")
                .long("data_size")
                .takes_value(true)
                .value_name("SIZE_BYTES")
                .help("Size of packet to dos with."),
        )
        .arg(
            Arg::with_name("random_data")
                .long("random_data")
                .takes_value(false)
                .help("Use random data for each iteration."),
        )
        .get_matches();

    let mut entrypoint_addr = SocketAddr::from(([127, 0, 0, 1], 8001));
    if let Some(addr) = matches.value_of("entrypoint") {
        entrypoint_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1)
        });
    }
    let num_nodes = value_t!(matches, "num_nodes", usize).unwrap_or(1);
    let data_size = value_t!(matches, "data_size", usize).unwrap_or(128);
    let random_data = matches.is_present("random_data");

    let mode = value_t_or_exit!(matches, "mode", String);

    info!("Finding cluster entry: {:?}", entrypoint_addr);
    let (nodes, _archivers) = discover_cluster(&entrypoint_addr, num_nodes).unwrap_or_else(|err| {
        eprintln!("Failed to discover {} nodes: {:?}", num_nodes, err);
        exit(1);
    });

    info!("done found {} nodes", nodes.len());

    let mut target = None;
    for node in &nodes {
        if node.gossip == entrypoint_addr {
            target = match mode.as_str() {
                "gossip" => Some(node.gossip),
                "tvu" => Some(node.tvu),
                "tvu_forwards" => Some(node.tvu_forwards),
                "tpu" => Some(node.tpu),
                "tpu_forwards" => Some(node.tpu_forwards),
                "repair" => Some(node.repair),
                "serve_repair" => Some(node.serve_repair),
                &_ => panic!("Unknown mode"),
            };
            break;
        }
    }
    let target = target.expect("should have target");

    info!("Targetting {}", target);
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let mut data = vec![0u8; data_size];
    thread_rng().fill(&mut data[..]);
    let mut last_log = Instant::now();
    let mut count = 0;
    let mut error_count = 0;
    loop {
        if random_data {
            thread_rng().fill(&mut data[..]);
        }
        let res = socket.send_to(&data, target);
        count += 1;
        if res.is_err() {
            error_count += 1;
        }
        if last_log.elapsed().as_secs() > 5 {
            info!("count: {} errors: {}", count, error_count);
            last_log = Instant::now();
            count = 0;
        }
    }
}
