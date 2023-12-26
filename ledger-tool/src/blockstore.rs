//! The `blockstore` subcommand

use {
    clap::{App, AppSettings, ArgMatches, SubCommand},
    std::path::Path,
};

pub trait BlockstoreSubCommand {
    fn blockstore_subcommand(self) -> Self;
}

impl BlockstoreSubCommand for App<'_, '_> {
    fn blockstore_subcommand(self) -> Self {
        self.subcommand(
            SubCommand::with_name("blockstore")
                .about("Commands to interact with a local Blockstore")
                .setting(AppSettings::InferSubcommands)
                .setting(AppSettings::SubcommandRequiredElseHelp),
        )
    }
}

pub fn blockstore_process_command(ledger_path: &Path, matches: &ArgMatches<'_>) {
    match matches.subcommand() {
        _ => unreachable!(),
    }
}
