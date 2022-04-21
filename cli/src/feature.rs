use {
    crate::{
        cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
        spend_utils::{resolve_spend_tx_and_check_account_balance, SpendAmount},
    },
    clap::{App, AppSettings, Arg, ArgMatches, SubCommand},
    console::style,
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    solana_clap_utils::{input_parsers::*, input_validators::*, keypair::*},
    solana_cli_output::{QuietDisplay, VerboseDisplay},
    solana_client::{client_error::ClientError, rpc_client::RpcClient},
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_sdk::{
        account::Account,
        clock::Slot,
        feature::{self, Feature},
        feature_set::FEATURE_NAMES,
        message::Message,
        pubkey::Pubkey,
        transaction::Transaction,
    },
    std::{
        cmp::Ordering,
        collections::{HashMap, HashSet},
        fmt,
        str::FromStr,
        sync::Arc,
    },
};

const DEFAULT_MAX_ACTIVE_DISPLAY_AGE_SLOTS: Slot = 15_000_000; // ~90days

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
        display_all: bool,
    },
    Activate {
        feature: Pubkey,
        force: ForceActivation,
    },
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "status", content = "sinceSlot")]
pub enum CliFeatureStatus {
    Inactive,
    Pending,
    Active(Slot),
}

impl PartialOrd for CliFeatureStatus {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CliFeatureStatus {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Inactive, Self::Inactive) => Ordering::Equal,
            (Self::Inactive, _) => Ordering::Greater,
            (_, Self::Inactive) => Ordering::Less,
            (Self::Pending, Self::Pending) => Ordering::Equal,
            (Self::Pending, _) => Ordering::Greater,
            (_, Self::Pending) => Ordering::Less,
            (Self::Active(self_active_slot), Self::Active(other_active_slot)) => {
                self_active_slot.cmp(other_active_slot)
            }
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliFeature {
    pub id: String,
    pub description: String,
    #[serde(flatten)]
    pub status: CliFeatureStatus,
}

impl PartialOrd for CliFeature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CliFeature {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.status.cmp(&other.status) {
            Ordering::Equal => self.id.cmp(&other.id),
            ordering => ordering,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliFeatures {
    pub features: Vec<CliFeature>,
    pub feature_activation_allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_feature_sets: Option<CliClusterFeatureSets>,
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
                        style(format!("active since slot {:>9}", activation_slot)).green(),
                },
                feature.description,
            )?;
        }

        if let Some(feature_sets) = &self.cluster_feature_sets {
            write!(f, "{}", feature_sets)?;
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliClusterFeatureSets {
    pub tool_feature_set: u32,
    pub feature_sets: Vec<CliFeatureSet>,
    #[serde(skip)]
    pub stake_allowed: bool,
    #[serde(skip)]
    pub rpc_allowed: bool,
}

impl fmt::Display for CliClusterFeatureSets {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut tool_feature_set_matches_cluster = false;

        let software_versions_title = "Software Version";
        let feature_set_title = "Feature Set";
        let stake_percent_title = "Stake";
        let rpc_percent_title = "RPC";
        let mut max_software_versions_len = software_versions_title.len();
        let mut max_feature_set_len = feature_set_title.len();
        let mut max_stake_percent_len = stake_percent_title.len();
        let mut max_rpc_percent_len = rpc_percent_title.len();

        let feature_sets: Vec<_> = self
            .feature_sets
            .iter()
            .map(|feature_set_info| {
                let me = if self.tool_feature_set == feature_set_info.feature_set {
                    tool_feature_set_matches_cluster = true;
                    true
                } else {
                    false
                };
                let software_versions: Vec<_> = feature_set_info
                    .software_versions
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                let software_versions = software_versions.join(", ");
                let feature_set = if feature_set_info.feature_set == 0 {
                    "unknown".to_string()
                } else {
                    feature_set_info.feature_set.to_string()
                };
                let stake_percent = format!("{:.2}%", feature_set_info.stake_percent);
                let rpc_percent = format!("{:.2}%", feature_set_info.rpc_percent);

                max_software_versions_len = max_software_versions_len.max(software_versions.len());
                max_feature_set_len = max_feature_set_len.max(feature_set.len());
                max_stake_percent_len = max_stake_percent_len.max(stake_percent.len());
                max_rpc_percent_len = max_rpc_percent_len.max(rpc_percent.len());

                (
                    software_versions,
                    feature_set,
                    stake_percent,
                    rpc_percent,
                    me,
                )
            })
            .collect();

        if !tool_feature_set_matches_cluster {
            writeln!(
                f,
                "\n{}",
                style("To activate features the tool and cluster feature sets must match, select a tool version that matches the cluster")
                    .bold())?;
        } else {
            if !self.stake_allowed {
                write!(
                    f,
                    "\n{}",
                    style("To activate features the stake must be >= 95%")
                        .bold()
                        .red()
                )?;
            }
            if !self.rpc_allowed {
                write!(
                    f,
                    "\n{}",
                    style("To activate features the RPC nodes must be >= 95%")
                        .bold()
                        .red()
                )?;
            }
        }
        writeln!(
            f,
            "\n\n{}",
            style(format!("Tool Feature Set: {}", self.tool_feature_set)).bold()
        )?;
        writeln!(
            f,
            "{}",
            style(format!(
                "{1:<0$}  {3:<2$}  {5:<4$}  {7:<6$}",
                max_software_versions_len,
                software_versions_title,
                max_feature_set_len,
                feature_set_title,
                max_stake_percent_len,
                stake_percent_title,
                max_rpc_percent_len,
                rpc_percent_title,
            ))
            .bold(),
        )?;
        for (software_versions, feature_set, stake_percent, rpc_percent, me) in feature_sets {
            writeln!(
                f,
                "{1:<0$}  {3:>2$}  {5:>4$} {7:>6$}  {8}",
                max_software_versions_len,
                software_versions,
                max_feature_set_len,
                feature_set,
                max_stake_percent_len,
                stake_percent,
                max_rpc_percent_len,
                rpc_percent,
                if me { "<-- me" } else { "" },
            )?;
        }
        writeln!(f)
    }
}

impl QuietDisplay for CliClusterFeatureSets {}
impl VerboseDisplay for CliClusterFeatureSets {}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliFeatureSet {
    software_versions: Vec<CliVersion>,
    feature_set: u32,
    stake_percent: f64,
    rpc_percent: f32,
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct CliVersion(Option<semver::Version>);

impl fmt::Display for CliVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match &self.0 {
            None => "unknown".to_string(),
            Some(version) => version.to_string(),
        };
        write!(f, "{}", s)
    }
}

impl FromStr for CliVersion {
    type Err = semver::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let version_option = if s == "unknown" {
            None
        } else {
            Some(semver::Version::from_str(s)?)
        };
        Ok(CliVersion(version_option))
    }
}

impl Serialize for CliVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CliVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        CliVersion::from_str(s).map_err(serde::de::Error::custom)
    }
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
                        )
                        .arg(
                            Arg::with_name("display_all")
                                .long("display-all")
                                .help("display all features regardless of age"),
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
            let display_all = matches.is_present("display_all");
            features.sort();
            CliCommandInfo {
                command: CliCommand::Feature(FeatureCliCommand::Status {
                    features,
                    display_all,
                }),
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
        FeatureCliCommand::Status {
            features,
            display_all,
        } => process_status(rpc_client, config, features, *display_all),
        FeatureCliCommand::Activate { feature, force } => {
            process_activate(rpc_client, config, *feature, *force)
        }
    }
}

#[derive(Debug, Default)]
struct WorkingFeatureSetStatsEntry {
    stake: u64,
    rpc_nodes_count: u32,
    software_versions: HashSet<Option<semver::Version>>,
}
type WorkingFeatureSetStats = HashMap<u32, WorkingFeatureSetStatsEntry>;

#[derive(Debug, Default)]
struct FeatureSetStatsEntry {
    stake_percent: f64,
    rpc_nodes_percent: f32,
    software_versions: Vec<Option<semver::Version>>,
}
type FeatureSetStats = HashMap<u32, FeatureSetStatsEntry>;

fn feature_set_stats(rpc_client: &RpcClient) -> Result<FeatureSetStats, ClientError> {
    // Validator identity -> feature set
    let feature_sets = rpc_client
        .get_cluster_nodes()?
        .into_iter()
        .map(|contact_info| {
            (
                contact_info.pubkey,
                contact_info.feature_set,
                contact_info.rpc.is_some(),
                contact_info
                    .version
                    .and_then(|v| semver::Version::parse(&v).ok()),
            )
        })
        .collect::<Vec<_>>();

    let vote_accounts = rpc_client.get_vote_accounts()?;

    let mut total_active_stake: u64 = vote_accounts
        .delinquent
        .iter()
        .map(|vote_account| vote_account.activated_stake)
        .sum();

    let vote_stakes = vote_accounts
        .current
        .into_iter()
        .map(|vote_account| {
            total_active_stake += vote_account.activated_stake;
            (vote_account.node_pubkey, vote_account.activated_stake)
        })
        .collect::<HashMap<_, _>>();

    let mut feature_set_stats: WorkingFeatureSetStats = HashMap::new();
    let mut total_rpc_nodes = 0;
    for (node_id, feature_set, is_rpc, version) in feature_sets {
        let feature_set = feature_set.unwrap_or(0);
        let feature_set_entry = feature_set_stats.entry(feature_set).or_default();

        feature_set_entry.software_versions.insert(version);

        if let Some(vote_stake) = vote_stakes.get(&node_id) {
            feature_set_entry.stake += *vote_stake;
        }

        if is_rpc {
            feature_set_entry.rpc_nodes_count += 1;
            total_rpc_nodes += 1;
        }
    }

    Ok(feature_set_stats
        .into_iter()
        .filter_map(
            |(
                feature_set,
                WorkingFeatureSetStatsEntry {
                    stake,
                    rpc_nodes_count,
                    software_versions,
                },
            )| {
                let stake_percent = (stake as f64 / total_active_stake as f64) * 100.;
                let rpc_nodes_percent = (rpc_nodes_count as f32 / total_rpc_nodes as f32) * 100.;
                let mut software_versions = software_versions.into_iter().collect::<Vec<_>>();
                software_versions.sort();
                if stake_percent >= 0.001 || rpc_nodes_percent >= 0.001 {
                    Some((
                        feature_set,
                        FeatureSetStatsEntry {
                            stake_percent,
                            rpc_nodes_percent,
                            software_versions,
                        },
                    ))
                } else {
                    None
                }
            },
        )
        .collect())
}

// Feature activation is only allowed when 95% of the active stake is on the current feature set
fn feature_activation_allowed(
    rpc_client: &RpcClient,
    quiet: bool,
) -> Result<(bool, Option<CliClusterFeatureSets>), ClientError> {
    let my_feature_set = solana_version::Version::default().feature_set;

    let feature_set_stats = feature_set_stats(rpc_client)?;

    let (stake_allowed, rpc_allowed) = feature_set_stats
        .get(&my_feature_set)
        .map(
            |FeatureSetStatsEntry {
                 stake_percent,
                 rpc_nodes_percent,
                 ..
             }| (*stake_percent >= 95., *rpc_nodes_percent >= 95.),
        )
        .unwrap_or((false, false));

    let cluster_feature_sets = if quiet {
        None
    } else {
        let mut feature_sets = feature_set_stats
            .into_iter()
            .map(
                |(
                    feature_set,
                    FeatureSetStatsEntry {
                        stake_percent,
                        rpc_nodes_percent: rpc_percent,
                        software_versions,
                    },
                )| {
                    CliFeatureSet {
                        software_versions: software_versions.into_iter().map(CliVersion).collect(),
                        feature_set,
                        stake_percent,
                        rpc_percent,
                    }
                },
            )
            .collect::<Vec<_>>();
        feature_sets.sort_by(|l, r| {
            match l.software_versions[0]
                .cmp(&r.software_versions[0])
                .reverse()
            {
                Ordering::Equal => {
                    match l
                        .stake_percent
                        .partial_cmp(&r.stake_percent)
                        .unwrap()
                        .reverse()
                    {
                        Ordering::Equal => {
                            l.rpc_percent.partial_cmp(&r.rpc_percent).unwrap().reverse()
                        }
                        o => o,
                    }
                }
                o => o,
            }
        });
        Some(CliClusterFeatureSets {
            tool_feature_set: my_feature_set,
            feature_sets,
            stake_allowed,
            rpc_allowed,
        })
    };

    Ok((stake_allowed && rpc_allowed, cluster_feature_sets))
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
    display_all: bool,
) -> ProcessResult {
    let filter = if !display_all {
        let now = rpc_client.get_slot()?;
        now.checked_sub(DEFAULT_MAX_ACTIVE_DISPLAY_AGE_SLOTS)
    } else {
        None
    };
    let mut inactive = false;
    let mut features = rpc_client
        .get_multiple_accounts(feature_ids)?
        .into_iter()
        .zip(feature_ids)
        .map(|(account, feature_id)| {
            let feature_name = FEATURE_NAMES.get(feature_id).unwrap();
            account
                .and_then(status_from_account)
                .map(|feature_status| CliFeature {
                    id: feature_id.to_string(),
                    description: feature_name.to_string(),
                    status: feature_status,
                })
                .unwrap_or_else(|| {
                    inactive = true;
                    CliFeature {
                        id: feature_id.to_string(),
                        description: feature_name.to_string(),
                        status: CliFeatureStatus::Inactive,
                    }
                })
        })
        .filter(|feature| match (filter, &feature.status) {
            (Some(min_activation), CliFeatureStatus::Active(activation)) => {
                activation > &min_activation
            }
            _ => true,
        })
        .collect::<Vec<_>>();

    features.sort_unstable();

    let (feature_activation_allowed, cluster_feature_sets) =
        feature_activation_allowed(rpc_client, features.len() <= 1)?;
    let feature_set = CliFeatures {
        features,
        feature_activation_allowed,
        cluster_feature_sets,
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

    if !feature_activation_allowed(rpc_client, false)?.0 {
        match force {
        ForceActivation::Almost =>
            return Err("Add force argument once more to override the sanity check to force feature activation ".into()),
        ForceActivation::Yes => println!("FEATURE ACTIVATION FORCED"),
        ForceActivation::No =>
            return Err("Feature activation is not allowed at this time".into()),
        }
    }

    let rent = rpc_client.get_minimum_balance_for_rent_exemption(Feature::size_of())?;

    let blockhash = rpc_client.get_latest_blockhash()?;
    let (message, _) = resolve_spend_tx_and_check_account_balance(
        rpc_client,
        false,
        SpendAmount::Some(rent),
        &blockhash,
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
