use {
    crate::{
        cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
        spend_utils::{resolve_spend_tx_and_check_account_balance, SpendAmount},
    },
    clap::{value_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand},
    console::style,
    serde::{Deserialize, Serialize},
    solana_clap_utils::{
        fee_payer::*, hidden_unless_forced, input_parsers::*, input_validators::*, keypair::*,
    },
    solana_cli_output::{cli_version::CliVersion, QuietDisplay, VerboseDisplay},
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{client_error::Error as ClientError, request::MAX_MULTIPLE_ACCOUNTS},
    solana_sdk::{
        account::Account,
        clock::Slot,
        epoch_schedule::EpochSchedule,
        feature::{self, Feature},
        feature_set::FEATURE_NAMES,
        genesis_config::ClusterType,
        message::Message,
        pubkey::Pubkey,
        stake_history::Epoch,
        transaction::Transaction,
    },
    std::{cmp::Ordering, collections::HashMap, fmt, rc::Rc, str::FromStr},
};

const DEFAULT_MAX_ACTIVE_DISPLAY_AGE_SLOTS: Slot = 15_000_000; // ~90days

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ForceActivation {
    No,
    Almost,
    Yes,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FeatureCliCommand {
    Status {
        features: Vec<Pubkey>,
        display_all: bool,
    },
    Activate {
        feature: Pubkey,
        cluster: ClusterType,
        force: ForceActivation,
        fee_payer: SignerIndex,
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
        (&self.status, &self.id).cmp(&(&other.status, &other.id))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliFeatures {
    pub features: Vec<CliFeature>,
    #[serde(skip)]
    pub epoch_schedule: EpochSchedule,
    #[serde(skip)]
    pub current_slot: Slot,
    pub feature_activation_allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_feature_sets: Option<CliClusterFeatureSets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_software_versions: Option<CliClusterSoftwareVersions>,
    #[serde(skip)]
    pub inactive: bool,
}

impl fmt::Display for CliFeatures {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.features.is_empty() {
            writeln!(
                f,
                "{}",
                style(format!(
                    "{:<44} | {:<23} | {} | {}",
                    "Feature", "Status", "Activation Slot", "Description"
                ))
                .bold()
            )?;
        }
        for feature in &self.features {
            writeln!(
                f,
                "{:<44} | {:<23} | {:<15} | {}",
                feature.id,
                match feature.status {
                    CliFeatureStatus::Inactive => style("inactive".to_string()).red(),
                    CliFeatureStatus::Pending => {
                        let current_epoch = self.epoch_schedule.get_epoch(self.current_slot);
                        style(format!("pending until epoch {}", current_epoch + 1)).yellow()
                    }
                    CliFeatureStatus::Active(activation_slot) => {
                        let activation_epoch = self.epoch_schedule.get_epoch(activation_slot);
                        style(format!("active since epoch {activation_epoch}")).green()
                    }
                },
                match feature.status {
                    CliFeatureStatus::Active(activation_slot) => activation_slot.to_string(),
                    _ => "NA".to_string(),
                },
                feature.description,
            )?;
        }

        if let Some(software_versions) = &self.cluster_software_versions {
            write!(f, "{software_versions}")?;
        }

        if let Some(feature_sets) = &self.cluster_feature_sets {
            write!(f, "{feature_sets}")?;
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
    pub feature_sets: Vec<CliFeatureSetStats>,
    #[serde(skip)]
    pub stake_allowed: bool,
    #[serde(skip)]
    pub rpc_allowed: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliClusterSoftwareVersions {
    tool_software_version: CliVersion,
    software_versions: Vec<CliSoftwareVersionStats>,
}

impl fmt::Display for CliClusterSoftwareVersions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let software_version_title = "Software Version";
        let stake_percent_title = "Stake";
        let rpc_percent_title = "RPC";
        let mut max_software_version_len = software_version_title.len();
        let mut max_stake_percent_len = stake_percent_title.len();
        let mut max_rpc_percent_len = rpc_percent_title.len();

        let software_versions: Vec<_> = self
            .software_versions
            .iter()
            .map(|software_version_stats| {
                let stake_percent = format!("{:.2}%", software_version_stats.stake_percent);
                let rpc_percent = format!("{:.2}%", software_version_stats.rpc_percent);
                let software_version = software_version_stats.software_version.to_string();

                max_software_version_len = max_software_version_len.max(software_version.len());
                max_stake_percent_len = max_stake_percent_len.max(stake_percent.len());
                max_rpc_percent_len = max_rpc_percent_len.max(rpc_percent.len());

                (software_version, stake_percent, rpc_percent)
            })
            .collect();

        writeln!(
            f,
            "\n\n{}",
            style(format!(
                "Tool Software Version: {}",
                self.tool_software_version
            ))
            .bold()
        )?;
        writeln!(
            f,
            "{}",
            style(format!(
                "{software_version_title:<max_software_version_len$}  {stake_percent_title:>max_stake_percent_len$}  {rpc_percent_title:>max_rpc_percent_len$}",
            ))
            .bold(),
        )?;
        for (software_version, stake_percent, rpc_percent) in software_versions {
            let me = self.tool_software_version.to_string() == software_version;
            writeln!(
                f,
                "{1:<0$}  {3:>2$}  {5:>4$}  {6}",
                max_software_version_len,
                software_version,
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
                "{software_versions_title:<max_software_versions_len$}  {feature_set_title:<max_feature_set_len$}  {stake_percent_title:>max_stake_percent_len$}  {rpc_percent_title:>max_rpc_percent_len$}",
            ))
            .bold(),
        )?;
        for (software_versions, feature_set, stake_percent, rpc_percent, me) in feature_sets {
            writeln!(
                f,
                "{1:<0$}  {3:>2$}  {5:>4$}  {7:>6$}  {8}",
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
pub struct CliFeatureSetStats {
    software_versions: Vec<CliVersion>,
    feature_set: u32,
    stake_percent: f64,
    rpc_percent: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSoftwareVersionStats {
    software_version: CliVersion,
    stake_percent: f64,
    rpc_percent: f32,
}

/// Check an RPC's reported genesis hash against the ClusterType's known genesis hash
fn check_rpc_genesis_hash(
    cluster_type: &ClusterType,
    rpc_client: &RpcClient,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(genesis_hash) = cluster_type.get_genesis_hash() {
        let rpc_genesis_hash = rpc_client.get_genesis_hash()?;
        if rpc_genesis_hash != genesis_hash {
            return Err(format!(
                "The genesis hash for the specified cluster {cluster_type:?} does not match the \
                genesis hash reported by the specified RPC. Cluster genesis hash: {genesis_hash}, \
                RPC reported genesis hash: {rpc_genesis_hash}"
            )
            .into());
        }
    }
    Ok(())
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
                            Arg::with_name("cluster")
                                .value_name("CLUSTER")
                                .possible_values(&ClusterType::STRINGS)
                                .required(true)
                                .help("The cluster to activate the feature on"),
                        )
                        .arg(
                            Arg::with_name("force")
                                .long("yolo")
                                .hidden(hidden_unless_forced())
                                .multiple(true)
                                .help("Override activation sanity checks. Don't use this flag"),
                        )
                        .arg(fee_payer_arg()),
                ),
        )
    }
}

fn known_feature(feature: &Pubkey) -> Result<(), CliError> {
    if FEATURE_NAMES.contains_key(feature) {
        Ok(())
    } else {
        Err(CliError::BadParameter(format!(
            "Unknown feature: {feature}"
        )))
    }
}

pub fn parse_feature_subcommand(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let response = match matches.subcommand() {
        ("activate", Some(matches)) => {
            let cluster = value_t_or_exit!(matches, "cluster", ClusterType);
            let (feature_signer, feature) = signer_of(matches, "feature", wallet_manager)?;
            let (fee_payer, fee_payer_pubkey) =
                signer_of(matches, FEE_PAYER_ARG.name, wallet_manager)?;

            let force = match matches.occurrences_of("force") {
                2 => ForceActivation::Yes,
                1 => ForceActivation::Almost,
                _ => ForceActivation::No,
            };

            let signer_info = default_signer.generate_unique_signers(
                vec![fee_payer, feature_signer],
                matches,
                wallet_manager,
            )?;

            let feature = feature.unwrap();

            known_feature(&feature)?;

            CliCommandInfo {
                command: CliCommand::Feature(FeatureCliCommand::Activate {
                    feature,
                    cluster,
                    force,
                    fee_payer: signer_info.index_of(fee_payer_pubkey).unwrap(),
                }),
                signers: signer_info.signers,
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
            let display_all =
                matches.is_present("display_all") || features.len() < FEATURE_NAMES.len();
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
        FeatureCliCommand::Activate {
            feature,
            cluster,
            force,
            fee_payer,
        } => process_activate(rpc_client, config, *feature, *cluster, *force, *fee_payer),
    }
}

#[derive(Debug, Default)]
struct FeatureSetStatsEntry {
    stake_percent: f64,
    rpc_nodes_percent: f32,
    software_versions: Vec<CliVersion>,
}

#[derive(Debug, Default, Clone, Copy)]
struct ClusterInfoStatsEntry {
    stake_percent: f64,
    rpc_percent: f32,
}

struct ClusterInfoStats {
    stats_map: HashMap<(u32, CliVersion), ClusterInfoStatsEntry>,
}

impl ClusterInfoStats {
    fn aggregate_by_feature_set(&self) -> HashMap<u32, FeatureSetStatsEntry> {
        let mut feature_set_map = HashMap::<u32, FeatureSetStatsEntry>::new();
        for ((feature_set, software_version), stats_entry) in &self.stats_map {
            let map_entry = feature_set_map.entry(*feature_set).or_default();
            map_entry.rpc_nodes_percent += stats_entry.rpc_percent;
            map_entry.stake_percent += stats_entry.stake_percent;
            map_entry.software_versions.push(software_version.clone());
        }
        for stats_entry in feature_set_map.values_mut() {
            stats_entry
                .software_versions
                .sort_by(|l, r| l.cmp(r).reverse());
        }
        feature_set_map
    }

    fn aggregate_by_software_version(&self) -> HashMap<CliVersion, ClusterInfoStatsEntry> {
        let mut software_version_map = HashMap::<CliVersion, ClusterInfoStatsEntry>::new();
        for ((_feature_set, software_version), stats_entry) in &self.stats_map {
            let map_entry = software_version_map
                .entry(software_version.clone())
                .or_default();
            map_entry.rpc_percent += stats_entry.rpc_percent;
            map_entry.stake_percent += stats_entry.stake_percent;
        }
        software_version_map
    }
}

fn cluster_info_stats(rpc_client: &RpcClient) -> Result<ClusterInfoStats, ClientError> {
    #[derive(Default)]
    struct StatsEntry {
        stake_lamports: u64,
        rpc_nodes_count: u32,
    }

    let cluster_info_list = rpc_client
        .get_cluster_nodes()?
        .into_iter()
        .map(|contact_info| {
            (
                contact_info.pubkey,
                contact_info.feature_set,
                contact_info.rpc.is_some(),
                contact_info
                    .version
                    .and_then(|v| CliVersion::from_str(&v).ok())
                    .unwrap_or_else(CliVersion::unknown_version),
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

    let mut cluster_info_stats: HashMap<(u32, CliVersion), StatsEntry> = HashMap::new();
    let mut total_rpc_nodes = 0;
    for (node_id, feature_set, is_rpc, version) in cluster_info_list {
        let feature_set = feature_set.unwrap_or(0);
        let stats_entry = cluster_info_stats
            .entry((feature_set, version))
            .or_default();

        if let Some(vote_stake) = vote_stakes.get(&node_id) {
            stats_entry.stake_lamports += *vote_stake;
        }

        if is_rpc {
            stats_entry.rpc_nodes_count += 1;
            total_rpc_nodes += 1;
        }
    }

    Ok(ClusterInfoStats {
        stats_map: cluster_info_stats
            .into_iter()
            .filter_map(
                |(
                    cluster_config,
                    StatsEntry {
                        stake_lamports,
                        rpc_nodes_count,
                    },
                )| {
                    let stake_percent = (stake_lamports as f64 / total_active_stake as f64) * 100.;
                    let rpc_percent = (rpc_nodes_count as f32 / total_rpc_nodes as f32) * 100.;
                    if stake_percent >= 0.001 || rpc_percent >= 0.001 {
                        Some((
                            cluster_config,
                            ClusterInfoStatsEntry {
                                stake_percent,
                                rpc_percent,
                            },
                        ))
                    } else {
                        None
                    }
                },
            )
            .collect(),
    })
}

// Feature activation is only allowed when 95% of the active stake is on the current feature set
fn feature_activation_allowed(
    rpc_client: &RpcClient,
    quiet: bool,
) -> Result<
    (
        bool,
        Option<CliClusterFeatureSets>,
        Option<CliClusterSoftwareVersions>,
    ),
    ClientError,
> {
    let cluster_info_stats = cluster_info_stats(rpc_client)?;
    let feature_set_stats = cluster_info_stats.aggregate_by_feature_set();

    let tool_version = solana_version::Version::default();
    let tool_feature_set = tool_version.feature_set;
    let tool_software_version = CliVersion::from(semver::Version::new(
        tool_version.major as u64,
        tool_version.minor as u64,
        tool_version.patch as u64,
    ));
    let (stake_allowed, rpc_allowed) = feature_set_stats
        .get(&tool_feature_set)
        .map(
            |FeatureSetStatsEntry {
                 stake_percent,
                 rpc_nodes_percent,
                 ..
             }| (*stake_percent >= 95., *rpc_nodes_percent >= 95.),
        )
        .unwrap_or_default();

    let cluster_software_versions = if quiet {
        None
    } else {
        let mut software_versions: Vec<_> = cluster_info_stats
            .aggregate_by_software_version()
            .into_iter()
            .map(|(software_version, stats)| CliSoftwareVersionStats {
                software_version,
                stake_percent: stats.stake_percent,
                rpc_percent: stats.rpc_percent,
            })
            .collect();
        software_versions.sort_by(|l, r| l.software_version.cmp(&r.software_version).reverse());
        Some(CliClusterSoftwareVersions {
            software_versions,
            tool_software_version,
        })
    };

    let cluster_feature_sets = if quiet {
        None
    } else {
        let mut feature_sets: Vec<_> = feature_set_stats
            .into_iter()
            .map(|(feature_set, stats_entry)| CliFeatureSetStats {
                feature_set,
                software_versions: stats_entry.software_versions,
                rpc_percent: stats_entry.rpc_nodes_percent,
                stake_percent: stats_entry.stake_percent,
            })
            .collect();

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
            tool_feature_set,
            feature_sets,
            stake_allowed,
            rpc_allowed,
        })
    };

    Ok((
        stake_allowed && rpc_allowed,
        cluster_feature_sets,
        cluster_software_versions,
    ))
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

pub fn get_feature_activation_epoch(
    rpc_client: &RpcClient,
    feature_id: &Pubkey,
) -> Result<Option<Epoch>, ClientError> {
    rpc_client
        .get_feature_activation_slot(feature_id)
        .and_then(|activation_slot: Option<Slot>| {
            rpc_client
                .get_epoch_schedule()
                .map(|epoch_schedule| (activation_slot, epoch_schedule))
        })
        .map(|(activation_slot, epoch_schedule)| {
            activation_slot.map(|slot| epoch_schedule.get_epoch(slot))
        })
}

fn process_status(
    rpc_client: &RpcClient,
    config: &CliConfig,
    feature_ids: &[Pubkey],
    display_all: bool,
) -> ProcessResult {
    let current_slot = rpc_client.get_slot()?;
    let filter = if !display_all {
        current_slot.checked_sub(DEFAULT_MAX_ACTIVE_DISPLAY_AGE_SLOTS)
    } else {
        None
    };
    let mut inactive = false;
    let mut features = vec![];
    for feature_ids in feature_ids.chunks(MAX_MULTIPLE_ACCOUNTS) {
        let mut feature_chunk = rpc_client
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
        features.append(&mut feature_chunk);
    }

    features.sort_unstable();

    let (feature_activation_allowed, cluster_feature_sets, cluster_software_versions) =
        feature_activation_allowed(rpc_client, features.len() <= 1)?;
    let epoch_schedule = rpc_client.get_epoch_schedule()?;
    let feature_set = CliFeatures {
        features,
        current_slot,
        epoch_schedule,
        feature_activation_allowed,
        cluster_feature_sets,
        cluster_software_versions,
        inactive,
    };
    Ok(config.output_format.formatted_string(&feature_set))
}

fn process_activate(
    rpc_client: &RpcClient,
    config: &CliConfig,
    feature_id: Pubkey,
    cluster: ClusterType,
    force: ForceActivation,
    fee_payer: SignerIndex,
) -> ProcessResult {
    check_rpc_genesis_hash(&cluster, rpc_client)?;

    let fee_payer = config.signers[fee_payer];
    let account = rpc_client
        .get_multiple_accounts(&[feature_id])?
        .into_iter()
        .next()
        .unwrap();

    if let Some(account) = account {
        if feature::from_account(&account).is_some() {
            return Err(format!("{feature_id} has already been activated").into());
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
        &fee_payer.pubkey(),
        |lamports| {
            Message::new(
                &feature::activate_with_lamports(&feature_id, &fee_payer.pubkey(), lamports),
                Some(&fee_payer.pubkey()),
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
