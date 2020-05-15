//! A command-line executable for monitoring a cluster's gossip plane.

use clap::{
    crate_description, crate_name, value_t, value_t_or_exit, App, AppSettings, Arg, ArgMatches,
    SubCommand,
};
use solana_clap_utils::input_validators::{is_port, is_pubkey};
use solana_client::rpc_client::RpcClient;
use solana_core::{contact_info::ContactInfo, gossip_service::discover};
use solana_sdk::pubkey::Pubkey;
use std::error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::exit;

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup_with_default("solana=info");

    let shred_version_arg = Arg::with_name("shred_version")
        .long("shred-version")
        .value_name("VERSION")
        .takes_value(true)
        .default_value("0")
        .help("Filter gossip nodes by this shred version");

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("rpc-url")
                .about("Get an RPC URL for the cluster")
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
                    Arg::with_name("all")
                        .long("all")
                        .takes_value(false)
                        .help("Return all RPC URLs"),
                )
                .arg(
                    Arg::with_name("any")
                        .long("any")
                        .takes_value(false)
                        .conflicts_with("all")
                        .help("Return any RPC URL"),
                )
                .arg(
                    Arg::with_name("timeout")
                        .long("timeout")
                        .value_name("SECONDS")
                        .takes_value(true)
                        .default_value("15")
                        .help("Timeout in seconds"),
                )
                .arg(&shred_version_arg)
                .setting(AppSettings::DisableVersion),
        )
        .subcommand(
            SubCommand::with_name("spy")
                .about("Monitor the gossip entrypoint")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("entrypoint")
                        .short("n")
                        .long("entrypoint")
                        .value_name("HOST:PORT")
                        .takes_value(true)
                        .validator(solana_net_utils::is_host_port)
                        .help("Rendezvous with the cluster at this entrypoint"),
                )
                .arg(
                    clap::Arg::with_name("gossip_port")
                        .long("gossip-port")
                        .value_name("PORT")
                        .takes_value(true)
                        .validator(is_port)
                        .help("Gossip port number for the node"),
                )
                .arg(
                    clap::Arg::with_name("gossip_host")
                        .long("gossip-host")
                        .value_name("HOST")
                        .takes_value(true)
                        .validator(solana_net_utils::is_host)
                        .help("Gossip DNS name or IP address for the node [default: ask --entrypoint, or 127.0.0.1 when --entrypoint is not provided]"),
                )
                .arg(
                    Arg::with_name("num_nodes")
                        .short("N")
                        .long("num-nodes")
                        .value_name("NUM")
                        .takes_value(true)
                        .conflicts_with("num_nodes_exactly")
                        .help("Wait for at least NUM nodes to be visible"),
                )
                .arg(
                    Arg::with_name("num_nodes_exactly")
                        .short("E")
                        .long("num-nodes-exactly")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("Wait for exactly NUM nodes to be visible"),
                )
                .arg(
                    Arg::with_name("node_pubkey")
                        .short("p")
                        .long("pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey)
                        .help("Public key of a specific node to wait for"),
                )
                .arg(&shred_version_arg)
                .arg(
                    Arg::with_name("timeout")
                        .long("timeout")
                        .value_name("SECONDS")
                        .takes_value(true)
                        .help("Maximum time to wait in seconds [default: wait forever]"),
                ),
        )
        .subcommand(
            SubCommand::with_name("stop")
                .about("Send stop request to a node")
                .setting(AppSettings::DisableVersion)
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
                    Arg::with_name("node_pubkey")
                        .index(1)
                        .required(true)
                        .value_name("PUBKEY")
                        .validator(is_pubkey)
                        .help("Public key of a specific node to stop"),
                ),
        )
        .get_matches();

    fn parse_entrypoint(matches: &ArgMatches) -> Option<SocketAddr> {
        matches.value_of("entrypoint").map(|entrypoint| {
            solana_net_utils::parse_host_port(entrypoint).unwrap_or_else(|e| {
                eprintln!("failed to parse entrypoint address: {}", e);
                exit(1);
            })
        })
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
            let shred_version = value_t_or_exit!(matches, "shred_version", u16);

            let entrypoint_addr = parse_entrypoint(&matches);

            let gossip_host = matches
                .value_of("gossip_host")
                .map(|gossip_host| {
                    solana_net_utils::parse_host(gossip_host).unwrap_or_else(|e| {
                        eprintln!("failed to parse gossip-host: {}", e);
                        exit(1);
                    })
                })
                .unwrap_or_else(|| {
                    if let Some(entrypoint_addr) = entrypoint_addr {
                        solana_net_utils::get_public_ip_addr(&entrypoint_addr).unwrap_or_else(
                            |err| {
                                eprintln!(
                                    "Failed to contact cluster entrypoint {}: {}",
                                    entrypoint_addr, err
                                );
                                exit(1);
                            },
                        )
                    } else {
                        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
                    }
                });

            let gossip_addr = SocketAddr::new(
                gossip_host,
                value_t!(matches, "gossip_port", u16).unwrap_or_else(|_| {
                    solana_net_utils::find_available_port_in_range(
                        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                        (0, 1),
                    )
                    .expect("unable to find an available gossip port")
                }),
            );

            let (_all_peers, validators) = discover(
                entrypoint_addr.as_ref(),
                num_nodes,
                timeout,
                pubkey,
                None,
                Some(&gossip_addr),
                shred_version,
            )?;

            if timeout.is_some() {
                if let Some(num) = num_nodes {
                    if validators.len() < num {
                        let add = if num_nodes_exactly.is_some() {
                            ""
                        } else {
                            " or more"
                        };
                        eprintln!(
                            "Error: Insufficient validators discovered.  Expecting {}{}",
                            num, add,
                        );
                        exit(1);
                    }
                }
                if let Some(node) = pubkey {
                    if validators.iter().find(|x| x.id == node).is_none() {
                        eprintln!("Error: Could not find node {:?}", node);
                        exit(1);
                    }
                }
            }
            if let Some(num_nodes_exactly) = num_nodes_exactly {
                if validators.len() > num_nodes_exactly {
                    eprintln!(
                        "Error: Extra nodes discovered.  Expecting exactly {}",
                        num_nodes_exactly
                    );
                    exit(1);
                }
            }
        }
        ("rpc-url", Some(matches)) => {
            let any = matches.is_present("any");
            let all = matches.is_present("all");
            let entrypoint_addr = parse_entrypoint(&matches);
            let timeout = value_t_or_exit!(matches, "timeout", u64);
            let shred_version = value_t_or_exit!(matches, "shred_version", u16);
            let (_all_peers, validators) = discover(
                entrypoint_addr.as_ref(),
                Some(1),
                Some(timeout),
                None,
                entrypoint_addr.as_ref(),
                None,
                shred_version,
            )?;

            let rpc_addrs: Vec<_> = validators
                .iter()
                .filter_map(|contact_info| {
                    if (any || all || Some(contact_info.gossip) == entrypoint_addr)
                        && ContactInfo::is_valid_address(&contact_info.rpc)
                    {
                        return Some(contact_info.rpc);
                    }
                    None
                })
                .collect();

            if rpc_addrs.is_empty() {
                eprintln!("No RPC URL found");
                exit(1);
            }

            for rpc_addr in rpc_addrs {
                println!("http://{}", rpc_addr);
                if any {
                    break;
                }
            }
        }
        ("stop", Some(matches)) => {
            let entrypoint_addr = parse_entrypoint(&matches);
            let pubkey = matches
                .value_of("node_pubkey")
                .unwrap()
                .parse::<Pubkey>()
                .unwrap();
            let (_all_peers, validators) = discover(
                entrypoint_addr.as_ref(),
                None,
                None,
                Some(pubkey),
                None,
                None,
                0,
            )?;
            let validator = validators.iter().find(|x| x.id == pubkey).unwrap();

            if !ContactInfo::is_valid_address(&validator.rpc) {
                eprintln!(
                    "Error: RPC service is not enabled on validator {:?}",
                    pubkey
                );
                exit(1);
            }
            println!("\nSending stop request to validator {:?}", pubkey);

            let result = RpcClient::new_socket(validator.rpc).validator_exit()?;
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
