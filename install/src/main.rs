use clap::{crate_description, crate_name, crate_version, App, AppSettings, Arg, SubCommand};
//use clap::{crate_description, crate_version, load_yaml, App, AppSettings};
use std::error;
use std::time::Duration;

const TARGET: &str = env!("TARGET");
const BUILD_SECONDS_SINCE_UNIX_EPOCH: &str = env!("BUILD_SECONDS_SINCE_UNIX_EPOCH");

fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::ArgRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("init")
                .about("initializes a new installation")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("json_rpc_url")
                        .short("u")
                        .long("url")
                        .value_name("URL")
                        .takes_value(true)
                        .default_value("https://api.testnet.solana.com/")
                        .help("JSON RPC URL for the solana cluster"),
                )
                .arg(
                    Arg::with_name("update_pubkey")
                        .short("p")
                        .long("pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .default_value("Solana-managed update manifest")
                        .help("Public key of the update manifest"),
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
                .arg(
                    Arg::with_name("download_url")
                        .index(1)
                        .required(true)
                        .help("URL to the solana release archive"),
                )
                .arg(
                    Arg::with_name("update_manifest_keypair")
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

    println!("TARGET={}", TARGET);
    println!(
        "BUILD_SECONDS_SINCE_UNIX_EPOCH={:?}",
        Duration::from_secs(u64::from_str_radix(BUILD_SECONDS_SINCE_UNIX_EPOCH, 10).unwrap())
    );
    println!("{:?}", matches);
    Ok(())
}
