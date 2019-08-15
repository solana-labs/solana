use clap::{crate_description, crate_name, crate_version, Arg, ArgGroup, ArgMatches, SubCommand};
use console::style;
use solana_sdk::signature::{gen_keypair_file, read_keypair, KeypairUtil};
use solana_wallet::config::{self, Config};
use solana_wallet::display::println_name_value;
use solana_wallet::wallet::{app, parse_command, process_command, WalletConfig, WalletError};
use std::error;

fn parse_settings(matches: &ArgMatches<'_>) -> Result<bool, Box<dyn error::Error>> {
    let parse_args = match matches.subcommand() {
        ("get", Some(subcommand_matches)) => {
            if let Some(config_file) = matches.value_of("config_file") {
                let config = Config::load(config_file).unwrap_or_default();
                if let Some(field) = subcommand_matches.value_of("specific_setting") {
                    let value = match field {
                        "url" => config.url,
                        "keypair" => config.keypair,
                        _ => unreachable!(),
                    };
                    println_name_value(&format!("* {}:", field), &value);
                } else {
                    println_name_value("Wallet Config:", config_file);
                    println_name_value("* url:", &config.url);
                    println_name_value("* keypair:", &config.keypair);
                }
            } else {
                println!("{} Either provide the `--config` arg or ensure home directory exists to use the default config location", style("No config file found.").bold());
            }
            false
        }
        ("set", Some(subcommand_matches)) => {
            if let Some(config_file) = matches.value_of("config_file") {
                let mut config = Config::load(config_file).unwrap_or_default();
                if let Some(url) = subcommand_matches.value_of("url") {
                    config.url = url.to_string();
                }
                if let Some(keypair) = subcommand_matches.value_of("keypair") {
                    config.keypair = keypair.to_string();
                }
                config.save(config_file)?;
                println_name_value("Wallet Config Updated:", config_file);
                println_name_value("* url:", &config.url);
                println_name_value("* keypair:", &config.keypair);
            } else {
                println!("{} Either provide the `--config` arg or ensure home directory exists to use the default config location", style("No config file found.").bold());
            }
            false
        }
        _ => true,
    };
    Ok(parse_args)
}

pub fn parse_args(matches: &ArgMatches<'_>) -> Result<WalletConfig, Box<dyn error::Error>> {
    let config = if let Some(config_file) = matches.value_of("config_file") {
        Config::load(config_file).unwrap_or_default()
    } else {
        Config::default()
    };
    let json_rpc_url = if let Some(url) = matches.value_of("json_rpc_url") {
        url.to_string()
    } else if config.url != "" {
        config.url
    } else {
        let default = WalletConfig::default();
        default.json_rpc_url
    };

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
    } else if config.keypair != "" {
        &config.keypair
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
        .arg({
            let arg = Arg::with_name("config_file")
                .short("c")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *config::CONFIG_FILE {
                arg.default_value(&config_file)
            } else {
                arg
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .short("u")
                .long("url")
                .value_name("URL")
                .takes_value(true)
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
        .subcommand(
            SubCommand::with_name("get")
                .about("Get wallet config settings")
                .arg(
                    Arg::with_name("specific_setting")
                        .index(1)
                        .value_name("CONFIG_FIELD")
                        .takes_value(true)
                        .possible_values(&["url", "keypair"])
                        .help("Return a specific config setting"),
                ),
        )
        .subcommand(
            SubCommand::with_name("set")
                .about("Set a wallet config setting")
                .arg(
                    Arg::with_name("url")
                        .short("u")
                        .long("url")
                        .value_name("URL")
                        .takes_value(true)
                        .validator(is_url)
                        .help("Set default JSON RPC URL to query"),
                )
                .arg(
                    Arg::with_name("keypair")
                        .short("k")
                        .long("keypair")
                        .value_name("PATH")
                        .takes_value(true)
                        .help("/path/to/id.json"),
                )
                .group(
                    ArgGroup::with_name("config_settings")
                        .args(&["url", "keypair"])
                        .multiple(true)
                        .required(true),
                ),
        )
        .get_matches();

    if parse_settings(&matches)? {
        let config = parse_args(&matches)?;
        let result = process_command(&config)?;
        println!("{}", result);
    }
    Ok(())
}
