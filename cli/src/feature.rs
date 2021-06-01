use crate::{
    cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    spend_utils::{resolve_spend_tx_and_check_account_balance, SpendAmount},
};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use console::style;
use serde::{Deserialize, Serialize};
use solana_clap_utils::{input_parsers::*, input_validators::*, keypair::*};
use solana_cli_output::{QuietDisplay, VerboseDisplay};
use solana_client::{client_error::ClientError, rpc_client::RpcClient};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{
    account::Account,
    clock::Slot,
    feature::{self, Feature},
    feature_set::FEATURE_NAMES,
    message::Message,
    pubkey::Pubkey,
    transaction::Transaction,
};
use std::{collections::HashMap, fmt, sync::Arc};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ForceActivation {
    No,
    Almost,
    Yes,
}

#[derive(Debug, PartialEq)]
pub enum FeatureCliCommand {
    Status {
        features: Vec<Pubkey>,
    },
    Activate {
        feature: Pubkey,
        force: ForceActivation,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status", content = "sinceSlot")]
pub enum CliFeatureStatus {
    Inactive,
    Pending,
    Active(Slot),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliFeature {
    pub id: String,
    pub description: String,
    #[serde(flatten)]
    pub status: CliFeatureStatus,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliFeatures {
    pub features: Vec<CliFeature>,
    pub feature_activation_allowed: bool,
    #[serde(skip)]
    pub inactive: bool,
}

impl fmt::Display for CliFeatures {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.features.len() > 1 {
            writeln!(
                f,
                "{}",
                style(format!(
                    "{:<44} | {:<27} | {}",
                    "Feature", "Status", "Description"
                ))
                .bold()
            )?;
        }
        for feature in &self.features {
            writeln!(
                f,
                "{:<44} | {:<27} | {}",
                feature.id,
                match feature.status {
                    CliFeatureStatus::Inactive => style("inactive".to_string()).red(),
                    CliFeatureStatus::Pending => style("activation pending".to_string()).yellow(),
                    CliFeatureStatus::Active(activation_slot) =>
                        style(format!("active since slot {}", activation_slot)).green(),
                },
                feature.description,
            )?;
        }
        if self.inactive && !self.feature_activation_allowed {
            writeln!(
                f,
                "{}",
                style("\nFeature activation is not allowed at this time")
                    .bold()
                    .red()
            )?;
        }
        Ok(())
    }
}

impl QuietDisplay for CliFeatures {}
impl VerboseDisplay for CliFeatures {}

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
                        )
                        .arg(
                            Arg::with_name("force")
                                .long("yolo")
                                .hidden(true)
                                .multiple(true)
                                .help("Override activation sanity checks. Don't use this flag"),
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

            let force = match matches.occurrences_of("force") {
                2 => ForceActivation::Yes,
                1 => ForceActivation::Almost,
                _ => ForceActivation::No,
            };

            signers.push(feature_signer.unwrap());
            let feature = feature.unwrap();

            known_feature(&feature)?;

            CliCommandInfo {
                command: CliCommand::Feature(FeatureCliCommand::Activate { feature, force }),
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
        FeatureCliCommand::Status { features } => process_status(rpc_client, config, features),
        FeatureCliCommand::Activate { feature, force } => {
            process_activate(rpc_client, config, *feature, *force)
        }
    }
}

fn active_stake_by_feature_set(rpc_client: &RpcClient) -> Result<HashMap<u32, f64>, ClientError> {
    // Validator identity -> feature set
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

    // Sum all active stake by feature set
    let mut active_stake_by_feature_set: HashMap<u32, u64> = HashMap::new();
    for vote_account in vote_accounts.current {
        if let Some(Some(feature_set)) = feature_set_map.get(&vote_account.node_pubkey) {
            *active_stake_by_feature_set.entry(*feature_set).or_default() +=
                vote_account.activated_stake;
        } else {
            *active_stake_by_feature_set
                .entry(0 /* "unknown" */)
                .or_default() += vote_account.activated_stake;
        }
    }

    Ok(active_stake_by_feature_set
        .into_iter()
        .map(|(feature_set, active_stake)| {
            (
                feature_set,
                active_stake as f64 * 100. / total_active_stake as f64,
            )
        })
        .collect())
}

// Feature activation is only allowed when 95% of the active stake is on the current feature set
fn feature_activation_allowed(rpc_client: &RpcClient, quiet: bool) -> Result<bool, ClientError> {
    let my_feature_set = solana_version::Version::default().feature_set;

    let active_stake_by_feature_set = active_stake_by_feature_set(rpc_client)?;

    let feature_activation_allowed = active_stake_by_feature_set
        .get(&my_feature_set)
        .map(|percentage| *percentage >= 95.)
        .unwrap_or(false);

    if !feature_activation_allowed && !quiet {
        if active_stake_by_feature_set.get(&my_feature_set).is_none() {
            println!(
                "{}",
                style("To activate features the tool and cluster feature sets must match, select a tool version that matches the cluster")
                    .bold());
        } else {
            println!(
                "{}",
                style("To activate features the stake must be >= 95%").bold()
            );
        }
        println!(
            "{}",
            style(format!("Tool Feature Set: {}", my_feature_set)).bold()
        );
        println!("{}", style("Cluster Feature Sets and Stakes:").bold());
        for (feature_set, percentage) in active_stake_by_feature_set.iter() {
            if *feature_set == 0 {
                println!("  unknown    - {:.2}%", percentage);
            } else {
                println!(
                    "  {:<10} - {:.2}% {}",
                    feature_set,
                    percentage,
                    if *feature_set == my_feature_set {
                        " <-- me"
                    } else {
                        ""
                    }
                );
            }
        }
        println!();
    }

    Ok(feature_activation_allowed)
}

fn status_from_account(account: Account) -> Option<CliFeatureStatus> {
    feature::from_account(&account).map(|feature| match feature.activated_at {
        None => CliFeatureStatus::Pending,
        Some(activation_slot) => CliFeatureStatus::Active(activation_slot),
    })
}

fn get_feature_status(
    rpc_client: &RpcClient,
    feature_id: &Pubkey,
) -> Result<Option<CliFeatureStatus>, Box<dyn std::error::Error>> {
    rpc_client
        .get_account(feature_id)
        .map(status_from_account)
        .map_err(|e| e.into())
}

pub fn get_feature_is_active(
    rpc_client: &RpcClient,
    feature_id: &Pubkey,
) -> Result<bool, Box<dyn std::error::Error>> {
    get_feature_status(rpc_client, feature_id)
        .map(|status| matches!(status, Some(CliFeatureStatus::Active(_))))
}

fn process_status(
    rpc_client: &RpcClient,
    config: &CliConfig,
    feature_ids: &[Pubkey],
) -> ProcessResult {
    let mut features: Vec<CliFeature> = vec![];
    let mut inactive = false;
    for (i, account) in rpc_client
        .get_multiple_accounts(feature_ids)?
        .into_iter()
        .enumerate()
    {
        let feature_id = &feature_ids[i];
        let feature_name = FEATURE_NAMES.get(feature_id).unwrap();
        if let Some(account) = account {
            if let Some(feature_status) = status_from_account(account) {
                features.push(CliFeature {
                    id: feature_id.to_string(),
                    description: feature_name.to_string(),
                    status: feature_status,
                });
                continue;
            }
        }
        inactive = true;
        features.push(CliFeature {
            id: feature_id.to_string(),
            description: feature_name.to_string(),
            status: CliFeatureStatus::Inactive,
        });
    }

    let feature_activation_allowed = feature_activation_allowed(rpc_client, features.len() <= 1)?;
    let feature_set = CliFeatures {
        features,
        feature_activation_allowed,
        inactive,
    };
    Ok(config.output_format.formatted_string(&feature_set))
}

fn process_activate(
    rpc_client: &RpcClient,
    config: &CliConfig,
    feature_id: Pubkey,
    force: ForceActivation,
) -> ProcessResult {
    let account = rpc_client
        .get_multiple_accounts(&[feature_id])?
        .into_iter()
        .next()
        .unwrap();

    if let Some(account) = account {
        if feature::from_account(&account).is_some() {
            return Err(format!("{} has already been activated", feature_id).into());
        }
    }

    if !feature_activation_allowed(rpc_client, false)? {
        match force {
        ForceActivation::Almost =>
            return Err("Add force argument once more to override the sanity check to force feature activation ".into()),
        ForceActivation::Yes => println!("FEATURE ACTIVATION FORCED"),
        ForceActivation::No =>
            return Err("Feature activation is not allowed at this time".into()),
        }
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
                &feature::activate_with_lamports(
                    &feature_id,
                    &config.signers[0].pubkey(),
                    lamports,
                ),
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
