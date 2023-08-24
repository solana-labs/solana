use {
    crate::cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    clap::{App, Arg, ArgMatches, SubCommand},
    solana_clap_utils::{
        input_parsers::{pubkeys_of, value_of},
        input_validators::is_valid_pubkey,
        keypair::*,
    },
    solana_cli_output::{
        CliEpochRewardshMetadata, CliInflation, CliKeyedEpochReward, CliKeyedEpochRewards,
    },
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{clock::Epoch, pubkey::Pubkey},
    std::rc::Rc,
};

#[derive(Debug, PartialEq, Eq)]
pub enum InflationCliCommand {
    Show,
    Rewards(Vec<Pubkey>, Option<Epoch>),
}

pub trait InflationSubCommands {
    fn inflation_subcommands(self) -> Self;
}

impl InflationSubCommands for App<'_, '_> {
    fn inflation_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("inflation")
                .about("Show inflation information")
                .subcommand(
                    SubCommand::with_name("rewards")
                        .about("Show inflation rewards for a set of addresses")
                        .arg(pubkey!(
                            Arg::with_name("addresses")
                                .value_name("ADDRESS")
                                .index(1)
                                .multiple(true)
                                .required(true),
                            "Address of account to query for rewards. "
                        ))
                        .arg(
                            Arg::with_name("rewards_epoch")
                                .long("rewards-epoch")
                                .takes_value(true)
                                .value_name("EPOCH")
                                .help("Display rewards for specific epoch [default: latest epoch]"),
                        ),
                ),
        )
    }
}

pub fn parse_inflation_subcommand(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let command = match matches.subcommand() {
        ("rewards", Some(matches)) => {
            let addresses = pubkeys_of(matches, "addresses").unwrap();
            let rewards_epoch = value_of(matches, "rewards_epoch");
            InflationCliCommand::Rewards(addresses, rewards_epoch)
        }
        _ => InflationCliCommand::Show,
    };
    Ok(CliCommandInfo {
        command: CliCommand::Inflation(command),
        signers: vec![],
    })
}

pub fn process_inflation_subcommand(
    rpc_client: &RpcClient,
    config: &CliConfig,
    inflation_subcommand: &InflationCliCommand,
) -> ProcessResult {
    match inflation_subcommand {
        InflationCliCommand::Show => process_show(rpc_client, config),
        InflationCliCommand::Rewards(ref addresses, rewards_epoch) => {
            process_rewards(rpc_client, config, addresses, *rewards_epoch)
        }
    }
}

fn process_show(rpc_client: &RpcClient, config: &CliConfig) -> ProcessResult {
    let governor = rpc_client.get_inflation_governor()?;
    let current_rate = rpc_client.get_inflation_rate()?;

    let inflation = CliInflation {
        governor,
        current_rate,
    };

    Ok(config.output_format.formatted_string(&inflation))
}

fn process_rewards(
    rpc_client: &RpcClient,
    config: &CliConfig,
    addresses: &[Pubkey],
    rewards_epoch: Option<Epoch>,
) -> ProcessResult {
    let rewards = rpc_client
        .get_inflation_reward(addresses, rewards_epoch)
        .map_err(|err| {
            if let Some(epoch) = rewards_epoch {
                format!("Rewards not available for epoch {epoch}")
            } else {
                format!("Rewards not available {err}")
            }
        })?;
    let epoch_schedule = rpc_client.get_epoch_schedule()?;

    let mut epoch_rewards: Vec<CliKeyedEpochReward> = vec![];
    let epoch_metadata = if let Some(Some(first_reward)) = rewards.iter().find(|&v| v.is_some()) {
        let (epoch_start_time, epoch_end_time) =
            crate::stake::get_epoch_boundary_timestamps(rpc_client, first_reward, &epoch_schedule)?;
        for (reward, address) in rewards.iter().zip(addresses) {
            let cli_reward = reward.as_ref().and_then(|reward| {
                crate::stake::make_cli_reward(reward, epoch_start_time, epoch_end_time)
            });
            epoch_rewards.push(CliKeyedEpochReward {
                address: address.to_string(),
                reward: cli_reward,
            });
        }
        let block_time = rpc_client.get_block_time(first_reward.effective_slot)?;
        Some(CliEpochRewardshMetadata {
            epoch: first_reward.epoch,
            effective_slot: first_reward.effective_slot,
            block_time,
        })
    } else {
        None
    };
    let cli_rewards = CliKeyedEpochRewards {
        epoch_metadata,
        rewards: epoch_rewards,
    };
    Ok(config.output_format.formatted_string(&cli_rewards))
}
