#[macro_use]
extern crate clap;
use dirs;
#[macro_use]
extern crate solana;

use clap::{App, Arg, ArgMatches, SubCommand};
use solana::logger;
use solana::wallet::{gen_keypair_file, parse_command, process_command, WalletConfig, WalletError};
use solana_sdk::signature::{read_keypair, KeypairUtil};
use std::error;
use std::net::SocketAddr;

pub fn parse_args(matches: &ArgMatches<'_>) -> Result<WalletConfig, Box<dyn error::Error>> {
    let network = if let Some(addr) = matches.value_of("network") {
        addr.parse().or_else(|_| {
            Err(WalletError::BadParameter(
                "Invalid network location".to_string(),
            ))
        })?
    } else {
        socketaddr!("127.0.0.1:8001")
    };
    let timeout = if let Some(secs) = matches.value_of("timeout") {
        Some(secs.to_string().parse().expect("integer"))
    } else {
        None
    };

    let proxy = matches.value_of("proxy").map(|proxy| proxy.to_string());

    let mut path = dirs::home_dir().expect("home directory");
    let id_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        if !path.exists() {
            gen_keypair_file(path.to_str().unwrap().to_string())?;
            println!("New keypair generated at: {:?}", path.to_str().unwrap());
        }

        path.to_str().unwrap()
    };
    let id = read_keypair(id_path).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Unable to open keypair file: {}",
            err, id_path
        )))
    })?;

    let command = parse_command(id.pubkey(), &matches)?;

    Ok(WalletConfig {
        id,
        command,
        network,
        timeout,
        proxy,
        drone_port: None,
    })
}

fn main() -> Result<(), Box<dyn error::Error>> {
    logger::setup();
    let matches = App::new("solana-wallet")
        .version(crate_version!())
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("HOST:PORT")
                .takes_value(true)
                .help("Rendezvous with the network at this gossip entry point; defaults to 127.0.0.1:8001"),
        ).arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/id.json"),
        ).arg(
            Arg::with_name("timeout")
                .long("timeout")
                .value_name("SECS")
                .takes_value(true)
                .help("Max seconds to wait to get necessary gossip from the network"),
        ).arg(
            Arg::with_name("proxy")
                .long("proxy")
                .takes_value(true)
                .value_name("URL")
                .help("Address of TLS proxy")
                .conflicts_with("rpc-port")
        ).subcommand(SubCommand::with_name("address").about("Get your public key"))
        .subcommand(
            SubCommand::with_name("airdrop")
                .about("Request a batch of tokens")
                .arg(
                    Arg::with_name("tokens")
                        .index(1)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of tokens to request"),
                ),
        ).subcommand(SubCommand::with_name("balance").about("Get your balance"))
        .subcommand(
            SubCommand::with_name("cancel")
                .about("Cancel a transfer")
                .arg(
                    Arg::with_name("process-id")
                        .index(1)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .help("The process id of the transfer to cancel"),
                ),
        ).subcommand(
            SubCommand::with_name("confirm")
                .about("Confirm transaction by signature")
                .arg(
                    Arg::with_name("signature")
                        .index(1)
                        .value_name("SIGNATURE")
                        .takes_value(true)
                        .required(true)
                        .help("The transaction signature to confirm"),
                ),
        ).subcommand(
            SubCommand::with_name("deploy")
                .about("Deploy a program")
                .arg(
                    Arg::with_name("program-location")
                        .index(1)
                        .value_name("PATH")
                        .takes_value(true)
                        .required(true)
                        .help("/path/to/program.o"),
                )
                // TODO: Add "loader" argument; current default is bpf_loader
        ).subcommand(
            SubCommand::with_name("get-transaction-count")
                .about("Get current transaction count")
        ).subcommand(
            SubCommand::with_name("pay")
                .about("Send a payment")
                .arg(
                    Arg::with_name("to")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("The pubkey of recipient"),
                ).arg(
                    Arg::with_name("tokens")
                        .index(2)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of tokens to send"),
                ).arg(
                    Arg::with_name("timestamp")
                        .long("after")
                        .value_name("DATETIME")
                        .takes_value(true)
                        .help("A timestamp after which transaction will execute"),
                ).arg(
                    Arg::with_name("timestamp-pubkey")
                        .long("require-timestamp-from")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .requires("timestamp")
                        .help("Require timestamp from this third party"),
                ).arg(
                    Arg::with_name("witness")
                        .long("require-signature-from")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .multiple(true)
                        .use_delimiter(true)
                        .help("Any third party signatures required to unlock the tokens"),
                ).arg(
                    Arg::with_name("cancelable")
                        .long("cancelable")
                        .takes_value(false),
                ),
        ).subcommand(
            SubCommand::with_name("send-signature")
                .about("Send a signature to authorize a transfer")
                .arg(
                    Arg::with_name("to")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("The pubkey of recipient"),
                ).arg(
                    Arg::with_name("process-id")
                        .index(2)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .help("The process id of the transfer to authorize")
                )
        ).subcommand(
            SubCommand::with_name("send-timestamp")
                .about("Send a timestamp to unlock a transfer")
                .arg(
                    Arg::with_name("to")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("The pubkey of recipient"),
                ).arg(
                    Arg::with_name("process-id")
                        .index(2)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .help("The process id of the transfer to unlock")
                ).arg(
                    Arg::with_name("datetime")
                        .long("date")
                        .value_name("DATETIME")
                        .takes_value(true)
                        .help("Optional arbitrary timestamp to apply")
                )
        ).get_matches();

    let config = parse_args(&matches)?;
    let result = process_command(&config)?;
    println!("{}", result);
    Ok(())
}
