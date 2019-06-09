use clap::{crate_description, crate_name, crate_version, Arg, ArgMatches};
use solana_sdk::signature::{gen_keypair_file, read_keypair, KeypairUtil};
use solana_wallet::wallet::{app, parse_command, process_command, WalletConfig, WalletError};
use std::error;

pub fn parse_args(matches: &ArgMatches<'_>) -> Result<WalletConfig, Box<dyn error::Error>> {
    let json_rpc_url = matches.value_of("json_rpc_url").unwrap().to_string();

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

    let mut path = dirs::home_dir().expect("home directory");
    let id_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        if !path.exists() {
            gen_keypair_file(path.to_str().unwrap())?;
            println!("New keypair generated at: {}", path.to_str().unwrap());
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
        command,
        drone_host,
        drone_port,
        json_rpc_url,
        keypair,
        rpc_client: None,
    })
}

// Return an error if a url cannot be parsed.
fn is_url(string: String) -> Result<(), String> {
    match url::Url::parse(&string) {
        Ok(url) => {
            if url.has_host() {
                Ok(())
            } else {
                Err("no host provided".to_string())
            }
        }
        Err(err) => Err(format!("{:?}", err)),
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup();

    let default = WalletConfig::default();
    let default_drone_port = format!("{}", default.drone_port);

    let matches = app(crate_name!(), crate_description!(), crate_version!())
        .arg(
            Arg::with_name("json_rpc_url")
                .short("u")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .default_value(&default.json_rpc_url)
                .validator(is_url)
                .help("JSON RPC URL for the solana cluster"),
        )
        .arg(
            Arg::with_name("drone_host")
                .long("drone-host")
                .value_name("HOST")
                .takes_value(true)
                .help("Drone host to use [default: same as the --url host]"),
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
        .get_matches();

    let config = parse_args(&matches)?;
    let result = process_command(&config)?;
    println!("{}", result);
    Ok(())
}
