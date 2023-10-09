use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg, ArgMatches},
    log::*,
    solana_k8s_cluster::{
        docker::{DockerConfig, DockerImageConfig},
        genesis::{
            Genesis, GenesisFlags, SetupConfig, DEFAULT_INTERNAL_NODE_SOL,
            DEFAULT_INTERNAL_NODE_STAKE_SOL,
        },
        initialize_globals,
        kubernetes::{Kubernetes, ValidatorConfig},
        ledger_helper::LedgerHelper,
        release::{BuildConfig, Deploy},
        ValidatorType,
    },
    solana_core::ledger_cleanup_service::{DEFAULT_MAX_LEDGER_SHREDS, DEFAULT_MIN_MAX_LEDGER_SHREDS},
    std::{thread, time::Duration},
};

const BOOTSTRAP_VALIDATOR_REPLICAS: i32 = 1;

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
                    _ => Err(String::from("number_of_validators should be greater than 0")),
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
        // Genesis config
        .arg(
            Arg::with_name("prebuild_genesis")
                .long("prebuild-genesis")
                .help("Prebuild gensis. Generates keys for validators and writes to file"),
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
        .get_matches()
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
        prebuild_genesis: matches.is_present("prebuild_genesis"),
    };

    let build_config = BuildConfig {
        release_channel: matches.value_of("release_channel").unwrap_or_default(),
        deploy_method: matches.value_of("deploy_method").unwrap(),
        do_build: matches.is_present("do_build"),
        debug_build: matches.is_present("debug_build"),
        profile_build: matches.is_present("profile_build"),
        docker_build: matches.is_present("docker_build"),
    };

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
        }
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

    // Check if namespace exists
    let mut kub_controller = Kubernetes::new(setup_config.namespace, &mut validator_config).await;
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

    // Download validator version and Build docker image
    let docker_image_config = if build_config.docker_build {
        Some(DockerImageConfig {
            base_image: matches.value_of("base_image").unwrap_or_default(),
            image_name: matches.value_of("image_name").unwrap(),
            tag: matches.value_of("image_tag").unwrap_or_default(),
            registry: matches.value_of("registry_name").unwrap(),
        })
    } else {
        None
    };

    let deploy = Deploy::new(build_config.clone());
    match deploy.prepare().await {
        Ok(_) => info!("Validator setup prepared successfully"),
        Err(err) => {
            error!("Exiting........ {}", err);
            return;
        }
    }

    if let Some(config) = docker_image_config {
        let docker = DockerConfig::new(config, build_config.deploy_method);
        let image_types = vec![ValidatorType::Bootstrap, ValidatorType::Standard];
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
        match docker.push_image(&ValidatorType::Bootstrap) {
            Ok(_) => info!("Bootstrap Image pushed successfully to registry"),
            Err(err) => {
                error!("{}", err);
                return;
            }
        }

        // Need to push image to registry so Monogon nodes can pull image from registry to local
        match docker.push_image(&ValidatorType::Standard) {
            Ok(_) => info!("Validator Image pushed successfully to registry"),
            Err(err) => {
                error!("{}", err);
                return;
            }
        }
    }

    info!("Creating Genesis");
    let mut genesis = Genesis::new(genesis_flags);
    match genesis.generate_faucet() {
        Ok(_) => (),
        Err(err) => {
            error!("generate faucet error! {}", err);
            return;
        }
    }
    match genesis.generate_accounts(ValidatorType::Bootstrap, 1) {
        Ok(_) => (),
        Err(err) => {
            error!("generate accounts error! {}", err);
            return;
        }
    }

    match genesis.generate_accounts(ValidatorType::Standard, setup_config.num_validators) {
        Ok(_) => (),
        Err(err) => {
            error!("generate accounts error! {}", err);
            return;
        }
    }

    // creates genesis and writes to binary file
    match genesis.generate() {
        Ok(_) => (),
        Err(err) => {
            error!("generate genesis error! {}", err);
            return;
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
        match LedgerHelper::create_snapshot(warp_slot) {
            Ok(_) => (),
            Err(err) => {
                error!("Failed to create snapshot: {}", err);
                return;
            }
        }
    }

    if wait_for_supermajority.is_some() {
        match LedgerHelper::create_bank_hash() {
            Ok(bank_hash) => kub_controller.set_bank_hash(bank_hash),
            Err(err) => {
                error!("Failed to get bank hash: {}", err);
                return;
            }
        };
    }

    match genesis.package_up() {
        Ok(_) => (),
        Err(err) => {
            error!("package genesis error! {}", err);
            return;
        }
    }

    // std::process::exit(-1);

    // Begin Kubernetes Setup and Deployment
    let config_map = match kub_controller.create_genesis_config_map().await {
        Ok(config_map) => {
            info!("successfully deployed config map");
            config_map
        }
        Err(err) => {
            error!("Failed to deploy config map: {}", err);
            return;
        }
    };

    let bootstrap_container_name = matches
        .value_of("bootstrap_container_name")
        .unwrap_or_default();
    let bootstrap_image_name = matches
        .value_of("bootstrap_image_name")
        .expect("Bootstrap image name is required");
    let validator_container_name = matches
        .value_of("validator_container_name")
        .unwrap_or_default();
    let validator_image_name = matches
        .value_of("validator_image_name")
        .expect("Validator image name is required");

    let bootstrap_secret = match kub_controller.create_bootstrap_secret("bootstrap-accounts-secret")
    {
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

    let label_selector =
        kub_controller.create_selector("app.kubernetes.io/name", "bootstrap-validator");
    let bootstrap_replica_set = match kub_controller
        .create_bootstrap_validator_replicas_set(
            bootstrap_container_name,
            bootstrap_image_name,
            BOOTSTRAP_VALIDATOR_REPLICAS,
            config_map.metadata.name.clone(),
            bootstrap_secret.metadata.name.clone(),
            &label_selector,
        )
        .await
    {
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

    let bootstrap_service =
        kub_controller.create_validator_service("bootstrap-validator", &label_selector);
    match kub_controller.deploy_service(&bootstrap_service).await {
        Ok(_) => info!("bootstrap validator service deployed successfully"),
        Err(err) => error!(
            "Error! Failed to deploy bootstrap validator service. err: {:?}",
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

        let label_selector = kub_controller.create_selector(
            "app.kubernetes.io/name",
            format!("validator-{}", validator_index).as_str(),
        );

        let validator_replica_set = match kub_controller
            .create_validator_replicas_set(
                validator_container_name,
                validator_index,
                validator_image_name,
                1,
                config_map.metadata.name.clone(),
                validator_secret.metadata.name.clone(),
                &label_selector,
            )
            .await
        {
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
            format!("validator-{}", validator_index).as_str(),
            &label_selector,
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
}
