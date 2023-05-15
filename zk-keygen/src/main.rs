use {
    clap::{crate_description, crate_name, Arg, ArgMatches, Command},
    solana_clap_v3_utils::DisplayError,
    solana_cli_config::CONFIG_FILE,
    std::error,
};

fn app(crate_version: &str) -> Command {
    Command::new(crate_name!())
        .about(crate_description!())
        .version(crate_version)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg({
            let arg = Arg::new("config_file")
                .short('C')
                .long("config")
                .value_name("FILEPATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *CONFIG_FILE {
                arg.default_value(config_file)
            } else {
                arg
            }
        })
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = app(solana_version::version!())
        .try_get_matches()
        .unwrap_or_else(|e| e.exit());
    do_main(&matches).map_err(|err| DisplayError::new_as_boxed(err).into())
}

fn do_main(_matches: &ArgMatches) -> Result<(), Box<dyn error::Error>> {
    unimplemented!()
}
