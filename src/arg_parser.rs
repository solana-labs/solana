use crate::args::{Args, Command, ScrubArgs, TransferArgs};
use clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand};
//use solana_clap_utils::input_validators::{is_valid_pubkey, is_valid_signer};
use solana_cli_config::CONFIG_FILE;
use std::ffi::OsString;
use std::process::exit;

//fn fee_payer_arg<'a, 'b>() -> Arg<'a, 'b> {
//    Arg::with_name("fee_payer")
//        .long("fee-payer")
//        .required(true)
//        .takes_value(true)
//        .value_name("KEYPAIR")
//        .validator(is_valid_signer)
//        .help("Fee payer")
//}
//
//fn funding_keypair_arg<'a, 'b>() -> Arg<'a, 'b> {
//    Arg::with_name("funding_keypair")
//        .required(true)
//        .takes_value(true)
//        .value_name("FUNDING_KEYPAIR")
//        .validator(is_valid_signer)
//        .help("Keypair to fund accounts")
//}

pub(crate) fn get_matches<'a, I, T>(args: I) -> ArgMatches<'a>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let default_config_file = CONFIG_FILE.as_ref().unwrap();
    App::new("solana-stake-accounts")
        .about("about")
        .version("version")
        .arg(
            Arg::with_name("config_file")
                .long("config")
                .takes_value(true)
                .value_name("FILEPATH")
                .default_value(default_config_file)
                .help("Config file"),
        )
        .arg(
            Arg::with_name("url")
                .long("url")
                .global(true)
                .takes_value(true)
                .value_name("URL")
                .help("RPC entrypoint address. i.e. http://devnet.solana.com"),
        )
        .subcommand(
            SubCommand::with_name("scrub")
                .about("Scrub a data file")
                .arg(
                    Arg::with_name("input_csv")
                        .required(true)
                        .index(1)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Input CSV file"),
                ),
        )
        .subcommand(
            SubCommand::with_name("transfer")
                .about("Scrub a data file")
                .arg(
                    Arg::with_name("input_csv")
                        .required(true)
                        .index(1)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("Scrubbed CSV file"),
                )
                .arg(
                    Arg::with_name("state_csv")
                        .required(true)
                        .index(2)
                        .takes_value(true)
                        .value_name("FILE")
                        .help("State CSV file"),
                )
                .arg(
                    Arg::with_name("dry_run")
                        .long("dry-run")
                        .help("Do not execute any transfers"),
                ),
        )
        .get_matches_from(args)
}

fn parse_scrub_args(matches: &ArgMatches<'_>) -> ScrubArgs {
    ScrubArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
    }
}

fn parse_transfer_args(matches: &ArgMatches<'_>) -> TransferArgs {
    TransferArgs {
        input_csv: value_t_or_exit!(matches, "input_csv", String),
        state_csv: value_t_or_exit!(matches, "state_csv", String),
        dry_run: matches.is_present("dry_run"),
    }
}

pub(crate) fn parse_args<I, T>(args: I) -> Args
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = get_matches(args);
    let config_file = matches.value_of("config_file").unwrap().to_string();
    let url = matches.value_of("url").map(|x| x.to_string());

    let command = match matches.subcommand() {
        ("scrub", Some(matches)) => Command::Scrub(parse_scrub_args(matches)),
        ("transfer", Some(matches)) => Command::Transfer(parse_transfer_args(matches)),
        _ => {
            eprintln!("{}", matches.usage());
            exit(1);
        }
    };
    Args {
        config_file,
        url,
        command,
    }
}
