#![allow(clippy::integer_arithmetic)]
use {
    clap::{crate_description, crate_name, App, AppSettings},
    log::*,
    solana_measure::measure::Measure,
    std::process::exit,
};

#[allow(clippy::cognitive_complexity)]
fn main() {
    solana_logger::setup_with_default("solana=info");

    let mut measure_total_execution_time = Measure::start("accounts tool");
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .setting(AppSettings::InferSubcommands)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::VersionlessSubcommands)
        .get_matches();
    match matches.subcommand() {
        ("", _) => {
            eprintln!("{}", matches.usage());
            measure_total_execution_time.stop();
            info!("{}", measure_total_execution_time);
            exit(1);
        }
        _ => unreachable!(),
    };
}
