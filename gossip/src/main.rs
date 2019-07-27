//! A command-line executable for monitoring a cluster's gossip plane.

#[macro_use]
extern crate solana;

use clap::{crate_description, crate_name, crate_version, App, AppSettings, Arg, SubCommand};
use solana::contact_info::ContactInfo;
use solana::gossip_service::discover;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::error;
use std::net::SocketAddr;
use std::process::exit;

fn pubkey_validator(pubkey: String) -> Result<(), String> {
    match pubkey.parse::<Pubkey>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("solana=info")).init();

    let mut entrypoint_addr = SocketAddr::from(([127, 0, 0, 1], 8001));
    let entrypoint_string = entrypoint_addr.to_string();
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("entrypoint")
                .short("n")
                .long("entrypoint")
                .value_name("HOST:PORT")
                .takes_value(true)
                .default_value(&entrypoint_string)
                .help("Rendezvous with the cluster at this entry point"),
        )
        .subcommand(
            SubCommand::with_name("spy")
                .about("Monitor the gossip entrypoint")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("num_nodes")
                        .short("N")
                        .long("num-nodes")
                        .value_name("NUM")
                        .takes_value(true)
                        .conflicts_with("num_nodes_exactly")
                        .help("Wait for at least NUM nodes to converge"),
                )
                .arg(
                    Arg::with_name("num_nodes_exactly")
                        .short("E")
                        .long("num-nodes-exactly")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("Wait for exactly NUM nodes to converge"),
                )
                .arg(
                    Arg::with_name("node_pubkey")
                        .short("p")
                        .long("pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(pubkey_validator)
                        .help("Public key of a specific node to wait for"),
                )
                .arg(
                    Arg::with_name("timeout")
                        .long("timeout")
                        .value_name("SECS")
                        .takes_value(true)
                        .help(
                            "Maximum time to wait for cluster to converge [default: wait forever]",
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("stop")
                .about("Send stop request to a node")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("node_pubkey")
                        .index(1)
                        .required(true)
                        .value_name("PUBKEY")
                        .validator(pubkey_validator)
                        .help("Public key of a specific node to stop"),
                ),
        )
        .get_matches();

    if let Some(addr) = matches.value_of("entrypoint") {
        entrypoint_addr = solana_netutil::parse_host_port(addr).unwrap_or_else(|e| {
            eprintln!("failed to parse entrypoint address: {}", e);
            exit(1)
        });
    }
    match matches.subcommand() {
        ("spy", Some(matches)) => {
            let num_nodes_exactly = matches
                .value_of("num_nodes_exactly")
                .map(|num| num.to_string().parse().unwrap());
            let num_nodes = matches
                .value_of("num_nodes")
                .map(|num| num.to_string().parse().unwrap())
                .or(num_nodes_exactly);
            let timeout = matches
                .value_of("timeout")
                .map(|secs| secs.to_string().parse().unwrap());
            let pubkey = matches
                .value_of("node_pubkey")
                .map(|pubkey_str| pubkey_str.parse::<Pubkey>().unwrap());

            let gossip_addr = {
                let mut addr = socketaddr_any!();
                addr.set_ip(
                    solana_netutil::get_public_ip_addr(&entrypoint_addr).unwrap_or_else(|err| {
                        eprintln!("failed to contact {}: {}", entrypoint_addr, err);
                        exit(1)
                    }),
                );
                Some(addr)
            };

            let (nodes, _replicators) = discover(
                &entrypoint_addr,
                num_nodes,
                timeout,
                pubkey,
                gossip_addr.as_ref(),
            )?;

            if timeout.is_some() {
                if let Some(num) = num_nodes {
                    if nodes.len() < num {
                        let add = if num_nodes_exactly.is_some() {
                            ""
                        } else {
                            " or more"
                        };
                        eprintln!(
                            "Error: Insufficient nodes discovered.  Expecting {}{}",
                            num, add,
                        );
                    }
                }
                if let Some(node) = pubkey {
                    if nodes.iter().find(|x| x.id == node).is_none() {
                        eprintln!("Error: Could not find node {:?}", node);
                    }
                }
            }
            if num_nodes_exactly.is_some() && nodes.len() > num_nodes_exactly.unwrap() {
                eprintln!(
                    "Error: Extra nodes discovered.  Expecting exactly {}",
                    num_nodes_exactly.unwrap()
                );
            }
        }
        ("stop", Some(matches)) => {
            let pubkey = matches
                .value_of("node_pubkey")
                .unwrap()
                .parse::<Pubkey>()
                .unwrap();
            let (nodes, _replicators) = discover(&entrypoint_addr, None, None, Some(pubkey), None)?;
            let node = nodes.iter().find(|x| x.id == pubkey).unwrap();

            if !ContactInfo::is_valid_address(&node.rpc) {
                eprintln!("Error: RPC service is not enabled on node {:?}", pubkey);
            }
            println!("\nSending stop request to node {:?}", pubkey);

            let result = RpcClient::new_socket(node.rpc).fullnode_exit()?;
            if result {
                println!("Stop signal accepted");
            } else {
                eprintln!("Error: Stop signal ignored");
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}
