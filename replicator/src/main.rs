use clap::{crate_description, crate_name, crate_version, App, Arg};
use solana::cluster_info::{Node, FULLNODE_PORT_RANGE};
use solana::contact_info::ContactInfo;
use solana::replicator::Replicator;
use solana::socketaddr;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use std::process::exit;
use std::sync::Arc;

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("identity")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .help("File containing an identity (keypair)"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(true)
                .help("Rendezvous with the cluster at this entry point"),
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

    let ledger_path = matches.value_of("ledger").unwrap();

    let keypair = if let Some(identity) = matches.value_of("identity") {
        read_keypair(identity).unwrap_or_else(|err| {
            eprintln!("{}: Unable to open keypair file: {}", err, identity);
            exit(1);
        })
    } else {
        Keypair::new()
    };

    let entrypoint_addr = matches
        .value_of("entrypoint")
        .map(|entrypoint| {
            solana_netutil::parse_host_port(entrypoint).expect("failed to parse entrypoint address")
        })
        .unwrap();

    let gossip_addr = {
        let mut addr = socketaddr!([127, 0, 0, 1], 8700);
        addr.set_ip(solana_netutil::get_public_ip_addr(&entrypoint_addr).unwrap());
        addr
    };
    let node =
        Node::new_replicator_with_external_ip(&keypair.pubkey(), &gossip_addr, FULLNODE_PORT_RANGE);

    println!(
        "replicating the data with keypair={:?} gossip_addr={:?}",
        keypair.pubkey(),
        gossip_addr
    );

    let entrypoint_info = ContactInfo::new_gossip_entry_point(&entrypoint_addr);
    let storage_keypair = Arc::new(Keypair::new());
    let mut replicator = Replicator::new(
        ledger_path,
        node,
        entrypoint_info,
        Arc::new(keypair),
        storage_keypair,
        None,
    )
    .unwrap();

    replicator.run();
    replicator.close();
}
