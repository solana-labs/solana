use crate::{
    cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    spend_utils::{resolve_spend_tx_and_check_account_balance, SpendAmount},
};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use console::style;
use solana_clap_utils::{input_parsers::*, input_validators::*, keypair::*};
use solana_client::{client_error::ClientError, rpc_client::RpcClient};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_runtime::{
    feature::{self, Feature},
    feature_set::FEATURE_NAMES,
};
use solana_sdk::{message::Message, pubkey::Pubkey, system_instruction, transaction::Transaction};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum FeatureCliCommand {
    Status { features: Vec<Pubkey> },
    Activate { feature: Pubkey },
}

pub trait FeatureSubCommands {
    fn feature_subcommands(self) -> Self;
}

impl FeatureSubCommands for App<'_, '_> {
    fn feature_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("feature")
                .about("Runtime feature management")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("status")
                        .about("Query runtime feature status")
                        .arg(
                            Arg::with_name("features")
                                .value_name("ADDRESS")
                                .validator(is_valid_pubkey)
                                .index(1)
                                .multiple(true)
                                .help("Feature status to query [default: all known features]"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("activate")
                        .about("Activate a runtime feature")
                        .arg(
                            Arg::with_name("feature")
                                .value_name("FEATURE_KEYPAIR")
                                .validator(is_valid_signer)
                                .index(1)
                                .required(true)
                                .help("The signer for the feature to activate"),
                        ),
                ),
        )
    }
}

fn known_feature(feature: &Pubkey) -> Result<(), CliError> {
    if FEATURE_NAMES.contains_key(feature) {
        Ok(())
    } else {
        Err(CliError::BadParameter(format!(
            "Unknown feature: {}",
            feature
        )))
    }
}

pub fn parse_feature_subcommand(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let response = match matches.subcommand() {
        ("activate", Some(matches)) => {
            let (feature_signer, feature) = signer_of(matches, "feature", wallet_manager)?;
            let mut signers = vec![default_signer.signer_from_path(matches, wallet_manager)?];
            signers.push(feature_signer.unwrap());
            let feature = feature.unwrap();

            known_feature(&feature)?;

            CliCommandInfo {
                command: CliCommand::Feature(FeatureCliCommand::Activate { feature }),
                signers,
            }
        }
        ("status", Some(matches)) => {
            let mut features = if let Some(features) = pubkeys_of(matches, "features") {
                for feature in &features {
                    known_feature(feature)?;
                }
                features
            } else {
                FEATURE_NAMES.keys().cloned().collect()
            };
            features.sort();
            CliCommandInfo {
                command: CliCommand::Feature(FeatureCliCommand::Status { features }),
                signers: vec![],
            }
        }
        _ => unreachable!(),
    };
    Ok(response)
}

pub fn process_feature_subcommand(
    rpc_client: &RpcClient,
    config: &CliConfig,
    feature_subcommand: &FeatureCliCommand,
) -> ProcessResult {
    match feature_subcommand {
        FeatureCliCommand::Status { features } => process_status(rpc_client, features),
        FeatureCliCommand::Activate { feature } => process_activate(rpc_client, config, *feature),
    }
}

// Feature activation is only allowed when 95% of the active stake is on the current feature set
fn feature_activation_allowed(rpc_client: &RpcClient) -> Result<bool, ClientError> {
    let my_feature_set = solana_version::Version::default().feature_set;

    let feature_set_map = rpc_client
        .get_cluster_nodes()?
        .into_iter()
        .map(|contact_info| (contact_info.pubkey, contact_info.feature_set))
        .collect::<HashMap<_, _>>();

    let vote_accounts = rpc_client.get_vote_accounts()?;

    let total_active_stake: u64 = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .map(|vote_account| vote_account.activated_stake)
        .sum();

    let total_compatible_stake: u64 = vote_accounts
        .current
        .iter()
        .map(|vote_account| {
            if Some(&Some(my_feature_set)) == feature_set_map.get(&vote_account.node_pubkey) {
                vote_account.activated_stake
            } else {
                0
            }
        })
        .sum();

    Ok(total_compatible_stake * 100 / total_active_stake >= 95)
}

fn process_status(rpc_client: &RpcClient, feature_ids: &[Pubkey]) -> ProcessResult {
    if feature_ids.len() > 1 {
        println!(
            "{}",
            style(format!(
                "{:<44} {:<40} {}",
                "Feature", "Description", "Status"
            ))
            .bold()
        );
    }

    let mut inactive = false;
    for (i, account) in rpc_client
        .get_multiple_accounts(feature_ids)?
        .into_iter()
        .enumerate()
    {
        let feature_id = &feature_ids[i];
        let feature_name = FEATURE_NAMES.get(feature_id).unwrap();
        if let Some(account) = account {
            if let Some(feature) = Feature::from_account(&account) {
                match feature.activated_at {
                    None => println!(
                        "{:<44} {:<40} {}",
                        feature_id,
                        feature_name,
                        style("activation pending").yellow()
                    ),
                    Some(activation_slot) => {
                        println!(
                            "{:<44} {:<40} {}",
                            feature_id,
                            feature_name,
                            style(format!("active since slot {}", activation_slot)).green()
                        );
                    }
                }
                continue;
            }
        }
        inactive = true;
        println!(
            "{:<44} {:<40} {}",
            feature_id,
            feature_name,
            style("inactive").red()
        );
    }

    if inactive && !feature_activation_allowed(rpc_client)? {
        println!(
            "{}",
            style("\nFeature activation is not allowed at this time")
                .bold()
                .red()
        );
    }
    Ok("".to_string())
}

fn process_activate(
    rpc_client: &RpcClient,
    config: &CliConfig,
    feature_id: Pubkey,
) -> ProcessResult {
    let account = rpc_client
        .get_multiple_accounts(&[feature_id])?
        .into_iter()
        .next()
        .unwrap();
    if let Some(account) = account {
        if Feature::from_account(&account).is_some() {
            return Err(format!("{} has already been activated", feature_id).into());
        }
    }

    if !feature_activation_allowed(rpc_client)? {
        return Err("Feature activation is not allowed at this time".into());
    }

    let rent = rpc_client.get_minimum_balance_for_rent_exemption(Feature::size_of())?;

    let (blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let (message, _) = resolve_spend_tx_and_check_account_balance(
        rpc_client,
        false,
        SpendAmount::Some(rent),
        &fee_calculator,
        &config.signers[0].pubkey(),
        |lamports| {
            Message::new(
                &[
                    system_instruction::transfer(
                        &config.signers[0].pubkey(),
                        &feature_id,
                        lamports,
                    ),
                    system_instruction::allocate(&feature_id, Feature::size_of() as u64),
                    system_instruction::assign(&feature_id, &feature::id()),
                ],
                Some(&config.signers[0].pubkey()),
            )
        },
        config.commitment,
    )?;
    let mut transaction = Transaction::new_unsigned(message);
    transaction.try_sign(&config.signers, blockhash)?;

    println!(
        "Activating {} ({})",
        FEATURE_NAMES.get(&feature_id).unwrap(),
        feature_id
    );
    rpc_client.send_and_confirm_transaction_with_spinner(&transaction)?;
    Ok("".to_string())
}
