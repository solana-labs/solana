#[macro_use]
extern crate lazy_static;

use clap::{crate_description, crate_name, App, AppSettings, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{
    input_parsers::pubkey_of,
    input_validators::{is_pubkey, is_url},
};

mod build_env;
mod command;
mod config;
mod defaults;
mod stop_process;
mod update_manifest;

pub fn is_semver(semver: &str) -> Result<(), String> {
    match semver::Version::parse(&semver) {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

pub fn is_release_channel(channel: &str) -> Result<(), String> {
    match channel {
        "edge" | "beta" | "stable" => Ok(()),
        _ => Err(format!("Invalid release channel {}", channel)),
    }
}

pub fn is_explicit_release(string: String) -> Result<(), String> {
    if string.starts_with('v') && is_semver(string.split_at(1).1).is_ok() {
        return Ok(());
    }
    is_semver(&string).or_else(|_| is_release_channel(&string))
}

pub fn explicit_release_of(
    matches: &ArgMatches<'_>,
    name: &str,
) -> Option<config::ExplicitRelease> {
    matches
        .value_of(name)
        .map(ToString::to_string)
        .map(|explicit_release| {
            if explicit_release.starts_with('v')
                && is_semver(explicit_release.split_at(1).1).is_ok()
            {
                config::ExplicitRelease::Semver(explicit_release.split_at(1).1.to_string())
            } else if is_semver(&explicit_release).is_ok() {
                config::ExplicitRelease::Semver(explicit_release)
            } else {
                config::ExplicitRelease::Channel(explicit_release)
            }
        })
}

fn handle_init(matches: &ArgMatches<'_>, config_file: &str) -> Result<(), String> {
    let json_rpc_url = matches.value_of("json_rpc_url").unwrap();
    let update_manifest_pubkey = pubkey_of(&matches, "update_manifest_pubkey");
    let data_dir = matches.value_of("data_dir").unwrap();
    let no_modify_path = matches.is_present("no_modify_path");
    let explicit_release = explicit_release_of(&matches, "explicit_release");

    if update_manifest_pubkey.is_none() && explicit_release.is_none() {
        Err(format!(
            "Please specify the release to install for {}.  See --help for more",
            build_env::TARGET
        ))
    } else {
        command::init(
            config_file,
            data_dir,
            json_rpc_url,
            &update_manifest_pubkey.unwrap_or_default(),
            no_modify_path,
            explicit_release,
        )
    }
}

pub fn main() -> Result<(), String> {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg({
            let arg = Arg::with_name("config_file")
                .short("c")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            match *defaults::CONFIG_FILE {
                Some(ref config_file) => arg.default_value(&config_file),
                None => arg.required(true),
            }
        })
        .subcommand(
            SubCommand::with_name("init")
                .about("initializes a new installation")
                .setting(AppSettings::DisableVersion)
                .arg({
                    let arg = Arg::with_name("data_dir")
                        .short("d")
                        .long("data-dir")
                        .value_name("PATH")
                        .takes_value(true)
                        .required(true)
                        .help("Directory to store install data");
                    match *defaults::DATA_DIR {
                        Some(ref data_dir) => arg.default_value(&data_dir),
                        None => arg,
                    }
                })
                .arg(
                    Arg::with_name("json_rpc_url")
                        .short("u")
                        .long("url")
                        .value_name("URL")
                        .takes_value(true)
                        .default_value(defaults::JSON_RPC_URL)
                        .validator(is_url)
                        .help("JSON RPC URL for the solana cluster"),
                )
                .arg(
                    Arg::with_name("no_modify_path")
                        .long("no-modify-path")
                        .help("Don't configure the PATH environment variable"),
                )
                .arg({
                    let arg = Arg::with_name("update_manifest_pubkey")
                        .short("p")
                        .long("pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey)
                        .help("Public key of the update manifest");

                    match defaults::update_manifest_pubkey(build_env::TARGET) {
                        Some(default_value) => arg.default_value(default_value),
                        None => arg,
                    }
                })
                .arg(
                    Arg::with_name("explicit_release")
                        .value_name("release")
                        .index(1)
                        .conflicts_with_all(&["json_rpc_url", "update_manifest_pubkey"])
                        .validator(is_explicit_release)
                        .help("The release version or channel to install"),
                ),
        )
        .subcommand(
            SubCommand::with_name("info")
                .about("displays information about the current installation")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("local_info_only")
                        .short("l")
                        .long("local")
                        .help(
                        "only display local information, don't check the cluster for new updates",
                    ),
                ),
        )
        .subcommand(
            SubCommand::with_name("deploy")
                .about("deploys a new update")
                .setting(AppSettings::DisableVersion)
                .arg({
                    let arg = Arg::with_name("from_keypair_file")
                        .short("k")
                        .long("keypair")
                        .value_name("PATH")
                        .takes_value(true)
                        .required(true)
                        .help("Keypair file of the account that funds the deployment");
                    match *defaults::USER_KEYPAIR {
                        Some(ref config_file) => arg.default_value(&config_file),
                        None => arg,
                    }
                })
                .arg(
                    Arg::with_name("json_rpc_url")
                        .short("u")
                        .long("url")
                        .value_name("URL")
                        .takes_value(true)
                        .default_value(defaults::JSON_RPC_URL)
                        .validator(is_url)
                        .help("JSON RPC URL for the solana cluster"),
                )
                .arg(
                    Arg::with_name("download_url")
                        .index(1)
                        .required(true)
                        .validator(is_url)
                        .help("URL to the solana release archive"),
                )
                .arg(
                    Arg::with_name("update_manifest_keypair_file")
                        .index(2)
                        .required(true)
                        .help("Keypair file for the update manifest (/path/to/keypair.json)"),
                ),
        )
        .subcommand(
            SubCommand::with_name("update")
                .about("checks for an update, and if available downloads and applies it")
                .setting(AppSettings::DisableVersion),
        )
        .subcommand(
            SubCommand::with_name("run")
                .about("Runs a program while periodically checking and applying software updates")
                .after_help("The program will be restarted upon a successful software update")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("program_name")
                        .index(1)
                        .required(true)
                        .help("program to run"),
                )
                .arg(
                    Arg::with_name("program_arguments")
                        .index(2)
                        .multiple(true)
                        .help("arguments to supply to the program"),
                ),
        )
        .get_matches();

    let config_file = matches.value_of("config_file").unwrap();

    match matches.subcommand() {
        ("init", Some(matches)) => handle_init(&matches, &config_file),
        ("info", Some(matches)) => {
            let local_info_only = matches.is_present("local_info_only");
            command::info(config_file, local_info_only).map(|_| ())
        }
        ("deploy", Some(matches)) => {
            let from_keypair_file = matches.value_of("from_keypair_file").unwrap();
            let json_rpc_url = matches.value_of("json_rpc_url").unwrap();
            let download_url = matches.value_of("download_url").unwrap();
            let update_manifest_keypair_file =
                matches.value_of("update_manifest_keypair_file").unwrap();
            command::deploy(
                json_rpc_url,
                from_keypair_file,
                download_url,
                update_manifest_keypair_file,
            )
        }
        ("update", Some(_matches)) => command::update(config_file).map(|_| ()),
        ("run", Some(matches)) => {
            let program_name = matches.value_of("program_name").unwrap();
            let program_arguments = matches
                .values_of("program_arguments")
                .map(Iterator::collect)
                .unwrap_or_else(|| vec![]);

            command::run(config_file, program_name, program_arguments)
        }
        _ => unreachable!(),
    }
}

pub fn main_init() -> Result<(), String> {
    solana_logger::setup();

    let matches = App::new("solana-install-init")
        .about("initializes a new installation")
        .version(solana_clap_utils::version!())
        .arg({
            let arg = Arg::with_name("config_file")
                .short("c")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .help("Configuration file to use");
            match *defaults::CONFIG_FILE {
                Some(ref config_file) => arg.default_value(&config_file),
                None => arg.required(true),
            }
        })
        .arg({
            let arg = Arg::with_name("data_dir")
                .short("d")
                .long("data-dir")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("Directory to store install data");
            match *defaults::DATA_DIR {
                Some(ref data_dir) => arg.default_value(&data_dir),
                None => arg,
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .short("u")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .default_value(defaults::JSON_RPC_URL)
                .validator(is_url)
                .help("JSON RPC URL for the solana cluster"),
        )
        .arg(
            Arg::with_name("no_modify_path")
                .long("no-modify-path")
                .help("Don't configure the PATH environment variable"),
        )
        .arg({
            let arg = Arg::with_name("update_manifest_pubkey")
                .short("p")
                .long("pubkey")
                .value_name("PUBKEY")
                .takes_value(true)
                .validator(is_pubkey)
                .help("Public key of the update manifest");

            match defaults::update_manifest_pubkey(build_env::TARGET) {
                Some(default_value) => arg.default_value(default_value),
                None => arg,
            }
        })
        .arg(
            Arg::with_name("explicit_release")
                .value_name("release")
                .index(1)
                .conflicts_with_all(&["json_rpc_url", "update_manifest_pubkey"])
                .validator(is_explicit_release)
                .help("The release version or channel to install"),
        )
        .get_matches();

    let config_file = matches.value_of("config_file").unwrap();
    handle_init(&matches, &config_file)
}
