use crate::cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult};
use clap::{App, ArgMatches, SubCommand};
use console::style;
use solana_clap_utils::keypair::*;
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
    _config: &CliConfig,
    inflation_subcommand: &InflationCliCommand,
) -> ProcessResult {
    assert_eq!(*inflation_subcommand, InflationCliCommand::Show);

    let governor = rpc_client.get_inflation_governor()?;
    let current_inflation_rate = rpc_client.get_inflation_rate()?;

    println!("{}", style("Inflation Governor:").bold());
    if (governor.initial - governor.terminal).abs() < f64::EPSILON {
        println!(
            "Fixed APR:               {:>5.2}%",
            governor.terminal * 100.
        );
    } else {
        println!("Initial APR:             {:>5.2}%", governor.initial * 100.);
        println!(
            "Terminal APR:            {:>5.2}%",
            governor.terminal * 100.
        );
        println!("Rate reduction per year: {:>5.2}%", governor.taper * 100.);
    }
    if governor.foundation_term > 0. {
        println!("Foundation percentage:   {:>5.2}%", governor.foundation);
        println!(
            "Foundation term:         {:.1} years",
            governor.foundation_term
        );
    }

    println!(
        "\n{}",
        style(format!(
            "Inflation for Epoch {}:",
            current_inflation_rate.epoch
        ))
        .bold()
    );
    println!(
        "Total APR:               {:>5.2}%",
        current_inflation_rate.total * 100.
    );
    println!(
        "Staking APR:             {:>5.2}%",
        current_inflation_rate.validator * 100.
    );
    println!(
        "Foundation APR:          {:>5.2}%",
        current_inflation_rate.foundation * 100.
    );

    Ok("".to_string())
}
