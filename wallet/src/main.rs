use clap::{
    crate_description, crate_name, crate_version, App, AppSettings, Arg, ArgMatches, SubCommand,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{gen_keypair_file, read_keypair, KeypairUtil};
use solana_wallet::wallet::{parse_command, process_command, WalletConfig, WalletError};
use std::error;

pub fn parse_args(matches: &ArgMatches<'_>) -> Result<WalletConfig, Box<dyn error::Error>> {
    let host = solana_netutil::parse_host(matches.value_of("host").unwrap()).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "Invalid host: {:?}",
            err
        )))
    })?;

    let drone_host = if let Some(drone_host) = matches.value_of("drone_host") {
        Some(solana_netutil::parse_host(drone_host).or_else(|err| {
            Err(WalletError::BadParameter(format!(
                "Invalid drone host: {:?}",
                err
            )))
        })?)
    } else {
        None
    };

    let rpc_host = if let Some(rpc_host) = matches.value_of("rpc_host") {
        Some(solana_netutil::parse_host(rpc_host).or_else(|err| {
            Err(WalletError::BadParameter(format!(
                "Invalid rpc host: {:?}",
                err
            )))
        })?)
    } else {
        None
    };

    let drone_port = matches
        .value_of("drone_port")
        .unwrap()
        .parse()
        .or_else(|err| {
            Err(WalletError::BadParameter(format!(
                "Invalid drone port: {:?}",
                err
            )))
        })?;

    let rpc_port = matches
        .value_of("rpc_port")
        .unwrap()
        .parse()
        .or_else(|err| {
            Err(WalletError::BadParameter(format!(
                "Invalid rpc port: {:?}",
                err
            )))
        })?;

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
    let keypair = read_keypair(id_path).or_else(|err| {
        Err(WalletError::BadParameter(format!(
            "{}: Unable to open keypair file: {}",
            err, id_path
        )))
    })?;

    let command = parse_command(&keypair.pubkey(), &matches)?;

    Ok(WalletConfig {
        keypair,
        command,
        drone_host,
        drone_port,
        host,
        rpc_client: None,
        rpc_host,
        rpc_port,
        rpc_tls: matches.is_present("rpc_tls"),
    })
}

// Return an error if a pubkey cannot be parsed.
fn is_pubkey(string: String) -> Result<(), String> {
    match string.parse::<Pubkey>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup();

    let (default_host, default_rpc_port, default_drone_port) = {
        let defaults = WalletConfig::default();
        (
            defaults.host.to_string(),
            defaults.rpc_port.to_string(),
            defaults.drone_port.to_string(),
        )
    };

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(
            Arg::with_name("host")
                .short("n")
                .long("host")
                .value_name("IP ADDRESS")
                .takes_value(true)
                .default_value(&default_host)
                .help("Host to use for both RPC and drone"),
        )
        .arg(
            Arg::with_name("rpc_host")
                .long("rpc-host")
                .value_name("IP ADDRESS")
                .takes_value(true)
                .help("RPC host to use [default: same as --host]"),
        )
        .arg(
            Arg::with_name("rpc_port")
                .long("rpc-port")
                .value_name("PORT")
                .takes_value(true)
                .default_value(&default_rpc_port)
                .help("RPC port to use"),
        )
        .arg(
            Arg::with_name("rpc_tps")
                .long("rpc-tls")
                .help("Enable TLS for the RPC endpoint"),
        )
        .arg(
            Arg::with_name("drone_host")
                .long("drone-host")
                .value_name("IP ADDRESS")
                .takes_value(true)
                .help("Drone host to use [default: same as --host]"),
        )
        .arg(
            Arg::with_name("drone_port")
                .long("drone-port")
                .value_name("PORT")
                .takes_value(true)
                .default_value(&default_drone_port)
                .help("Drone port to use"),
        )
        .arg(
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
                .help("/path/to/id.json"),
        )
        .subcommand(SubCommand::with_name("address").about("Get your public key"))
        .subcommand(
            SubCommand::with_name("airdrop")
                .about("Request a batch of lamports")
                .arg(
                    Arg::with_name("lamports")
                        .index(1)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to request"),
                ),
        )
        .subcommand(
            SubCommand::with_name("balance")
                .about("Get your balance")
                .arg(
                    Arg::with_name("pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey)
                        .help("The public key of the balance to check"),
                ),
        )
        .subcommand(
            SubCommand::with_name("cancel")
                .about("Cancel a transfer")
                .arg(
                    Arg::with_name("process_id")
                        .index(1)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The process id of the transfer to cancel"),
                ),
        )
        .subcommand(
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
        )
        .subcommand(
            SubCommand::with_name("authorize-voter")
                .about("Authorize a different voter for this account")
                .arg(
                    Arg::with_name("authorized-voter-id")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Vote signer to authorize"),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-vote-account")
                .about("Create staking account for node")
                .arg(
                    Arg::with_name("voting_account_id")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Staking account address to fund"),
                )
                .arg(
                    Arg::with_name("node_id")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Staking account address to fund"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(3)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to send to staking account"),
                )
                .arg(
                    Arg::with_name("commission")
                        .value_name("NUM")
                        .takes_value(true)
                        .help("The commission on rewards this vote account should take, defaults to zero")
                ),

        )
        .subcommand(
            SubCommand::with_name("show-vote-account")
                .about("Show the contents of a vote account")
                .arg(
                    Arg::with_name("voting_account_id")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Vote account pubkey"),
                )
        )
        .subcommand(
            SubCommand::with_name("deploy")
                .about("Deploy a program")
                .arg(
                    Arg::with_name("program_location")
                        .index(1)
                        .value_name("PATH")
                        .takes_value(true)
                        .required(true)
                        .help("/path/to/program.o"),
                ), // TODO: Add "loader" argument; current default is bpf_loader
        )
        .subcommand(
            SubCommand::with_name("get-transaction-count").about("Get current transaction count"),
        )
        .subcommand(
            SubCommand::with_name("pay")
                .about("Send a payment")
                .arg(
                    Arg::with_name("to")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The pubkey of recipient"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .index(2)
                        .value_name("NUM")
                        .takes_value(true)
                        .required(true)
                        .help("The number of lamports to send"),
                )
                .arg(
                    Arg::with_name("timestamp")
                        .long("after")
                        .value_name("DATETIME")
                        .takes_value(true)
                        .help("A timestamp after which transaction will execute"),
                )
                .arg(
                    Arg::with_name("timestamp_pubkey")
                        .long("require-timestamp-from")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .requires("timestamp")
                        .validator(is_pubkey)
                        .help("Require timestamp from this third party"),
                )
                .arg(
                    Arg::with_name("witness")
                        .long("require-signature-from")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .multiple(true)
                        .use_delimiter(true)
                        .validator(is_pubkey)
                        .help("Any third party signatures required to unlock the lamports"),
                )
                .arg(
                    Arg::with_name("cancelable")
                        .long("cancelable")
                        .takes_value(false),
                ),
        )
        .subcommand(
            SubCommand::with_name("send-signature")
                .about("Send a signature to authorize a transfer")
                .arg(
                    Arg::with_name("to")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The pubkey of recipient"),
                )
                .arg(
                    Arg::with_name("process_id")
                        .index(2)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .help("The process id of the transfer to authorize"),
                ),
        )
        .subcommand(
            SubCommand::with_name("send-timestamp")
                .about("Send a timestamp to unlock a transfer")
                .arg(
                    Arg::with_name("to")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("The pubkey of recipient"),
                )
                .arg(
                    Arg::with_name("process_id")
                        .index(2)
                        .value_name("PROCESS_ID")
                        .takes_value(true)
                        .required(true)
                        .help("The process id of the transfer to unlock"),
                )
                .arg(
                    Arg::with_name("datetime")
                        .long("date")
                        .value_name("DATETIME")
                        .takes_value(true)
                        .help("Optional arbitrary timestamp to apply"),
                ),
        )
        .get_matches();

    let config = parse_args(&matches)?;
    let result = process_command(&config)?;
    println!("{}", result);
    Ok(())
}
