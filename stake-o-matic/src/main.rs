#![allow(clippy::integer_arithmetic)]
use {
    clap::{
        crate_description, crate_name, crate_version, value_t, value_t_or_exit, App, Arg,
        ArgMatches,
    },
    log::*,
    reqwest::StatusCode,
    solana_clap_utils::{
        input_parsers::{keypair_of, pubkey_of},
        input_validators::{
            is_amount, is_keypair, is_pubkey_or_keypair, is_url, is_valid_percentage,
        },
    },
    solana_cli_output::display::format_labeled_address,
    solana_client::{
        client_error, rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig,
        rpc_request::MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS, rpc_response::RpcVoteAccountInfo,
    },
    solana_metrics::datapoint_info,
    solana_notifier::Notifier,
    solana_sdk::{
        account_utils::StateMut,
        clock::{Epoch, Slot},
        commitment_config::CommitmentConfig,
        message::Message,
        native_token::*,
        pubkey::Pubkey,
        signature::{Keypair, Signature, Signer},
        transaction::Transaction,
    },
    solana_stake_program::{stake_instruction, stake_state::StakeState},
    std::{
        collections::{HashMap, HashSet},
        error,
        fs::File,
        path::PathBuf,
        process,
        str::FromStr,
        thread::sleep,
        time::Duration,
    },
    thiserror::Error,
};

mod confirmed_block_cache;
mod validator_list;
mod validators_app;

use confirmed_block_cache::ConfirmedBlockCache;

enum InfrastructureConcentrationAffectKind {
    Destake(String),
    Warn(String),
}

#[derive(Debug)]
enum InfrastructureConcentrationAffects {
    WarnAll,
    DestakeListed(HashSet<Pubkey>),
    DestakeAll,
}

impl InfrastructureConcentrationAffects {
    fn destake_memo(validator_id: &Pubkey, concentration: f64, config: &Config) -> String {
        format!(
            "🏟️ `{}` infrastructure concentration {:.1}% is too high. Max concentration is {:.0}%. Removed ◎{}",
            validator_id,
            concentration,
            config.max_infrastructure_concentration,
            lamports_to_sol(config.baseline_stake_amount),
        )
    }
    fn warning_memo(validator_id: &Pubkey, concentration: f64, config: &Config) -> String {
        format!(
            "🗺  `{}` infrastructure concentration {:.1}% is too high. Max concentration is {:.0}%. No stake removed. Consider finding a new data center",
            validator_id,
            concentration,
            config.max_infrastructure_concentration,
        )
    }
    pub fn memo(
        &self,
        validator_id: &Pubkey,
        concentration: f64,
        config: &Config,
    ) -> InfrastructureConcentrationAffectKind {
        match self {
            Self::DestakeAll => InfrastructureConcentrationAffectKind::Destake(Self::destake_memo(
                validator_id,
                concentration,
                config,
            )),
            Self::WarnAll => InfrastructureConcentrationAffectKind::Warn(Self::warning_memo(
                validator_id,
                concentration,
                config,
            )),
            Self::DestakeListed(ref list) => {
                if list.contains(validator_id) {
                    InfrastructureConcentrationAffectKind::Destake(Self::destake_memo(
                        validator_id,
                        concentration,
                        config,
                    ))
                } else {
                    InfrastructureConcentrationAffectKind::Warn(Self::warning_memo(
                        validator_id,
                        concentration,
                        config,
                    ))
                }
            }
        }
    }
}

#[derive(Debug, Error)]
#[error("cannot convert to InfrastructureConcentrationAffects: {0}")]
struct InfrastructureConcentrationAffectsFromStrError(String);

impl FromStr for InfrastructureConcentrationAffects {
    type Err = InfrastructureConcentrationAffectsFromStrError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        match lower.as_str() {
            "warn" => Ok(Self::WarnAll),
            "destake" => Ok(Self::DestakeAll),
            _ => {
                let file = File::open(s)
                    .map_err(|_| InfrastructureConcentrationAffectsFromStrError(s.to_string()))?;
                let mut list: Vec<String> = serde_yaml::from_reader(file)
                    .map_err(|_| InfrastructureConcentrationAffectsFromStrError(s.to_string()))?;
                let list = list
                    .drain(..)
                    .filter_map(|ref s| Pubkey::from_str(s).ok())
                    .collect::<HashSet<_>>();
                Ok(Self::DestakeListed(list))
            }
        }
    }
}

pub fn is_release_version(string: String) -> Result<(), String> {
    if string.starts_with('v') && semver::Version::parse(string.split_at(1).1).is_ok() {
        return Ok(());
    }
    semver::Version::parse(&string)
        .map(|_| ())
        .map_err(|err| format!("{:?}", err))
}

pub fn release_version_of(matches: &ArgMatches<'_>, name: &str) -> Option<semver::Version> {
    matches
        .value_of(name)
        .map(ToString::to_string)
        .map(|string| {
            if string.starts_with('v') {
                semver::Version::parse(string.split_at(1).1)
            } else {
                semver::Version::parse(&string)
            }
            .expect("semver::Version")
        })
}

#[derive(Debug)]
struct Config {
    json_rpc_url: String,
    cluster: String,
    source_stake_address: Pubkey,
    authorized_staker: Keypair,

    /// Only validators with an identity pubkey in this validator_list will be staked
    validator_list: HashSet<Pubkey>,

    dry_run: bool,

    /// Amount of lamports to stake any validator in the validator_list that is not delinquent
    baseline_stake_amount: u64,

    /// Amount of additional lamports to stake quality block producers in the validator_list
    bonus_stake_amount: u64,

    /// Quality validators produce within this percentage of the cluster average skip rate over
    /// the previous epoch
    quality_block_producer_percentage: usize,

    /// A delinquent validator gets this number of slots of grace (from the current slot) before it
    /// will be fully destaked.  The grace period is intended to account for unexpected bugs that
    /// cause a validator to go down
    delinquent_grace_slot_distance: u64,

    /// Don't ever unstake more than this percentage of the cluster at one time for poor block
    /// production
    max_poor_block_producer_percentage: usize,

    /// Vote accounts with a larger commission than this amount will not be staked.
    max_commission: u8,

    address_labels: HashMap<String, String>,

    /// If Some(), destake validators with a version less than this version subject to the
    /// `max_old_release_version_percentage` limit
    min_release_version: Option<semver::Version>,

    /// Don't ever unstake more than this percentage of the cluster at one time for running an
    /// older software version
    max_old_release_version_percentage: usize,

    /// Base path of confirmed block cache
    confirmed_block_cache_path: PathBuf,

    /// Vote accounts sharing infrastructure with larger than this amount will not be staked
    max_infrastructure_concentration: f64,

    /// How validators with infrastruction concentration above `max_infrastructure_concentration`
    /// will be affected. Accepted values are:
    /// 1) "warn"       - Stake unaffected. A warning message is notified
    /// 2) "destake"    - Removes all validator stake
    /// 3) PATH_TO_YAML - Reads a list of validator identity pubkeys from the specified YAML file
    ///                   destaking those in the list and warning any others
    infrastructure_concentration_affects: InfrastructureConcentrationAffects,

    bad_cluster_average_skip_rate: usize,
}

fn default_confirmed_block_cache_path() -> PathBuf {
    let home_dir = std::env::var("HOME").unwrap();
    PathBuf::from(home_dir).join(".cache/solana/som/confirmed-block-cache/")
}

fn get_config() -> Config {
    let default_confirmed_block_cache_path = default_confirmed_block_cache_path()
        .to_str()
        .unwrap()
        .to_string();
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *solana_cli_config::CONFIG_FILE {
                arg.default_value(&config_file)
            } else {
                arg
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .validator(is_url)
                .help("JSON RPC URL for the cluster")
        )
        .arg(
            Arg::with_name("cluster")
                .long("cluster")
                .value_name("NAME")
                .possible_values(&["mainnet-beta", "testnet"])
                .takes_value(true)
                .help("Name of the cluster to operate on")
        )
        .arg(
            Arg::with_name("validator_list_file")
                .long("validator-list")
                .value_name("FILE")
                .required(true)
                .takes_value(true)
                .conflicts_with("cluster")
                .help("File containing an YAML array of validator pubkeys eligible for staking")
        )
        .arg(
            Arg::with_name("confirm")
                .long("confirm")
                .takes_value(false)
                .help("Confirm that the stake adjustments should actually be made")
        )
        .arg(
            Arg::with_name("source_stake_address")
                .index(1)
                .value_name("ADDRESS")
                .takes_value(true)
                .required(true)
                .validator(is_pubkey_or_keypair)
                .help("The source stake account for splitting individual validator stake accounts from")
        )
        .arg(
            Arg::with_name("authorized_staker")
                .index(2)
                .value_name("KEYPAIR")
                .validator(is_keypair)
                .required(true)
                .takes_value(true)
                .help("Keypair of the authorized staker for the source stake account.")
        )
        .arg(
            Arg::with_name("quality_block_producer_percentage")
                .long("quality-block-producer-percentage")
                .value_name("PERCENTAGE")
                .takes_value(true)
                .default_value("15")
                .validator(is_valid_percentage)
                .help("Quality validators have a skip rate within this percentage of the cluster average in the previous epoch.")
        )
        .arg(
            Arg::with_name("bad_cluster_average_skip_rate")
                .long("bad-cluster-average-skip-rate")
                .value_name("PERCENTAGE")
                .takes_value(true)
                .default_value("50")
                .validator(is_valid_percentage)
                .help("Threshold to notify for a poor average cluster skip rate.")
        )
        .arg(
            Arg::with_name("max_poor_block_producer_percentage")
                .long("max-poor-block-producer-percentage")
                .value_name("PERCENTAGE")
                .takes_value(true)
                .default_value("20")
                .validator(is_valid_percentage)
                .help("Do not add or remove bonus stake from any non-delinquent validators if at least this percentage of all validators are poor block producers")
        )
        .arg(
            Arg::with_name("baseline_stake_amount")
                .long("baseline-stake-amount")
                .value_name("SOL")
                .takes_value(true)
                .default_value("5000")
                .validator(is_amount)
        )
        .arg(
            Arg::with_name("bonus_stake_amount")
                .long("bonus-stake-amount")
                .value_name("SOL")
                .takes_value(true)
                .default_value("50000")
                .validator(is_amount)
        )
        .arg(
            Arg::with_name("max_commission")
                .long("max-commission")
                .value_name("PERCENTAGE")
                .takes_value(true)
                .default_value("100")
                .validator(is_valid_percentage)
                .help("Vote accounts with a larger commission than this amount will not be staked")
        )
        .arg(
            Arg::with_name("min_release_version")
                .long("min-release-version")
                .value_name("SEMVER")
                .takes_value(true)
                .validator(is_release_version)
                .help("Remove the base and bonus stake from validators with \
                       a release version older than this one")
        )
        .arg(
            Arg::with_name("max_old_release_version_percentage")
                .long("max-old-release-version-percentage")
                .value_name("PERCENTAGE")
                .takes_value(true)
                .default_value("10")
                .validator(is_valid_percentage)
                .help("Do not remove stake from validators running older \
                       software versions if more than this percentage of \
                       all validators are running an older software version")
        )
        .arg(
            Arg::with_name("confirmed_block_cache_path")
                .long("confirmed-block-cache-path")
                .takes_value(true)
                .value_name("PATH")
                .default_value(&default_confirmed_block_cache_path)
                .help("Base path of confirmed block cache")
        )
        .arg(
            Arg::with_name("max_infrastructure_concentration")
                .long("max-infrastructure-concentration")
                .takes_value(true)
                .value_name("PERCENTAGE")
                .default_value("100")
                .validator(is_valid_percentage)
                .help("Vote accounts sharing infrastructure with larger than this amount will not be staked")
        )
        .arg(
            Arg::with_name("infrastructure_concentration_affects")
                .long("infrastructure-concentration-affects")
                .takes_value(true)
                .value_name("AFFECTS")
                .default_value("warn")
                .validator(|ref s| {
                    InfrastructureConcentrationAffects::from_str(s)
                        .map(|_| ())
                        .map_err(|e| format!("{}", e))
                })
                .help("How validators with infrastruction concentration above \
                       `max_infrastructure_concentration` will be affected. \
                       Accepted values are: \
                       1) warn         - Stake unaffected. A warning message \
                                         is notified \
                       2) destake      - Removes all validator stake \
                       3) PATH_TO_YAML - Reads a list of validator identity \
                                         pubkeys from the specified YAML file \
                                         destaking those in the list and warning \
                                         any others")
        )
        .get_matches();

    let config = if let Some(config_file) = matches.value_of("config_file") {
        solana_cli_config::Config::load(config_file).unwrap_or_default()
    } else {
        solana_cli_config::Config::default()
    };

    let source_stake_address = pubkey_of(&matches, "source_stake_address").unwrap();
    let authorized_staker = keypair_of(&matches, "authorized_staker").unwrap();
    let dry_run = !matches.is_present("confirm");
    let cluster = value_t!(matches, "cluster", String).unwrap_or_else(|_| "unknown".into());
    let quality_block_producer_percentage =
        value_t_or_exit!(matches, "quality_block_producer_percentage", usize);
    let max_commission = value_t_or_exit!(matches, "max_commission", u8);
    let max_poor_block_producer_percentage =
        value_t_or_exit!(matches, "max_poor_block_producer_percentage", usize);
    let max_old_release_version_percentage =
        value_t_or_exit!(matches, "max_old_release_version_percentage", usize);
    let baseline_stake_amount =
        sol_to_lamports(value_t_or_exit!(matches, "baseline_stake_amount", f64));
    let bonus_stake_amount = sol_to_lamports(value_t_or_exit!(matches, "bonus_stake_amount", f64));
    let min_release_version = release_version_of(&matches, "min_release_version");

    let (json_rpc_url, validator_list) = match cluster.as_str() {
        "mainnet-beta" => (
            value_t!(matches, "json_rpc_url", String)
                .unwrap_or_else(|_| "http://api.mainnet-beta.solana.com".into()),
            validator_list::mainnet_beta_validators(),
        ),
        "testnet" => (
            value_t!(matches, "json_rpc_url", String)
                .unwrap_or_else(|_| "http://testnet.solana.com".into()),
            validator_list::testnet_validators(),
        ),
        "unknown" => {
            let validator_list_file =
                File::open(value_t_or_exit!(matches, "validator_list_file", PathBuf))
                    .unwrap_or_else(|err| {
                        error!("Unable to open validator_list: {}", err);
                        process::exit(1);
                    });

            let validator_list = serde_yaml::from_reader::<_, Vec<String>>(validator_list_file)
                .unwrap_or_else(|err| {
                    error!("Unable to read validator_list: {}", err);
                    process::exit(1);
                })
                .into_iter()
                .map(|p| {
                    Pubkey::from_str(&p).unwrap_or_else(|err| {
                        error!("Invalid validator_list pubkey '{}': {}", p, err);
                        process::exit(1);
                    })
                })
                .collect();
            (
                value_t!(matches, "json_rpc_url", String)
                    .unwrap_or_else(|_| config.json_rpc_url.clone()),
                validator_list,
            )
        }
        _ => unreachable!(),
    };
    let validator_list = validator_list.into_iter().collect::<HashSet<_>>();
    let confirmed_block_cache_path = matches
        .value_of("confirmed_block_cache_path")
        .map(PathBuf::from)
        .unwrap();

    let bad_cluster_average_skip_rate =
        value_t!(matches, "bad_cluster_average_skip_rate", usize).unwrap_or(50);
    let max_infrastructure_concentration =
        value_t!(matches, "max_infrastructure_concentration", f64).unwrap();
    let infrastructure_concentration_affects = value_t!(
        matches,
        "infrastructure_concentration_affects",
        InfrastructureConcentrationAffects
    )
    .unwrap();

    let config = Config {
        json_rpc_url,
        cluster,
        source_stake_address,
        authorized_staker,
        validator_list,
        dry_run,
        baseline_stake_amount,
        bonus_stake_amount,
        delinquent_grace_slot_distance: 21600, // ~24 hours worth of slots at 2.5 slots per second
        quality_block_producer_percentage,
        max_commission,
        max_poor_block_producer_percentage,
        address_labels: config.address_labels,
        min_release_version,
        max_old_release_version_percentage,
        confirmed_block_cache_path,
        max_infrastructure_concentration,
        infrastructure_concentration_affects,
        bad_cluster_average_skip_rate,
    };

    info!("RPC URL: {}", config.json_rpc_url);
    config
}

fn get_stake_account(
    rpc_client: &RpcClient,
    address: &Pubkey,
) -> Result<(u64, StakeState), String> {
    let account = rpc_client.get_account(address).map_err(|e| {
        format!(
            "Failed to fetch stake account {}: {}",
            address,
            e.to_string()
        )
    })?;

    if account.owner != solana_stake_program::id() {
        return Err(format!(
            "not a stake account (owned by {}): {}",
            account.owner, address
        ));
    }

    account
        .state()
        .map_err(|e| {
            format!(
                "Failed to decode stake account at {}: {}",
                address,
                e.to_string()
            )
        })
        .map(|stake_state| (account.lamports, stake_state))
}

pub fn retry_rpc_operation<T, F>(mut retries: usize, op: F) -> client_error::Result<T>
where
    F: Fn() -> client_error::Result<T>,
{
    loop {
        let result = op();

        if let Err(client_error::ClientError {
            kind: client_error::ClientErrorKind::Reqwest(ref reqwest_error),
            ..
        }) = result
        {
            let can_retry = reqwest_error.is_timeout()
                || reqwest_error
                    .status()
                    .map(|s| s == StatusCode::BAD_GATEWAY || s == StatusCode::GATEWAY_TIMEOUT)
                    .unwrap_or(false);
            if can_retry && retries > 0 {
                info!("RPC request timeout, {} retries remaining", retries);
                retries -= 1;
                continue;
            }
        }
        return result;
    }
}

type BoxResult<T> = Result<T, Box<dyn error::Error>>;

///                    quality          poor             cluster_skip_rate
type ClassifyResult = (HashSet<Pubkey>, HashSet<Pubkey>, usize);

fn classify_producers(
    first_slot: Slot,
    first_slot_in_epoch: Slot,
    confirmed_blocks: HashSet<u64>,
    leader_schedule: HashMap<String, Vec<usize>>,
    quality_block_producer_percentage: usize,
) -> BoxResult<ClassifyResult> {
    let mut poor_block_producers = HashSet::new();
    let mut quality_block_producers = HashSet::new();
    let mut blocks_and_slots = HashMap::new();

    let mut total_blocks = 0;
    let mut total_slots = 0;
    for (validator_identity, relative_slots) in leader_schedule {
        let mut validator_blocks = 0;
        let mut validator_slots = 0;
        for relative_slot in relative_slots {
            let slot = first_slot_in_epoch + relative_slot as Slot;
            if slot >= first_slot {
                total_slots += 1;
                validator_slots += 1;
                if confirmed_blocks.contains(&slot) {
                    total_blocks += 1;
                    validator_blocks += 1;
                }
            }
        }
        trace!(
            "Validator {} produced {} blocks in {} slots",
            validator_identity,
            validator_blocks,
            validator_slots
        );
        if validator_slots > 0 {
            let validator_identity = Pubkey::from_str(&validator_identity)?;
            let e = blocks_and_slots.entry(validator_identity).or_insert((0, 0));
            e.0 += validator_blocks;
            e.1 += validator_slots;
        }
    }
    let cluster_average_rate = 100 - total_blocks * 100 / total_slots;
    for (validator_identity, (blocks, slots)) in blocks_and_slots {
        let skip_rate: usize = 100 - (blocks * 100 / slots);
        if skip_rate.saturating_sub(quality_block_producer_percentage) >= cluster_average_rate {
            poor_block_producers.insert(validator_identity);
        } else {
            quality_block_producers.insert(validator_identity);
        }
    }

    info!("quality_block_producers: {}", quality_block_producers.len());
    trace!("quality_block_producers: {:?}", quality_block_producers);
    info!("poor_block_producers: {}", poor_block_producers.len());
    trace!("poor_block_producers: {:?}", poor_block_producers);

    Ok((
        quality_block_producers,
        poor_block_producers,
        cluster_average_rate,
    ))
}

/// Split validators into quality/poor lists based on their block production over the given `epoch`
fn classify_block_producers(
    rpc_client: &RpcClient,
    config: &Config,
    epoch: Epoch,
) -> BoxResult<ClassifyResult> {
    let epoch_schedule = rpc_client.get_epoch_schedule()?;
    let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);
    let last_slot_in_epoch = epoch_schedule.get_last_slot_in_epoch(epoch);

    let first_available_block = rpc_client.get_first_available_block()?;
    let minimum_ledger_slot = rpc_client.minimum_ledger_slot()?;
    debug!(
        "first_available_block: {}, minimum_ledger_slot: {}",
        first_available_block, minimum_ledger_slot
    );

    if first_available_block >= last_slot_in_epoch {
        return Err(format!(
            "First available block is newer than the last epoch: {} > {}",
            first_available_block, last_slot_in_epoch
        )
        .into());
    }

    let first_slot = if first_available_block > first_slot_in_epoch {
        first_available_block
    } else {
        first_slot_in_epoch
    };

    let leader_schedule = rpc_client.get_leader_schedule(Some(first_slot))?.unwrap();

    let cache_path = config.confirmed_block_cache_path.join(&config.cluster);
    let cbc = ConfirmedBlockCache::open(cache_path, &config.json_rpc_url).unwrap();
    let confirmed_blocks = cbc
        .query(first_slot, last_slot_in_epoch)?
        .into_iter()
        .collect::<HashSet<_>>();

    classify_producers(
        first_slot,
        first_slot_in_epoch,
        confirmed_blocks,
        leader_schedule,
        config.quality_block_producer_percentage,
    )
}

fn validate_source_stake_account(
    rpc_client: &RpcClient,
    config: &Config,
) -> Result<u64, Box<dyn error::Error>> {
    // check source stake account
    let (source_stake_balance, source_stake_state) =
        get_stake_account(&rpc_client, &config.source_stake_address)?;

    info!(
        "stake account balance: {} SOL",
        lamports_to_sol(source_stake_balance)
    );
    match &source_stake_state {
        StakeState::Initialized(_) | StakeState::Stake(_, _) => source_stake_state
            .authorized()
            .map_or(Ok(source_stake_balance), |authorized| {
                if authorized.staker != config.authorized_staker.pubkey() {
                    Err(format!(
                        "The authorized staker for the source stake account is not {}",
                        config.authorized_staker.pubkey()
                    )
                    .into())
                } else {
                    Ok(source_stake_balance)
                }
            }),
        _ => Err(format!(
            "Source stake account is not in the initialized state: {:?}",
            source_stake_state
        )
        .into()),
    }
}

struct ConfirmedTransaction {
    success: bool,
    signature: Signature,
    memo: String,
}

/// Simulate a list of transactions and filter out the ones that will fail
fn simulate_transactions(
    rpc_client: &RpcClient,
    candidate_transactions: Vec<(Transaction, String)>,
) -> client_error::Result<Vec<(Transaction, String)>> {
    info!("Simulating {} transactions", candidate_transactions.len(),);
    let mut simulated_transactions = vec![];
    for (mut transaction, memo) in candidate_transactions {
        transaction.message.recent_blockhash =
            retry_rpc_operation(10, || rpc_client.get_recent_blockhash())?.0;

        let sim_result = rpc_client.simulate_transaction_with_config(
            &transaction,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                ..RpcSimulateTransactionConfig::default()
            },
        )?;
        if sim_result.value.err.is_some() {
            trace!(
                "filtering out transaction due to simulation failure: {:?}: {}",
                sim_result,
                memo
            );
        } else {
            simulated_transactions.push((transaction, memo))
        }
    }
    info!(
        "Successfully simulating {} transactions",
        simulated_transactions.len()
    );
    Ok(simulated_transactions)
}

fn transact(
    rpc_client: &RpcClient,
    dry_run: bool,
    transactions: Vec<(Transaction, String)>,
    authorized_staker: &Keypair,
) -> Result<Vec<ConfirmedTransaction>, Box<dyn error::Error>> {
    let authorized_staker_balance = rpc_client.get_balance(&authorized_staker.pubkey())?;
    info!(
        "Authorized staker balance: {} SOL",
        lamports_to_sol(authorized_staker_balance)
    );

    let (blockhash, fee_calculator, last_valid_slot) = rpc_client
        .get_recent_blockhash_with_commitment(rpc_client.commitment())?
        .value;
    info!("{} transactions to send", transactions.len());

    let required_fee = transactions.iter().fold(0, |fee, (transaction, _)| {
        fee + fee_calculator.calculate_fee(&transaction.message)
    });
    info!("Required fee: {} SOL", lamports_to_sol(required_fee));
    if required_fee > authorized_staker_balance {
        return Err("Authorized staker has insufficient funds".into());
    }

    let mut pending_transactions = HashMap::new();
    for (mut transaction, memo) in transactions.into_iter() {
        transaction.sign(&[authorized_staker], blockhash);

        pending_transactions.insert(transaction.signatures[0], memo);
        if !dry_run {
            rpc_client.send_transaction(&transaction)?;
        }
    }

    let mut finalized_transactions = vec![];
    loop {
        if pending_transactions.is_empty() {
            break;
        }

        let slot = rpc_client.get_slot_with_commitment(CommitmentConfig::finalized())?;
        info!(
            "Current slot={}, last_valid_slot={} (slots remaining: {}) ",
            slot,
            last_valid_slot,
            last_valid_slot.saturating_sub(slot)
        );

        if slot > last_valid_slot {
            error!(
                "Blockhash {} expired with {} pending transactions",
                blockhash,
                pending_transactions.len()
            );

            for (signature, memo) in pending_transactions.into_iter() {
                finalized_transactions.push(ConfirmedTransaction {
                    success: false,
                    signature,
                    memo,
                });
            }
            break;
        }

        let pending_signatures = pending_transactions.keys().cloned().collect::<Vec<_>>();
        let mut statuses = vec![];
        for pending_signatures_chunk in
            pending_signatures.chunks(MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS - 1)
        {
            trace!(
                "checking {} pending_signatures",
                pending_signatures_chunk.len()
            );
            statuses.extend(
                rpc_client
                    .get_signature_statuses(&pending_signatures_chunk)?
                    .value
                    .into_iter(),
            )
        }
        assert_eq!(statuses.len(), pending_signatures.len());

        for (signature, status) in pending_signatures.into_iter().zip(statuses.into_iter()) {
            info!("{}: status={:?}", signature, status);
            let completed = if dry_run {
                Some(true)
            } else if let Some(status) = &status {
                if status.confirmations.is_none() || status.err.is_some() {
                    Some(status.err.is_none())
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(success) = completed {
                warn!("{}: completed.  success={}", signature, success);
                let memo = pending_transactions.remove(&signature).unwrap();
                finalized_transactions.push(ConfirmedTransaction {
                    success,
                    signature,
                    memo,
                });
            }
        }
        sleep(Duration::from_secs(5));
    }

    Ok(finalized_transactions)
}

fn process_confirmations(
    mut confirmations: Vec<ConfirmedTransaction>,
    notifier: Option<&Notifier>,
) -> bool {
    let mut ok = true;

    confirmations.sort_by(|a, b| a.memo.cmp(&b.memo));
    for ConfirmedTransaction {
        success,
        signature,
        memo,
    } in confirmations
    {
        if success {
            info!("OK:   {}: {}", signature, memo);
            if let Some(notifier) = notifier {
                notifier.send(&memo)
            }
        } else {
            error!("FAIL: {}: {}", signature, memo);
            ok = false
        }
    }
    ok
}

const DATA_CENTER_ID_UNKNOWN: &str = "0-Unknown";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DataCenterId {
    asn: u64,
    location: String,
}

impl Default for DataCenterId {
    fn default() -> Self {
        Self::from_str(DATA_CENTER_ID_UNKNOWN).unwrap()
    }
}

impl std::str::FromStr for DataCenterId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '-');
        let asn = parts.next();
        let location = parts.next();
        if let (Some(asn), Some(location)) = (asn, location) {
            let asn = asn.parse().map_err(|e| format!("{:?}", e))?;
            let location = location.to_string();
            Ok(Self { asn, location })
        } else {
            Err(format!("cannot construct DataCenterId from input: {}", s))
        }
    }
}

impl std::fmt::Display for DataCenterId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}-{}", self.asn, self.location)
    }
}

#[derive(Clone, Debug, Default)]
struct DatacenterInfo {
    id: DataCenterId,
    stake: u64,
    stake_percent: f64,
    validators: Vec<Pubkey>,
}

impl DatacenterInfo {
    pub fn new(id: DataCenterId) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }
}

impl std::fmt::Display for DatacenterInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{:<30}  {:>20}  {:>5.2}  {}",
            self.id.to_string(),
            self.stake,
            self.stake_percent,
            self.validators.len()
        )
    }
}

fn get_data_center_info() -> Result<Vec<DatacenterInfo>, Box<dyn error::Error>> {
    let token = std::env::var("VALIDATORS_APP_TOKEN")?;
    let client = validators_app::Client::new(token);
    let validators = client.validators(None, None)?;
    let mut data_center_infos = HashMap::new();
    let mut total_stake = 0;
    let mut unknown_data_center_stake: u64 = 0;
    for v in validators.as_ref() {
        let account = v
            .account
            .as_ref()
            .and_then(|pubkey| Pubkey::from_str(pubkey).ok());
        let account = if let Some(account) = account {
            account
        } else {
            warn!("No vote pubkey for: {:?}", v);
            continue;
        };

        let stake = v.active_stake.unwrap_or(0);

        let data_center = v
            .data_center_key
            .as_deref()
            .or_else(|| {
                unknown_data_center_stake = unknown_data_center_stake.saturating_add(stake);
                None
            })
            .unwrap_or(DATA_CENTER_ID_UNKNOWN);
        let data_center_id = DataCenterId::from_str(data_center)
            .map_err(|e| {
                unknown_data_center_stake = unknown_data_center_stake.saturating_add(stake);
                e
            })
            .unwrap_or_default();

        let mut data_center_info = data_center_infos
            .entry(data_center_id.clone())
            .or_insert_with(|| DatacenterInfo::new(data_center_id));
        data_center_info.stake += stake;
        total_stake += stake;
        data_center_info.validators.push(account);
    }

    let unknown_percent = 100f64 * (unknown_data_center_stake as f64) / total_stake as f64;
    if unknown_percent > 3f64 {
        warn!("unknown data center percentage: {:.0}%", unknown_percent);
    }

    let data_center_infos = data_center_infos
        .drain()
        .map(|(_, mut i)| {
            i.stake_percent = 100f64 * i.stake as f64 / total_stake as f64;
            i
        })
        .collect();
    Ok(data_center_infos)
}

#[allow(clippy::cognitive_complexity)] // Yeah I know...
fn main() -> Result<(), Box<dyn error::Error>> {
    solana_logger::setup_with_default("solana=info");
    let config = get_config();

    let notifier = Notifier::default();
    let rpc_client = RpcClient::new(config.json_rpc_url.clone());

    if !config.dry_run && notifier.is_empty() {
        error!("A notifier must be active with --confirm");
        process::exit(1);
    }

    // Sanity check that the RPC endpoint is healthy before performing too much work
    rpc_client.get_health().unwrap_or_else(|err| {
        error!("RPC endpoint is unhealthy: {:?}", err);
        process::exit(1);
    });

    let cluster_nodes_with_old_version: HashSet<String> = match config.min_release_version {
        Some(ref min_release_version) => rpc_client
            .get_cluster_nodes()?
            .into_iter()
            .filter_map(|rpc_contact_info| {
                if let Ok(pubkey) = Pubkey::from_str(&rpc_contact_info.pubkey) {
                    if config.validator_list.contains(&pubkey) {
                        if let Some(ref version) = rpc_contact_info.version {
                            if let Ok(semver) = semver::Version::parse(version) {
                                if semver < *min_release_version {
                                    return Some(rpc_contact_info.pubkey);
                                }
                            }
                        }
                    }
                }
                None
            })
            .collect(),
        None => HashSet::default(),
    };

    if let Some(ref min_release_version) = config.min_release_version {
        info!(
            "Validators running a release older than {}: {:?}",
            min_release_version, cluster_nodes_with_old_version,
        );
    }

    let source_stake_balance = validate_source_stake_account(&rpc_client, &config)?;

    let epoch_info = rpc_client.get_epoch_info()?;
    let last_epoch = epoch_info.epoch - 1;

    info!("Epoch info: {:?}", epoch_info);

    let (quality_block_producers, poor_block_producers, cluster_average_skip_rate) =
        classify_block_producers(&rpc_client, &config, last_epoch)?;

    let too_many_poor_block_producers = poor_block_producers.len()
        > quality_block_producers.len() * config.max_poor_block_producer_percentage / 100;

    let too_many_old_validators = cluster_nodes_with_old_version.len()
        > (poor_block_producers.len() + quality_block_producers.len())
            * config.max_old_release_version_percentage
            / 100;

    // Fetch vote account status for all the validator_listed validators
    let vote_account_status = rpc_client.get_vote_accounts()?;
    let vote_account_info = vote_account_status
        .current
        .into_iter()
        .chain(vote_account_status.delinquent.into_iter())
        .filter_map(|vai| {
            let node_pubkey = Pubkey::from_str(&vai.node_pubkey).ok()?;
            if config.validator_list.contains(&node_pubkey) {
                Some(vai)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let infrastructure_concentration = get_data_center_info()
        .map_err(|e| {
            warn!("infrastructure concentration skipped: {}", e);
            e
        })
        .unwrap_or_default()
        .drain(..)
        .filter_map(|dci| {
            if dci.stake_percent > config.max_infrastructure_concentration {
                Some((dci.validators, dci.stake_percent))
            } else {
                None
            }
        })
        .flat_map(|(v, sp)| v.into_iter().map(move |v| (v, sp)))
        .collect::<HashMap<_, _>>();

    let mut source_stake_lamports_required = 0;
    let mut create_stake_transactions = vec![];
    let mut delegate_stake_transactions = vec![];
    let mut stake_activated_in_current_epoch: HashSet<Pubkey> = HashSet::new();
    let mut infrastructure_concentration_warnings = vec![];

    for RpcVoteAccountInfo {
        commission,
        node_pubkey: node_pubkey_str,
        root_slot,
        vote_pubkey,
        ..
    } in &vote_account_info
    {
        let formatted_node_pubkey =
            format_labeled_address(&node_pubkey_str, &config.address_labels);
        let node_pubkey = Pubkey::from_str(&node_pubkey_str).unwrap();
        let baseline_seed = &vote_pubkey.to_string()[..32];
        let bonus_seed = &format!("A{{{}", vote_pubkey)[..32];
        let vote_pubkey = Pubkey::from_str(&vote_pubkey).unwrap();

        let baseline_stake_address = Pubkey::create_with_seed(
            &config.authorized_staker.pubkey(),
            baseline_seed,
            &solana_stake_program::id(),
        )
        .unwrap();
        let bonus_stake_address = Pubkey::create_with_seed(
            &config.authorized_staker.pubkey(),
            bonus_seed,
            &solana_stake_program::id(),
        )
        .unwrap();

        debug!(
            "\nidentity: {}\n - vote address: {}\n - root slot: {}\n - baseline stake: {}\n - bonus stake: {}",
            node_pubkey, vote_pubkey, root_slot, baseline_stake_address, bonus_stake_address
        );

        // Transactions to create the baseline and bonus stake accounts
        if let Ok((balance, stake_state)) = get_stake_account(&rpc_client, &baseline_stake_address)
        {
            if balance <= config.baseline_stake_amount {
                info!(
                    "Unexpected balance in stake account {}: {}, expected {}",
                    baseline_stake_address, balance, config.baseline_stake_amount
                );
            }
            if let Some(delegation) = stake_state.delegation() {
                if epoch_info.epoch == delegation.activation_epoch {
                    stake_activated_in_current_epoch.insert(baseline_stake_address);
                }
            }
        } else {
            info!(
                "Need to create baseline stake account for validator {}",
                formatted_node_pubkey
            );
            source_stake_lamports_required += config.baseline_stake_amount;
            create_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &stake_instruction::split_with_seed(
                        &config.source_stake_address,
                        &config.authorized_staker.pubkey(),
                        config.baseline_stake_amount,
                        &baseline_stake_address,
                        &config.authorized_staker.pubkey(),
                        baseline_seed,
                    ),
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!(
                    "Creating baseline stake account for validator {} ({})",
                    formatted_node_pubkey, baseline_stake_address
                ),
            ));
        }

        if let Ok((balance, stake_state)) = get_stake_account(&rpc_client, &bonus_stake_address) {
            if balance <= config.bonus_stake_amount {
                info!(
                    "Unexpected balance in stake account {}: {}, expected {}",
                    bonus_stake_address, balance, config.bonus_stake_amount
                );
            }
            if let Some(delegation) = stake_state.delegation() {
                if epoch_info.epoch == delegation.activation_epoch {
                    stake_activated_in_current_epoch.insert(bonus_stake_address);
                }
            }
        } else {
            info!(
                "Need to create bonus stake account for validator {}",
                formatted_node_pubkey
            );
            source_stake_lamports_required += config.bonus_stake_amount;
            create_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &stake_instruction::split_with_seed(
                        &config.source_stake_address,
                        &config.authorized_staker.pubkey(),
                        config.bonus_stake_amount,
                        &bonus_stake_address,
                        &config.authorized_staker.pubkey(),
                        bonus_seed,
                    ),
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!(
                    "Creating bonus stake account for validator {} ({})",
                    formatted_node_pubkey, bonus_stake_address
                ),
            ));
        }

        let infrastructure_concentration_destake_memo = infrastructure_concentration
            .get(&node_pubkey)
            .map(|concentration| {
                config.infrastructure_concentration_affects.memo(
                    &node_pubkey,
                    *concentration,
                    &config,
                )
            })
            .and_then(|affect| match affect {
                InfrastructureConcentrationAffectKind::Destake(memo) => Some(memo),
                InfrastructureConcentrationAffectKind::Warn(memo) => {
                    infrastructure_concentration_warnings.push(memo);
                    None
                }
            });

        if let Some(memo_base) = infrastructure_concentration_destake_memo {
            // Deactivate baseline stake
            delegate_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &[stake_instruction::deactivate_stake(
                        &baseline_stake_address,
                        &config.authorized_staker.pubkey(),
                    )],
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!("{} {}", memo_base, "base stake"),
            ));

            // Deactivate bonus stake
            delegate_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &[stake_instruction::deactivate_stake(
                        &bonus_stake_address,
                        &config.authorized_staker.pubkey(),
                    )],
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!("{} {}", memo_base, "bonus stake"),
            ));
        } else if *commission > config.max_commission {
            // Deactivate baseline stake
            delegate_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &[stake_instruction::deactivate_stake(
                        &baseline_stake_address,
                        &config.authorized_staker.pubkey(),
                    )],
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!(
                    "⛔ `{}` commission of {}% is too high. Max commission is {}%. Removed ◎{} baseline stake",
                    formatted_node_pubkey,
                    commission,
                    config.max_commission,
                    lamports_to_sol(config.baseline_stake_amount),
                ),
            ));

            // Deactivate bonus stake
            delegate_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &[stake_instruction::deactivate_stake(
                        &bonus_stake_address,
                        &config.authorized_staker.pubkey(),
                    )],
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!(
                    "⛔ `{}` commission of {}% is too high. Max commission is {}%. Removed ◎{} bonus stake",
                    formatted_node_pubkey,
                    commission,
                    config.max_commission,
                    lamports_to_sol(config.bonus_stake_amount),
                ),
            ));
        } else if !too_many_old_validators
            && cluster_nodes_with_old_version.contains(node_pubkey_str)
        {
            // Deactivate baseline stake
            delegate_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &[stake_instruction::deactivate_stake(
                        &baseline_stake_address,
                        &config.authorized_staker.pubkey(),
                    )],
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!(
                    "🧮 `{}` is running an old software release. Removed ◎{} baseline stake",
                    formatted_node_pubkey,
                    lamports_to_sol(config.baseline_stake_amount),
                ),
            ));

            // Deactivate bonus stake
            delegate_stake_transactions.push((
                Transaction::new_unsigned(Message::new(
                    &[stake_instruction::deactivate_stake(
                        &bonus_stake_address,
                        &config.authorized_staker.pubkey(),
                    )],
                    Some(&config.authorized_staker.pubkey()),
                )),
                format!(
                    "🧮 `{}` is running an old software release. Removed ◎{} bonus stake",
                    formatted_node_pubkey,
                    lamports_to_sol(config.bonus_stake_amount),
                ),
            ));

        // Validator is not considered delinquent if its root slot is less than 256 slots behind the current
        // slot.  This is very generous.
        } else if *root_slot > epoch_info.absolute_slot - 256 {
            datapoint_info!(
                "validator-status",
                ("cluster", config.cluster, String),
                ("id", node_pubkey.to_string(), String),
                ("slot", epoch_info.absolute_slot, i64),
                ("root-slot", *root_slot, i64),
                ("ok", true, bool)
            );

            // Delegate baseline stake
            if !stake_activated_in_current_epoch.contains(&baseline_stake_address) {
                delegate_stake_transactions.push((
                    Transaction::new_unsigned(Message::new(
                        &[stake_instruction::delegate_stake(
                            &baseline_stake_address,
                            &config.authorized_staker.pubkey(),
                            &vote_pubkey,
                        )],
                        Some(&config.authorized_staker.pubkey()),
                    )),
                    format!(
                        "🥩 `{}` is current. Added ◎{} baseline stake",
                        formatted_node_pubkey,
                        lamports_to_sol(config.baseline_stake_amount),
                    ),
                ));
            }

            if !too_many_poor_block_producers {
                if quality_block_producers.contains(&node_pubkey) {
                    // Delegate bonus stake
                    if !stake_activated_in_current_epoch.contains(&bonus_stake_address) {
                        delegate_stake_transactions.push((
                        Transaction::new_unsigned(
                        Message::new(
                            &[stake_instruction::delegate_stake(
                                &bonus_stake_address,
                                &config.authorized_staker.pubkey(),
                                &vote_pubkey,
                            )],
                            Some(&config.authorized_staker.pubkey()),
                        )),
                        format!(
                            "🏅 `{}` was a quality block producer during epoch {}. Added ◎{} bonus stake",
                            formatted_node_pubkey,
                            last_epoch,
                            lamports_to_sol(config.bonus_stake_amount),
                        ),
                    ));
                    }
                } else if poor_block_producers.contains(&node_pubkey) {
                    // Deactivate bonus stake
                    delegate_stake_transactions.push((
                    Transaction::new_unsigned(
                    Message::new(
                        &[stake_instruction::deactivate_stake(
                            &bonus_stake_address,
                            &config.authorized_staker.pubkey(),
                        )],
                        Some(&config.authorized_staker.pubkey()),
                    )),
                    format!(
                        "💔 `{}` was a poor block producer during epoch {}. Removed ◎{} bonus stake",
                        formatted_node_pubkey,
                        last_epoch,
                        lamports_to_sol(config.bonus_stake_amount),
                    ),
                ));
                }
            }
        } else {
            // Destake the validator if it has been delinquent for longer than the grace period
            if *root_slot
                < epoch_info
                    .absolute_slot
                    .saturating_sub(config.delinquent_grace_slot_distance)
            {
                // Deactivate baseline stake
                delegate_stake_transactions.push((
                    Transaction::new_unsigned(Message::new(
                        &[stake_instruction::deactivate_stake(
                            &baseline_stake_address,
                            &config.authorized_staker.pubkey(),
                        )],
                        Some(&config.authorized_staker.pubkey()),
                    )),
                    format!(
                        "🏖️ `{}` is delinquent. Removed ◎{} baseline stake",
                        formatted_node_pubkey,
                        lamports_to_sol(config.baseline_stake_amount),
                    ),
                ));

                // Deactivate bonus stake
                delegate_stake_transactions.push((
                    Transaction::new_unsigned(Message::new(
                        &[stake_instruction::deactivate_stake(
                            &bonus_stake_address,
                            &config.authorized_staker.pubkey(),
                        )],
                        Some(&config.authorized_staker.pubkey()),
                    )),
                    format!(
                        "🏖️ `{}` is delinquent. Removed ◎{} bonus stake",
                        formatted_node_pubkey,
                        lamports_to_sol(config.bonus_stake_amount),
                    ),
                ));

                datapoint_info!(
                    "validator-status",
                    ("cluster", config.cluster, String),
                    ("id", node_pubkey.to_string(), String),
                    ("slot", epoch_info.absolute_slot, i64),
                    ("root-slot", *root_slot, i64),
                    ("ok", false, bool)
                );
            } else {
                // The validator is still considered current for the purposes of metrics reporting,
                datapoint_info!(
                    "validator-status",
                    ("cluster", config.cluster, String),
                    ("id", node_pubkey.to_string(), String),
                    ("slot", epoch_info.absolute_slot, i64),
                    ("root-slot", *root_slot, i64),
                    ("ok", true, bool)
                );
            }
        }
    }

    if create_stake_transactions.is_empty() {
        info!("All stake accounts exist");
    } else {
        info!(
            "{} SOL is required to create {} stake accounts",
            lamports_to_sol(source_stake_lamports_required),
            create_stake_transactions.len()
        );
        if source_stake_balance < source_stake_lamports_required {
            error!(
                "Source stake account has insufficient balance: {} SOL, but {} SOL is required",
                lamports_to_sol(source_stake_balance),
                lamports_to_sol(source_stake_lamports_required)
            );
            process::exit(1);
        }

        let create_stake_transactions =
            simulate_transactions(&rpc_client, create_stake_transactions)?;
        let confirmations = transact(
            &rpc_client,
            config.dry_run,
            create_stake_transactions,
            &config.authorized_staker,
        )?;

        if !process_confirmations(confirmations, None) {
            error!("Failed to create one or more stake accounts.  Unable to continue");
            process::exit(1);
        }
    }

    let delegate_stake_transactions =
        simulate_transactions(&rpc_client, delegate_stake_transactions)?;
    let confirmations = transact(
        &rpc_client,
        config.dry_run,
        delegate_stake_transactions,
        &config.authorized_staker,
    )?;

    if cluster_average_skip_rate > config.bad_cluster_average_skip_rate {
        let message = format!(
            "Cluster average skip rate: {} is above threshold: {}",
            cluster_average_skip_rate, config.bad_cluster_average_skip_rate
        );
        warn!("{}", message);
        if !config.dry_run {
            notifier.send(&message);
        }
    }

    if too_many_poor_block_producers {
        let message = format!(
            "Note: Something is wrong, more than {}% of validators classified \
                       as poor block producers in epoch {}.  Bonus stake frozen",
            config.max_poor_block_producer_percentage, last_epoch,
        );
        warn!("{}", message);
        if !config.dry_run {
            notifier.send(&message);
        }
    }

    if too_many_old_validators {
        let message = format!(
            "Note: Something is wrong, more than {}% of validators classified \
                     as running an older release",
            config.max_old_release_version_percentage
        );
        warn!("{}", message);
        if !config.dry_run {
            notifier.send(&message);
        }
    }

    let confirmations_succeeded = process_confirmations(
        confirmations,
        if config.dry_run {
            None
        } else {
            Some(&notifier)
        },
    );

    for memo in &infrastructure_concentration_warnings {
        if config.dry_run && !notifier.is_empty() {
            notifier.send(memo)
        }
    }

    if !confirmations_succeeded {
        process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_quality_producer() {
        solana_logger::setup();
        let percentage = 10;

        let confirmed_blocks: HashSet<Slot> = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 14, 21, 22, 43, 44, 45, 46, 47, 48,
        ]
        .iter()
        .cloned()
        .collect();
        let mut leader_schedule = HashMap::new();
        let l1 = Pubkey::new_unique();
        let l2 = Pubkey::new_unique();
        let l3 = Pubkey::new_unique();
        let l4 = Pubkey::new_unique();
        let l5 = Pubkey::new_unique();
        leader_schedule.insert(l1.to_string(), (0..10).collect());
        leader_schedule.insert(l2.to_string(), (10..20).collect());
        leader_schedule.insert(l3.to_string(), (20..30).collect());
        leader_schedule.insert(l4.to_string(), (30..40).collect());
        leader_schedule.insert(l5.to_string(), (40..50).collect());
        let (quality, poor, _cluster_average) =
            classify_producers(0, 0, confirmed_blocks, leader_schedule, percentage).unwrap();
        assert!(quality.contains(&l1));
        assert!(quality.contains(&l5));
        assert!(quality.contains(&l2));
        assert!(poor.contains(&l3));
        assert!(poor.contains(&l4));
    }
}
