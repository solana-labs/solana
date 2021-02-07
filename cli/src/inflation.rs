use crate::cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult};
use clap::{App, ArgMatches, SubCommand};
use solana_clap_utils::keypair::*;
use solana_cli_output::CliInflation;
use solana_client::rpc_client::RpcClient;
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use std::sync::Arc;

#[derive(Debug, PartialEq)]
pub enum InflationCliCommand {
    Show,
}

pub trait InflationSubCommands {
    fn inflation_subcommands(self) -> Self;
}

impl InflationSubCommands for App<'_, '_> {
    fn inflation_subcommands(self) -> Self {
        self.subcommand(SubCommand::with_name("inflation").about("Show inflation information"))
    }
}

pub fn parse_inflation_subcommand(
    _matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    Ok(CliCommandInfo {
        command: CliCommand::Inflation(InflationCliCommand::Show),
        signers: vec![],
    })
}

pub fn process_inflation_subcommand(
    rpc_client: &RpcClient,
    config: &CliConfig,
    inflation_subcommand: &InflationCliCommand,
) -> ProcessResult {
    assert_eq!(*inflation_subcommand, InflationCliCommand::Show);

    let governor = rpc_client.get_inflation_governor()?;
    let current_rate = rpc_client.get_inflation_rate()?;

    let inflation = CliInflation {
        governor,
        current_rate,
    };

    Ok(config.output_format.formatted_string(&inflation))
}
