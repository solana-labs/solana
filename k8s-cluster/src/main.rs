use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg, ArgMatches},
    log::*,
    solana_k8s_cluster::{
        add_tag_to_name, calculate_stake_allocations,
        docker::{DockerConfig, DockerImageConfig},
        genesis::{
            Genesis, GenesisFlags, DEFAULT_CLIENT_LAMPORTS_PER_SIGNATURE,
            DEFAULT_INTERNAL_NODE_SOL, DEFAULT_INTERNAL_NODE_STAKE_SOL,
        },
        get_solana_root, initialize_globals,
        kubernetes::{
            ClientConfig, Kubernetes, Metrics, NodeAffinityType, PodRequests, ValidatorConfig,
        },
        ledger_helper::LedgerHelper,
        parse_and_format_bench_tps_args,
        release::{BuildConfig, Deploy},
        ValidatorType,
    },
    solana_ledger::blockstore_cleanup_service::{
        DEFAULT_MAX_LEDGER_SHREDS, DEFAULT_MIN_MAX_LEDGER_SHREDS,
    },
    solana_sdk::{pubkey::Pubkey, signature::keypair::read_keypair_file, signer::Signer},
    std::{thread, time::Duration},
};

fn parse_matches() -> ArgMatches<'static> {
    App::new(crate_name!())
        .about(crate_description!())
        .arg(
            Arg::with_name("cluster_namespace")
                .long("namespace")
                .short("n")
                .takes_value(true)
                .default_value("default")
                .help("namespace to deploy test cluster"),
        )
        .arg(
            Arg::with_name("number_of_validators")
                .long("num-validators")
                .takes_value(true)
                .default_value("1")
                .help("Number of validator replicas to deploy")
                .validator(|s| match s.parse::<i32>() {
                    Ok(n) if n > 0 => Ok(()),
                    _ => Err(String::from("number_of_validators should be >= 0")),
                }),
        )
        .arg(
            Arg::with_name("bootstrap_container_name")
                .long("bootstrap-container")
                .takes_value(true)
                .required(true)
                .default_value("bootstrap-container")
                .help("Bootstrap Validator Container name"),
        )
        .arg(
            Arg::with_name("bootstrap_image_name")
                .long("bootstrap-image")
                .takes_value(true)
                .required(true)
                .help("Docker Image of Bootstrap Validator to deploy"),
        )
        .arg(
            Arg::with_name("validator_container_name")
                .long("validator-container")
                .takes_value(true)
                .required(true)
                .default_value("validator-container")
                .help("Validator Container name"),
        )
        .arg(
            Arg::with_name("validator_image_name")
                .long("validator-image")
                .takes_value(true)
                .required(true)
                .help("Docker Image of Validator to deploy"),
        )
        .arg(
            Arg::with_name("release_channel")
                .long("release-channel")
                .takes_value(true)
                .required_if("deploy_method", "tar") // Require if deploy_method is "tar"
                .help("release version. e.g. v1.16.5. Required if '--deploy-method tar'"),
        )
        .arg(
            Arg::with_name("deploy_method")
                .long("deploy-method")
                .takes_value(true)
                .possible_values(&["local", "tar", "skip"])
                .default_value("local")
                .help("Deploy method. tar, local, skip. [default: local]"),
        )
        .arg(
            Arg::with_name("do_build")
                .long("do-build")
                .help("Enable building for building from local repo"),
        )
        .arg(
            Arg::with_name("debug_build")
                .long("debug-build")
                .help("Enable debug build"),
        )
        .arg(
            Arg::with_name("profile_build")
                .long("profile-build")
                .help("Enable Profile Build flags"),
        )
        .arg(
            Arg::with_name("docker_build")
                .long("docker-build")
                .requires("registry_name")
                .help("Build Docker images. If not set, will assume docker image with whatever tag is specified should be used"),
        )
        .arg(
            Arg::with_name("registry_name")
                .long("registry")
                .takes_value(true)
                .required_if("docker_build", "true")
                .help("Registry to push docker image to"),
        )
        .arg(
            Arg::with_name("image_name")
                .long("image-name")
                .takes_value(true)
                .default_value_if("docker_build", None, "k8s-cluster-image")
                .requires("docker_build")
                .help("Docker image name. Will be prepended with validator_type (bootstrap or validator)"),
        )
        .arg(
            Arg::with_name("base_image")
                .long("base-image")
                .takes_value(true)
                .default_value_if("docker_build", None, "ubuntu:20.04")
                .requires("docker_build")
                .help("Docker base image"),
        )
        .arg(
            Arg::with_name("image_tag")
                .long("tag")
                .takes_value(true)
                .default_value_if("docker_build", None, "latest")
                .help("Docker image tag."),
        )
        // Multiple Deployment Config
        .arg(
            Arg::with_name("deployment_tag")
                .long("deployment-tag")
                .takes_value(true)
                .help("Add tag as suffix to this deployment's k8s components. Helps differentiate between two deployments
                in the same cluster. Tag gets added to pod, service, replica-set, etc names. As a result the tag must consist of
                lower case alphanumeric characters or '-', start with an alphabetic character, and end with an alphanumeric character
                (e.g. 'v1-16-5', 'v1', etc, regex used for validation is '[a-z]([-a-z0-9]*[a-z0-9])?')"),
        )
        .arg(
            Arg::with_name("no_bootstrap")
                .long("no-bootstrap")
                .help("Do not deploy a bootstrap validator. Used when deploying multiple clusters"),
        )
        // Genesis config
        .arg(
            Arg::with_name("skip_genesis_build")
                .long("skip-genesis-build")
                .help("skip genesis build. Don't generate a new genesis and associated validator accounts.
                    really just for testing. can rerun a basic test without having to build and push a new docker container"),
        )
        .arg(
            Arg::with_name("hashes_per_tick")
                .long("hashes-per-tick")
                .takes_value(true)
                .default_value("auto")
                .help("NUM_HASHES|sleep|auto - Override the default --hashes-per-tick for the cluster"),
        )
        .arg(
            Arg::with_name("slots_per_epoch")
                .long("slots-per-epoch")
                .takes_value(true)
                .help("override the number of slots in an epoch"),
        )
        .arg(
            Arg::with_name("target_lamports_per_signature")
                .long("target-lamports-per-signature")
                .takes_value(true)
                .help("Genesis config. target lamports per signature"),
        )
        .arg(
            Arg::with_name("faucet_lamports")
                .long("faucet-lamports")
                .takes_value(true)
                .help("Override the default 500000000000000000 lamports minted in genesis"),
        )
        .arg(
            Arg::with_name("bootstrap_validator_sol")
                .long("bootstrap-validator-sol")
                .takes_value(true)
                .help("Genesis config. bootstrap validator sol"),
        )
        .arg(
            Arg::with_name("bootstrap_validator_stake_sol")
                .long("bootstrap-validator-stake-sol")
                .takes_value(true)
                .help("Genesis config. bootstrap validator stake sol"),
        )
        .arg(
            Arg::with_name("internal_node_sol")
                .long("internal-node-sol")
                .takes_value(true)
                .help("Amount to fund internal nodes in genesis config."),
        )
        .arg(
            Arg::with_name("internal_node_stake_sol")
                .long("internal-node-stake-sol")
                .takes_value(true)
                .help("Amount to stake internal nodes (Sol)."),
        )
        .arg(
            Arg::with_name("internal_node_stake_distribution")
                .long("internal-node-stake-distribution")
                .takes_value(true)
                .multiple(true)
                .help("Distribution of node stake. based on 1,000,000 SOL.
                list of percentages. must add to 100%. e.g.
                    --internal-node-stake-distribution 33 33 34
                    --internal-node-stake-distribution 50 50
                    --internal-node-stake-distribution 22 18 21 5 34
                    "),
        )
        .arg(
            Arg::with_name("enable_warmup_epochs")
                .long("enable-warmup-epochs")
                .takes_value(true)
                .possible_values(&["true", "false"])
                .default_value("true")
                .help("Genesis config. enable warmup epoch. defaults to true"),
        )
        .arg(
            Arg::with_name("max_genesis_archive_unpacked_size")
                .long("max-genesis-archive-unpacked-size")
                .takes_value(true)
                .help("Genesis config. max_genesis_archive_unpacked_size"),
        )
        .arg(
            Arg::with_name("cluster_type")
                .long("cluster-type")
                .possible_values(&["development", "devnet", "testnet", "mainnet-beta"])
                .takes_value(true)
                .default_value("development")
                .help(
                    "Selects the features that will be enabled for the cluster"
                ),
        )
        .arg(
            Arg::with_name("tpu_enable_udp")
                .long("tpu-enable-udp")
                .help("Validator config. Enable UDP for tpu transactions."),
        )
        .arg(
            Arg::with_name("tpu_disable_quic")
                .long("tpu-disable-quic")
                .help("Validator config. Disable quic for tpu packet forwarding"),
        )
        .arg(
            Arg::with_name("gpu_mode")
                .long("gpu-mode")
                .takes_value(true)
                .possible_values(&["on", "off", "auto", "cuda"])
                .default_value("auto")
                .help("Not supported yet. Specify GPU mode to launch validators with (default: auto)."),
        )
        .arg(
            Arg::with_name("wait_for_supermajority")
                .long("wait-for-supermajority")
                .takes_value(true)
                .help("Slot number"),
        )
        .arg(
            Arg::with_name("warp_slot")
                .long("warp-slot")
                .takes_value(true)
                .help("Boot from a snapshot that has warped ahead to WARP_SLOT rather than a slot 0 genesis"),
        )
        .arg(
            Arg::with_name("limit_ledger_size")
                .long("limit-ledger-size")
                .takes_value(true)
                .help("Validator Config. The `--limit-ledger-size` parameter allows you to specify how many ledger
                shreds your node retains on disk. If you do not
                include this parameter, the validator will keep the entire ledger until it runs
                out of disk space. The default value attempts to keep the ledger disk usage
                under 500GB. More or less disk usage may be requested by adding an argument to
                `--limit-ledger-size` if desired. Check `solana-validator --help` for the
                default limit value used by `--limit-ledger-size`. More information about
                selecting a custom limit value is at : https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26"),
        )
        .arg(
            Arg::with_name("skip_poh_verify")
                .long("skip-poh-verify")
                .help("Validator config. If set, validators will skip verifying
                the ledger they already have saved to disk at
                boot (results in a much faster boot)"),
        )
        .arg(
            Arg::with_name("no_snapshot_fetch")
                .long("no-snapshot-fetch")
                .help("Validator config. If set, disables booting validators from a snapshot"),
        )
        .arg(
            Arg::with_name("require_tower")
                .long("require-tower")
                .help("Validator config. Refuse to start if saved tower state is not found.
                Off by default since validator won't restart if the pod restarts"),
        )
        .arg(
            Arg::with_name("full_rpc")
                .long("full-rpc")
                .help("Validator config. Support full RPC services on all nodes"),
        )
        // Client Configurations
        .arg(
            Arg::with_name("number_of_clients")
                .long("num-clients")
                .short("c")
                .takes_value(true)
                .default_value("0")
                .help("Number of clients ")
                .validator(|s| match s.parse::<i32>() {
                    Ok(n) if n >= 0 => Ok(()),
                    _ => Err(String::from("number_of_clients should be >= 0")),
                }),
        )
        .arg(
            Arg::with_name("client_delay_start")
                .long("client-delay-start")
                .takes_value(true)
                .default_value("0")
                .help("Number of seconds to wait after validators have finished starting before starting client programs
                (default: 0")
                .validator(|s| match s.parse::<i32>() {
                    Ok(n) if n >= 0 => Ok(()),
                    _ => Err(String::from("client_delay_start should be greater than 0")),
                }),
        )
        .arg(
            Arg::with_name("client_type")
                .long("client-type")
                .takes_value(true)
                .default_value("thin-client")
                .possible_values(&["thin-client", "tpu-client", "rpc-client"])
                .help("Client Config. options: thin-client, tpu-client, rpc-client. default: [thin-client]"),
        )
        .arg(
            Arg::with_name("client_to_run")
                .long("client-to-run")
                .takes_value(true)
                .default_value("bench-tps")
                .possible_values(&["bench-tps", "idle"])
                .help("Client Config. options: idle, bench-tps. default: [bench-tps]"),
        )
        .arg(
            Arg::with_name("bench_tps_args")
                .long("bench-tps-args")
                .value_name("KEY VALUE")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .help("Client Config.
                User can optionally provide extraArgs that are transparently
                supplied to the client program as command line parameters.
                For example,
                    --bench-tps-args 'tx-count=5000 thread-batch-sleep-ms=250'
                This will start bench-tps clients, and supply '--tx-count 5000 --thread-batch-sleep-ms 250'
                to the bench-tps client."),
        )
        .arg(
            Arg::with_name("target_node")
                .long("target-node")
                .takes_value(true)
                .help("Client Config. Optional: Specify an exact node to send transactions to. use: --target-node <Pubkey>.
                Not supported yet. TODO..."),
        )
        .arg(
            Arg::with_name("duration")
                .long("duration")
                .takes_value(true)
                .default_value("7500")
                .help("Client Config. Seconds to run benchmark, then exit; default is forever use: --duration <SECS>"),
        )
        .arg(
            Arg::with_name("num_nodes")
                .long("num-nodes")
                .short("-N")
                .takes_value(true)
                .help("Client Config. Optional: Wait for NUM nodes to converge: --num-nodes <NUM> "),
        )
        .arg(
            Arg::with_name("run_client")
                .long("run-client")
                .help("Run the client(s)"),
        )
        //Metrics Config
        .arg(
            Arg::with_name("metrics_host")
                .long("metrics-host")
                .takes_value(true)
                .requires_all(&["metrics_port", "metrics_db", "metrics_username", "metrics_password"])
                .help("Metrics Config. Optional: specify metrics host. e.g. https://internal-metrics.solana.com"),
        )
        .arg(
            Arg::with_name("metrics_port")
                .long("metrics-port")
                .takes_value(true)
                .help("Metrics Config. Optional: specify metrics port. e.g. 8086"),
        )
        .arg(
            Arg::with_name("metrics_db")
                .long("metrics-db")
                .takes_value(true)
                .help("Metrics Config. Optional: specify metrics database. e.g. k8s-cluster-<your name>"),
        )
        .arg(
            Arg::with_name("metrics_username")
                .long("metrics-username")
                .takes_value(true)
                .help("Metrics Config. Optional: specify metrics username"),
        )
        .arg(
            Arg::with_name("metrics_password")
                .long("metrics-password")
                .takes_value(true)
                .help("Metrics Config. Optional: Specify metrics password"),
        )
        //RPC config
        .arg(
            Arg::with_name("number_of_non_voting_validators")
                .long("num-non-voting-validators")
                .takes_value(true)
                .default_value("0")
                .help("Number of non voting validators ")
                .validator(|s| match s.parse::<i32>() {
                    Ok(n) if n >= 0 => Ok(()),
                    _ => Err(String::from("number_of_non_voting_validators should be >= 0")),
                }),
        )
        //Kubernetes Config
        .arg(
            Arg::with_name("node_type")
                .long("node-type")
                .possible_values(&["equinix", "lumen", "mixed"])
                .takes_value(true)
                .default_value("mixed")
                .help(
                    "Select the type of node you want to deploy your cluster on. Mixed is default.
                    Note: Lumen nodes have been known to act funky. If seeing issues, try deploying
                    to equinix nodes only."
                ),
        )
        .arg(
            Arg::with_name("cpu_requests")
                .long("cpu-requests")
                .takes_value(true)
                .default_value("20") // 20 cores
                .help("Kubernetes pod config. Specify minimum CPUs required for deploying validator.
                    can use millicore notation as well. e.g. 500m (500 millicores) == 0.5 and is equivalent to half a core.
                    [default: 20]"),
        )
        .arg(
            Arg::with_name("memory_requests")
                .long("memory-requests")
                .takes_value(true)
                .default_value("70Gi") // 70 Gigabytes
                .help("Kubernetes pod config. Specify minimum memory required for deploying validator.
                    Can specify unit here (B, Ki, Mi, Gi, Ti) for bytes, kilobytes, etc (2^N notation)
                    e.g. 1Gi == 1024Mi == 1024Ki == 1,047,576B. [default: 70Gi]"),
        )
        .get_matches()
}

pub const TOTAL_SOL: f64 = 1000000.0; // 1 mil sol to distribute across validators

#[derive(Clone, Debug)]
pub struct SetupConfig<'a> {
    pub namespace: &'a str,
    pub num_validators: i32,
    pub skip_genesis_build: bool,
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "INFO");
    }
    solana_logger::setup();
    let matches = parse_matches();
    initialize_globals();
    let setup_config = SetupConfig {
        namespace: matches.value_of("cluster_namespace").unwrap_or_default(),
        num_validators: value_t_or_exit!(matches, "number_of_validators", i32),
        skip_genesis_build: matches.is_present("skip_genesis_build"),
    };

    let stake_distribution: Option<Vec<u8>> = matches
        .values_of("internal_node_stake_distribution")
        .map(|values| {
            values
                .map(|s| s.parse::<u8>().expect("Invalid percentage in distribution"))
                .collect()
        });

    let (stake_per_bucket, stake_allocations) = match stake_distribution {
        Some(mut dist) => {
            match calculate_stake_allocations(TOTAL_SOL, setup_config.num_validators, &mut dist) {
                Ok((stake_per_bucket, alloc)) => (Some(stake_per_bucket), Some(alloc)),
                Err(err) => {
                    error!("{}", err);
                    std::process::exit(1);
                }
            }
        }
        None => (None, None),
    };

    if let (Some(_), Some(allocations)) = (&stake_per_bucket, &stake_allocations) {
        info!("stake per validator: {:?}", allocations);
    }

    if setup_config.skip_genesis_build
        && !get_solana_root()
            .join("config-k8s/bootstrap-validator")
            .exists()
    {
        error!("Skipping genesis build but there is not previous genesis to use. exiting...");
    }

    let num_non_voting_validators =
        value_t_or_exit!(matches, "number_of_non_voting_validators", i32);

    let client_config = ClientConfig {
        num_clients: value_t_or_exit!(matches, "number_of_clients", i32),
        client_delay_start: matches
            .value_of("client_delay_start")
            .unwrap()
            .parse()
            .expect("Failed to parse client_delay_start into u64"),
        client_type: matches
            .value_of("client_type")
            .unwrap_or_default()
            .to_string(),
        client_to_run: matches
            .value_of("client_to_run")
            .unwrap_or_default()
            .to_string(),
        bench_tps_args: parse_and_format_bench_tps_args(matches.value_of("bench_tps_args")),
        target_node: match matches.value_of("target_node") {
            Some(s) => match s.parse::<Pubkey>() {
                Ok(pubkey) => Some(pubkey),
                Err(e) => return error!("failed to parse pubkey in target_node: {}", e),
            },
            None => None,
        },
        duration: value_t_or_exit!(matches, "duration", u64),
        num_nodes: matches
            .value_of("num_nodes")
            .map(|value_str| value_str.parse().expect("Invalid value for num_nodes")),
        run_client: matches.is_present("run_client"),
    };

    info!("client to run: {}", client_config.client_to_run);

    if let Some(ref bench_tps_args) = client_config.bench_tps_args {
        for s in bench_tps_args.iter() {
            info!("s: {}", s);
        }
    }

    let build_config = BuildConfig::new(
        matches.value_of("release_channel").unwrap_or_default(),
        matches.value_of("deploy_method").unwrap(),
        matches.is_present("do_build"),
        matches.is_present("debug_build"),
        matches.is_present("profile_build"),
        matches.is_present("docker_build"),
    );

    let genesis_flags = GenesisFlags {
        hashes_per_tick: matches
            .value_of("hashes_per_tick")
            .unwrap_or_default()
            .to_string(),
        slots_per_epoch: matches.value_of("slots_per_epoch").map(|value_str| {
            value_str
                .parse()
                .expect("Invalid value for slots_per_epoch")
        }),
        target_lamports_per_signature: matches.value_of("target_lamports_per_signature").map(
            |value_str| {
                value_str
                    .parse()
                    .expect("Invalid value for target_lamports_per_signature")
            },
        ),
        faucet_lamports: matches.value_of("faucet_lamports").map(|value_str| {
            value_str
                .parse()
                .expect("Invalid value for faucet_lamports")
        }),
        enable_warmup_epochs: matches.value_of("enable_warmup_epochs").unwrap() == "true",
        max_genesis_archive_unpacked_size: matches
            .value_of("max_genesis_archive_unpacked_size")
            .map(|value_str| {
                value_str
                    .parse()
                    .expect("Invalid value for max_genesis_archive_unpacked_size")
            }),
        cluster_type: matches
            .value_of("cluster_type")
            .unwrap_or_default()
            .to_string(),
        bootstrap_validator_sol: matches
            .value_of("bootstrap_validator_sol")
            .map(|value_str| {
                value_str
                    .parse()
                    .expect("Invalid value for bootstrap_validator_sol")
            }),
        bootstrap_validator_stake_sol: matches.value_of("bootstrap_validator_stake_sol").map(
            |value_str| {
                value_str
                    .parse()
                    .expect("Invalid value for bootstrap_validator_stake_sol")
            },
        ),
    };

    let mut validator_config = ValidatorConfig {
        tpu_enable_udp: matches.is_present("tpu_enable_udp"),
        tpu_disable_quic: matches.is_present("tpu_disable_quic"),
        gpu_mode: matches.value_of("gpu_mode").unwrap(),
        internal_node_sol: matches
            .value_of("internal_node_sol")
            .unwrap_or(DEFAULT_INTERNAL_NODE_SOL.to_string().as_str())
            .parse::<f64>()
            .expect("Invalid value for internal_node_stake_sol") as f64,
        internal_node_stake_sol: matches
            .value_of("internal_node_stake_sol")
            .unwrap_or(DEFAULT_INTERNAL_NODE_STAKE_SOL.to_string().as_str())
            .parse::<f64>()
            .expect("Invalid value for internal_node_stake_sol")
            as f64,
        wait_for_supermajority: matches.value_of("wait_for_supermajority").map(|value_str| {
            value_str
                .parse()
                .expect("Invalid value for wait_for_supermajority")
        }),
        warp_slot: matches
            .value_of("warp_slot")
            .map(|value_str| value_str.parse().expect("Invalid value for warp_slot")),

        shred_version: None, // set after genesis created
        bank_hash: None,     // set after genesis created
        max_ledger_size: if matches.is_present("limit_ledger_size") {
            let limit_ledger_size = match matches.value_of("limit_ledger_size") {
                Some(_) => value_t_or_exit!(matches, "limit_ledger_size", u64),
                None => DEFAULT_MAX_LEDGER_SHREDS,
            };
            if limit_ledger_size < DEFAULT_MIN_MAX_LEDGER_SHREDS {
                error!(
                    "The provided --limit-ledger-size value was too small, the minimum value is {DEFAULT_MIN_MAX_LEDGER_SHREDS}"
                );
                return;
            }
            Some(limit_ledger_size)
        } else {
            None
        },
        skip_poh_verify: matches.is_present("skip_poh_verify"),
        no_snapshot_fetch: matches.is_present("no_snapshot_fetch"),
        require_tower: matches.is_present("require_tower"),
        enable_full_rpc: matches.is_present("enable_full_rpc"),
        entrypoints: Vec::new(),
        known_validators: None,
    };

    let wait_for_supermajority: Option<u64> = validator_config.wait_for_supermajority;
    let warp_slot: Option<u64> = validator_config.warp_slot;
    if !match (
        validator_config.wait_for_supermajority,
        validator_config.warp_slot,
    ) {
        (Some(slot1), Some(slot2)) => slot1 == slot2,
        (None, None) => true, // Both are None, consider them equal
        _ => true,
    } {
        panic!(
            "Error: When specifying both --wait-for-supermajority and --warp-slot, \
        they must use the same slot. ({:?} != {:?})",
            validator_config.wait_for_supermajority, validator_config.warp_slot
        );
    }

    info!("Runtime Config: {}", validator_config);

    let metrics = if matches.is_present("metrics_host") {
        Some(Metrics::new(
            matches.value_of("metrics_host").unwrap().to_string(),
            matches.value_of("metrics_port").unwrap().to_string(),
            matches.value_of("metrics_db").unwrap().to_string(),
            matches.value_of("metrics_username").unwrap().to_string(),
            matches.value_of("metrics_password").unwrap().to_string(),
        ))
    } else {
        None
    };

    // Get the user-defined node_type
    let node_type = match matches.value_of("node_type").unwrap_or_default() {
        "equinix" => NodeAffinityType::Equinix,
        "lumen" => NodeAffinityType::Lumen,
        "mixed" => NodeAffinityType::Mixed,
        _ => unreachable!(),
    };

    info!("Node Type: {}", node_type);

    let deployment_tag = matches.value_of("deployment_tag").map(|t| t.to_string());
    let no_bootstrap = matches.is_present("no_bootstrap");

    let pod_requests = PodRequests::new(
        matches.value_of("cpu_requests").unwrap().to_string(),
        matches.value_of("memory_requests").unwrap().to_string(),
    );

    // Check if namespace exists
    let mut kub_controller = Kubernetes::new(
        setup_config.namespace,
        &mut validator_config,
        client_config.clone(),
        metrics,
        node_type,
        deployment_tag.clone(),
        pod_requests,
    )
    .await;
    match kub_controller.namespace_exists().await {
        Ok(res) => {
            if !res {
                error!(
                    "Namespace: '{}' doesn't exist. Exiting...",
                    setup_config.namespace
                );
                return;
            }
        }
        Err(err) => {
            error!("{}", err);
            return;
        }
    }

    match kub_controller.set_nodes().await {
        Ok(_) => match kub_controller.nodes() {
            Some(_) => info!(
                "{} nodes available for deployment",
                kub_controller.node_count()
            ),
            None => info!("Deploying to mixed set of kubernetes nodes"),
        },
        Err(err) => {
            error!("{}", err);
            return;
        }
    }

    let deploy = Deploy::new(build_config.clone());
    match deploy.prepare().await {
        Ok(_) => info!("Validator setup prepared successfully"),
        Err(err) => {
            error!("Exiting........ {}", err);
            return;
        }
    }

    if !setup_config.skip_genesis_build {
        // if we are not deploying a bootstrap, we need to use the previous
        // genesis as the genesis for new validators, so do not delete genesis directory
        let retain_previous_genesis = no_bootstrap;
        let mut genesis: Genesis = Genesis::new(genesis_flags, retain_previous_genesis);
        if !no_bootstrap {
            info!("Creating Genesis");
            match genesis.generate_faucet() {
                Ok(_) => (),
                Err(err) => {
                    error!("generate faucet error! {}", err);
                    return;
                }
            }

            match genesis.generate_accounts(ValidatorType::Bootstrap, 1, None) {
                Ok(_) => (),
                Err(err) => {
                    error!("generate accounts error! {}", err);
                    return;
                }
            }

            // only create client accounts once
            if client_config.num_clients > 0 && client_config.client_to_run == "bench-tps" {
                match genesis.create_client_accounts(
                    client_config.num_clients,
                    DEFAULT_CLIENT_LAMPORTS_PER_SIGNATURE,
                    client_config.bench_tps_args,
                    build_config.build_path(),
                ) {
                    Ok(_) => (),
                    Err(err) => {
                        error!("generate client accounts error! {}", err);
                        return;
                    }
                }
            }

            // creates genesis and writes to binary file
            match genesis.generate(build_config.build_path()) {
                Ok(_) => (),
                Err(err) => {
                    error!("generate genesis error! {}", err);
                    return;
                }
            }
        }

        match genesis.generate_accounts(
            ValidatorType::Standard,
            setup_config.num_validators,
            deployment_tag.clone(),
        ) {
            Ok(_) => (),
            Err(err) => {
                error!("generate accounts error! {}", err);
                return;
            }
        }

        match genesis.generate_accounts(
            ValidatorType::NonVoting,
            num_non_voting_validators,
            deployment_tag.clone(),
        ) {
            Ok(_) => (),
            Err(err) => {
                error!("generate non voting accounts error! {}", err);
                return;
            }
        }
    }

    match LedgerHelper::get_shred_version() {
        Ok(shred_version) => kub_controller.set_shred_version(shred_version),
        Err(err) => {
            error!("{}", err);
            return;
        }
    }

    let actual_warp_slot = if warp_slot.is_none() && wait_for_supermajority.is_some() {
        wait_for_supermajority
    } else {
        warp_slot
    };

    if let Some(warp_slot) = actual_warp_slot {
        match LedgerHelper::create_snapshot(warp_slot, build_config.build_path()) {
            Ok(_) => (),
            Err(err) => {
                error!("Failed to create snapshot: {}", err);
                return;
            }
        }
    }

    if wait_for_supermajority.is_some() {
        match LedgerHelper::create_bank_hash(build_config.build_path()) {
            Ok(bank_hash) => kub_controller.set_bank_hash(bank_hash),
            Err(err) => {
                error!("Failed to get bank hash: {}", err);
                return;
            }
        };
    }

    // Download validator version and Build docker image
    let docker_image_config = if build_config.docker_build() && !setup_config.skip_genesis_build {
        Some(DockerImageConfig {
            base_image: matches.value_of("base_image").unwrap_or_default(),
            image_name: matches.value_of("image_name").unwrap(),
            tag: matches.value_of("image_tag").unwrap_or_default(),
            registry: matches.value_of("registry_name").unwrap(),
        })
    } else {
        None
    };

    if let Some(config) = docker_image_config {
        let docker = DockerConfig::new(config, build_config.deploy_method());
        let mut image_types = vec![];
        if !no_bootstrap {
            image_types.push(ValidatorType::Bootstrap);
        }
        if setup_config.num_validators > 0 {
            image_types.push(ValidatorType::Standard);
        }
        if num_non_voting_validators > 0 {
            image_types.push(ValidatorType::NonVoting);
        }

        for image_type in &image_types {
            match docker.build_image(image_type) {
                Ok(_) => info!("Docker image built successfully"),
                Err(err) => {
                    error!("Exiting........ {}", err);
                    return;
                }
            }
        }

        // Need to push image to registry so Monogon nodes can pull image from registry to local
        for image_type in &image_types {
            match docker.push_image(image_type) {
                Ok(_) => info!("{} image built successfully", image_type),
                Err(err) => {
                    error!("Exiting........ {}", err);
                    return;
                }
            }
        }
    }

    let bootstrap_container_name = matches
        .value_of("bootstrap_container_name")
        .unwrap_or_default();
    let bootstrap_image_name = matches
        .value_of("bootstrap_image_name")
        .expect("Bootstrap image name is required");

    //TODO: clean this up. these should just all be the same.
    let validator_container_name = matches
        .value_of("validator_container_name")
        .unwrap_or_default();
    let validator_image_name = matches
        .value_of("validator_image_name")
        .expect("Validator image name is required");

    // Just using validator container for client right now. they're all the same image
    let client_container_name = matches
        .value_of("validator_container_name")
        .unwrap_or_default();
    let client_image_name = matches
        .value_of("validator_image_name")
        .expect("Validator image name is required");

    // Just using validator container for nvv right now. they're all the same image
    let nvv_container_name = matches
        .value_of("validator_container_name")
        .unwrap_or_default();
    let nvv_image_name = matches
        .value_of("validator_image_name")
        .expect("Validator image name is required");

    // secret create once and use by all pods
    if kub_controller.metrics.is_some() && !no_bootstrap {
        let metrics_secret = match kub_controller.create_metrics_secret() {
            Ok(secret) => secret,
            Err(err) => {
                error!("Failed to create metrics secret! {}", err);
                return;
            }
        };
        match kub_controller.deploy_secret(&metrics_secret).await {
            Ok(_) => (),
            Err(err) => {
                error!("{}", err);
                return;
            }
        }
    };

    if !no_bootstrap {
        let bootstrap_secret =
            match kub_controller.create_bootstrap_secret("bootstrap-accounts-secret") {
                Ok(secret) => secret,
                Err(err) => {
                    error!("Failed to create bootstrap secret! {}", err);
                    return;
                }
            };
        match kub_controller.deploy_secret(&bootstrap_secret).await {
            Ok(_) => (),
            Err(err) => {
                error!("{}", err);
                return;
            }
        }

        // Bootstrap needs two labels. Because it is going to have two services. One via LB, one direct
        let mut bootstrap_rs_labels =
            kub_controller.create_selector("app.kubernetes.io/lb", "load-balancer-selector");
        bootstrap_rs_labels.insert(
            "app.kubernetes.io/name".to_string(),
            "bootstrap-validator-selector".to_string(),
        );
        bootstrap_rs_labels.insert(
            "app.kubernetes.io/type".to_string(),
            "bootstrap".to_string(),
        );

        let identity_path = get_solana_root().join("config-k8s/bootstrap-validator/identity.json");
        let bootstrap_keypair =
            read_keypair_file(identity_path).expect("Failed to read bootstrap keypair file");
        bootstrap_rs_labels.insert(
            "app.kubernetes.io/identity".to_string(),
            bootstrap_keypair.pubkey().to_string(),
        );

        let bootstrap_replica_set = match kub_controller.create_bootstrap_validator_replica_set(
            bootstrap_container_name,
            bootstrap_image_name,
            bootstrap_secret.metadata.name.clone(),
            &bootstrap_rs_labels,
        ) {
            Ok(replica_set) => replica_set,
            Err(err) => {
                error!("Error creating bootstrap validator replicas_set: {}", err);
                return;
            }
        };
        let bootstrap_replica_set_name = match kub_controller
            .deploy_replicas_set(&bootstrap_replica_set)
            .await
        {
            Ok(replica_set) => {
                info!("bootstrap validator replicas_set deployed successfully");
                replica_set.metadata.name.unwrap()
            }
            Err(err) => {
                error!(
                    "Error! Failed to deploy bootstrap validator replicas_set. err: {:?}",
                    err
                );
                return;
            }
        };

        let bootstrap_service_label = kub_controller
            .create_selector("app.kubernetes.io/name", "bootstrap-validator-selector");
        let bootstrap_service = kub_controller
            .create_bootstrap_service("bootstrap-validator-service", &bootstrap_service_label);
        match kub_controller.deploy_service(&bootstrap_service).await {
            Ok(_) => info!("bootstrap validator service deployed successfully"),
            Err(err) => error!(
                "Error! Failed to deploy bootstrap validator service. err: {:?}",
                err
            ),
        }

        //load balancer service. only create one and use for all deployments
        let load_balancer_label =
            kub_controller.create_selector("app.kubernetes.io/lb", "load-balancer-selector");
        //create load balancer
        let load_balancer = kub_controller.create_validator_load_balancer(
            "bootstrap-and-non-voting-lb-service",
            &load_balancer_label,
        );

        //deploy load balancer
        match kub_controller.deploy_service(&load_balancer).await {
            Ok(_) => info!("load balancer service deployed successfully"),
            Err(err) => error!(
                "Error! Failed to deploy load balancer service. err: {:?}",
                err
            ),
        }

        // wait for bootstrap replicaset to deploy
        while {
            match kub_controller
                .check_replica_set_ready(bootstrap_replica_set_name.as_str())
                .await
            {
                Ok(ok) => !ok, // Continue the loop if replica set is not ready: Ok(false)
                Err(_) => panic!("Error occurred while checking replica set readiness"),
            }
        } {
            info!("replica set: {} not ready...", bootstrap_replica_set_name);
            thread::sleep(Duration::from_secs(1));
        }
        info!("replica set: {} Ready!", bootstrap_replica_set_name);
    }

    //Create and deploy non-voting validators and faucet behind a load balancer
    // NonVoting nodes also need 2 selectors. 1 for load balancer, 1 for direct
    if num_non_voting_validators > 0 {
        // we need one load balancer for all of these nv validators...
        let mut non_voting_validators = vec![];
        for nvv_index in 0..num_non_voting_validators {
            let mut nvv_rs_labels = kub_controller.create_selector(
                "app.kubernetes.io/name",
                format!("non-voting-selector-{}", nvv_index).as_str(),
            );
            nvv_rs_labels.insert(
                "app.kubernetes.io/lb".to_string(),
                "load-balancer-selector".to_string(),
            );
            nvv_rs_labels.insert(
                "app.kubernetes.io/type".to_string(),
                "non-voting".to_string(),
            );

            let mut validator_with_optional_tag = "non-voting-validator".to_string();
            if let Some(tag) = &deployment_tag {
                validator_with_optional_tag =
                    add_tag_to_name(validator_with_optional_tag.as_str(), tag);
            }

            let identity_path = get_solana_root().join(format!(
                "config-k8s/{}-identity-{}.json",
                validator_with_optional_tag, nvv_index
            ));
            let nvv_keypair = read_keypair_file(identity_path)
                .expect("Failed to read non voting validator keypair file");
            nvv_rs_labels.insert(
                "app.kubernetes.io/identity".to_string(),
                nvv_keypair.pubkey().to_string(),
            );

            let nvv_secret = match kub_controller.create_non_voting_secret(nvv_index) {
                Ok(secret) => secret,
                Err(err) => {
                    error!("Failed to create nvv secret! {}", err);
                    return;
                }
            };
            match kub_controller.deploy_secret(&nvv_secret).await {
                Ok(_) => (),
                Err(err) => {
                    error!("{}", err);
                    return;
                }
            }

            let nvv_replica_set = match kub_controller.create_non_voting_validator_replica_set(
                nvv_container_name,
                nvv_index,
                nvv_image_name,
                nvv_secret.metadata.name.clone(),
                &nvv_rs_labels,
            ) {
                Ok(replica_set) => replica_set,
                Err(err) => {
                    error!("Error creating non voting validator replicas_set: {}", err);
                    return;
                }
            };

            // deploy replica set
            let nvv_replica_set_name =
                match kub_controller.deploy_replicas_set(&nvv_replica_set).await {
                    Ok(rs) => {
                        info!(
                            "non voting validator replica set ({}) deployed successfully",
                            nvv_index
                        );
                        rs.metadata.name.unwrap()
                    }
                    Err(err) => {
                        error!(
                        "Error! Failed to deploy non voting validator replica set: {}. err: {:?}",
                        nvv_index, err
                    );
                        return;
                    }
                };
            non_voting_validators.push(nvv_replica_set_name);

            //create nvv service
            let non_voting_label = kub_controller.create_selector(
                "app.kubernetes.io/name",
                format!("non-voting-selector-{}", nvv_index).as_str(),
            );
            let nvv_service = kub_controller.create_validator_service(
                "non-voting-service",
                &non_voting_label,
                nvv_index,
            );

            //deploy nvv service
            match kub_controller.deploy_service(&nvv_service).await {
                Ok(_) => info!("nvv service deployed successfully"),
                Err(err) => error!(
                    "Error! Failed to deploy non voting validator service. err: {:?}",
                    err
                ),
            }
        }

        // wait for at least one non voting validator replicaset to deploy
        loop {
            let mut one_nvv_ready = false;
            for nvv in &non_voting_validators {
                match kub_controller.check_replica_set_ready(nvv.as_str()).await {
                    Ok(ready) => {
                        if ready {
                            one_nvv_ready = true;
                            break;
                        }
                    } // Continue the loop if replica set is not ready: Ok(false)
                    Err(_) => panic!("Error occurred while checking faucet replica set readiness"),
                }
            }

            if one_nvv_ready {
                break;
            }

            info!("no non voting validator replica sets ready");
            thread::sleep(Duration::from_secs(10));
        }
        info!(">= 1 non voting validator ready");
    }

    // Create and deploy validators
    for validator_index in 0..setup_config.num_validators {
        let validator_secret = match kub_controller.create_validator_secret(validator_index) {
            Ok(secret) => secret,
            Err(err) => {
                error!("Failed to create validator secret! {}", err);
                return;
            }
        };
        match kub_controller.deploy_secret(&validator_secret).await {
            Ok(_) => (),
            Err(err) => {
                error!("{}", err);
                return;
            }
        }

        let mut validator_labels = kub_controller.create_selector(
            "app.kubernetes.io/name",
            format!("validator-{}", validator_index).as_str(),
        );
        validator_labels.insert(
            "app.kubernetes.io/type".to_string(),
            "validator".to_string(),
        );

        let mut validator_with_optional_tag = "validator".to_string();
        if let Some(tag) = &deployment_tag {
            validator_with_optional_tag =
                add_tag_to_name(validator_with_optional_tag.as_str(), tag);
        }

        let identity_path = get_solana_root().join(format!(
            "config-k8s/{}-identity-{}.json",
            validator_with_optional_tag, validator_index
        ));
        let validator_keypair =
            read_keypair_file(identity_path).expect("Failed to read validator keypair file");
        validator_labels.insert(
            "app.kubernetes.io/identity".to_string(),
            validator_keypair.pubkey().to_string(),
        );

        let stake = stake_allocations
            .as_ref()
            .and_then(|stake_vec| stake_vec.get(validator_index as usize))
            .copied();

        let stake_value = stake
            .map(|s| s.to_string())
            .unwrap_or(DEFAULT_INTERNAL_NODE_STAKE_SOL.to_string());

        validator_labels.insert("app.kubernetes.io/stake_in_sol".to_string(), stake_value);

        let validator_replica_set = match kub_controller.create_validator_replica_set(
            validator_container_name,
            validator_index,
            validator_image_name,
            validator_secret.metadata.name.clone(),
            &validator_labels,
            &stake,
        ) {
            Ok(replica_set) => replica_set,
            Err(err) => {
                error!("Error creating validator replicas_set: {}", err);
                return;
            }
        };

        let _ = match kub_controller
            .deploy_replicas_set(&validator_replica_set)
            .await
        {
            Ok(rs) => {
                info!(
                    "validator replica set ({}) deployed successfully",
                    validator_index
                );
                rs.metadata.name.unwrap()
            }
            Err(err) => {
                error!(
                    "Error! Failed to deploy validator replica set: {}. err: {:?}",
                    validator_index, err
                );
                return;
            }
        };

        let validator_service = kub_controller.create_validator_service(
            "validator-service",
            &validator_labels,
            validator_index,
        );
        match kub_controller.deploy_service(&validator_service).await {
            Ok(_) => info!(
                "validator service ({}) deployed successfully",
                validator_index
            ),
            Err(err) => error!(
                "Error! Failed to deploy validator service: {}. err: {:?}",
                validator_index, err
            ),
        }
    }

    if !client_config.run_client || client_config.num_clients <= 0 {
        return;
    }

    //TODO, we need to wait for all validators to actually be deployed before starting the client.
    // Aka need to somehow check that either all validators are connected via gossip and through tpu
    // or we just check that their replica sets are 1/1
    info!(
        "Waiting for client_delay_start: {} seconds",
        client_config.client_delay_start
    );
    thread::sleep(Duration::from_secs(client_config.client_delay_start));

    for client_index in 0..client_config.num_clients {
        info!("deploying client: {}", client_index);
        let client_secret = match kub_controller.create_client_secret(client_index) {
            Ok(secret) => secret,
            Err(err) => {
                error!("Failed to create client secret! {}", err);
                return;
            }
        };
        match kub_controller.deploy_secret(&client_secret).await {
            Ok(_) => (),
            Err(err) => {
                error!("{}", err);
                return;
            }
        }

        let label_selector = kub_controller.create_selector(
            "app.kubernetes.io/name",
            format!("client-{}", client_index).as_str(),
        );

        let client_replica_set = match kub_controller.create_client_replica_set(
            client_container_name,
            client_index,
            client_image_name,
            client_secret.metadata.name.clone(),
            &label_selector,
        ) {
            Ok(replica_set) => replica_set,
            Err(err) => {
                error!("Error creating client replicas_set: {}", err);
                return;
            }
        };

        let _ = match kub_controller
            .deploy_replicas_set(&client_replica_set)
            .await
        {
            Ok(rs) => {
                info!(
                    "client replica set ({}) deployed successfully",
                    client_index
                );
                rs.metadata.name.unwrap()
            }
            Err(err) => {
                error!(
                    "Error! Failed to deploy client replica set: {}. err: {:?}",
                    client_index, err
                );
                return;
            }
        };

        let client_service = kub_controller.create_validator_service(
            "client-service",
            &label_selector,
            client_index,
        );
        match kub_controller.deploy_service(&client_service).await {
            Ok(_) => info!("client service ({}) deployed successfully", client_index),
            Err(err) => error!(
                "Error! Failed to deploy client service: {}. err: {:?}",
                client_index, err
            ),
        }
    }
}
