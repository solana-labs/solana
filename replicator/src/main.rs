use clap::{crate_version, App, Arg};
use serde_json;
use solana::cluster_info::{Node, NodeInfo, FULLNODE_PORT_RANGE};
use solana::replicator::Replicator;
use solana::socketaddr;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::exit;

fn main() {
    solana_logger::setup();

    let matches = App::new("replicator")
        .version(crate_version!())
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("Run with the identity found in FILE"),
        )
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(true)
                .help("Rendezvous with the network at this gossip entry point"),
        )
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("use DIR as persistent ledger location"),
        )
        .get_matches();

    let ledger_path = matches.value_of("ledger");

    let (keypair, gossip) = if let Some(i) = matches.value_of("identity") {
        let path = i.to_string();
        if let Ok(file) = File::open(path.clone()) {
            let parse: serde_json::Result<solana_fullnode_config::Config> =
                serde_json::from_reader(file);
            if let Ok(config_data) = parse {
                let keypair = config_data.keypair();
                let node_info = NodeInfo::new_with_pubkey_socketaddr(
                    keypair.pubkey(),
                    &config_data.bind_addr(FULLNODE_PORT_RANGE.0),
                );
                (keypair, node_info.gossip)
            } else {
                eprintln!("failed to parse {}", path);
                exit(1);
            }
        } else {
            eprintln!("failed to read {}", path);
            exit(1);
        }
    } else {
        (Keypair::new(), socketaddr!([127, 0, 0, 1], 8700))
    };

    let node = Node::new_with_external_ip(keypair.pubkey(), &gossip);

    println!(
        "replicating the data with keypair: {:?} gossip:{:?}",
        keypair.pubkey(),
        gossip
    );

    let network_addr = matches
        .value_of("network")
        .map(|network| network.parse().expect("failed to parse network address"))
        .unwrap();

    let leader_info = NodeInfo::new_entry_point(&network_addr);

    let replicator = Replicator::new(ledger_path, node, &leader_info, &keypair).unwrap();

    replicator.join();
}
