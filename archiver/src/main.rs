use clap::{crate_description, crate_name, App, Arg};
use console::style;
use solana_archiver_lib::archiver::Archiver;
use solana_clap_utils::{
    input_parsers::keypair_of, input_validators::is_keypair_or_ask_keyword,
    keypair::SKIP_SEED_PHRASE_VALIDATION_ARG,
};
use solana_core::{
    cluster_info::{Node, VALIDATOR_PORT_RANGE},
    contact_info::ContactInfo,
};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::{Keypair, Signer},
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("identity_keypair")
                .short("i")
                .long("identity")
                .value_name("PATH")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .help("File containing an identity (keypair)"),
        )
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .required(true)
                .validator(solana_net_utils::is_host_port)
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
        .arg(
            Arg::with_name("storage_keypair")
                .short("s")
                .long("storage-keypair")
                .value_name("PATH")
                .takes_value(true)
                .validator(is_keypair_or_ask_keyword)
                .help("File containing the storage account keypair"),
        )
        .arg(
            Arg::with_name(SKIP_SEED_PHRASE_VALIDATION_ARG.name)
                .long(SKIP_SEED_PHRASE_VALIDATION_ARG.long)
                .help(SKIP_SEED_PHRASE_VALIDATION_ARG.help),
        )
        .get_matches();

    let ledger_path = PathBuf::from(matches.value_of("ledger").unwrap());

    let identity_keypair = keypair_of(&matches, "identity_keypair").unwrap_or_else(Keypair::new);

    let storage_keypair = keypair_of(&matches, "storage_keypair").unwrap_or_else(|| {
        clap::Error::with_description(
            "The `storage-keypair` argument was not found",
            clap::ErrorKind::ArgumentNotFound,
        )
        .exit();
    });

    let entrypoint_addr = matches
        .value_of("entrypoint")
        .map(|entrypoint| {
            solana_net_utils::parse_host_port(entrypoint)
                .expect("failed to parse entrypoint address")
        })
        .unwrap();

    let gossip_addr = {
        let ip = solana_net_utils::get_public_ip_addr(&entrypoint_addr).unwrap();
        let mut addr = SocketAddr::new(ip, 0);
        addr.set_ip(solana_net_utils::get_public_ip_addr(&entrypoint_addr).unwrap());
        addr
    };
    let node = Node::new_archiver_with_external_ip(
        &identity_keypair.pubkey(),
        &gossip_addr,
        VALIDATOR_PORT_RANGE,
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
    );

    println!(
        "{} version {} (branch={}, commit={})",
        style(crate_name!()).bold(),
        solana_clap_utils::version!(),
        option_env!("CI_BRANCH").unwrap_or("unknown"),
        option_env!("CI_COMMIT").unwrap_or("unknown")
    );
    solana_metrics::set_host_id(identity_keypair.pubkey().to_string());
    println!(
        "replicating the data with identity_keypair={:?} gossip_addr={:?}",
        identity_keypair.pubkey(),
        gossip_addr
    );

    let entrypoint_info = ContactInfo::new_gossip_entry_point(&entrypoint_addr);
    let archiver = Archiver::new(
        &ledger_path,
        node,
        entrypoint_info,
        Arc::new(identity_keypair),
        Arc::new(storage_keypair),
        CommitmentConfig::recent(),
    )
    .unwrap();

    archiver.join();
}
