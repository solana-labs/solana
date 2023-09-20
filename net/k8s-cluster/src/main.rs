use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg, ArgMatches},
    log::*,
    solana_k8s_cluster::{
        docker::{DockerConfig, DockerImageConfig},
        genesis::{Genesis, GenesisFlags, SetupConfig, DEFAULT_INTERNAL_NODE_STAKE_SOL, DEFAULT_INTERNAL_NODE_SOL},
        initialize_globals,
        kubernetes::{Kubernetes, RuntimeConfig},
        release::{BuildConfig, Deploy},
        ledger_helper::LedgerHelper,
    },
    solana_sdk::genesis_config::ClusterType,
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
                .default_value("")
                .help("Optional: release version. e.g. v1.16.5"),
        )
        .arg(
            Arg::with_name("deploy_method")
                .long("deploy-method")
                .takes_value(true)
                .possible_values(&["local", "tar", "skip"])
                .default_value("local"), // .help("Deploy method. tar, local, skip. [default: local]"),
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
                .help("Build Docker images. If not set, will assume local docker image should be used"),
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
                .help("Genesis config. hashes per tick"),
        )
        .arg(
            Arg::with_name("slots_per_epoch")
                .long("slots-per-epoch")
                .takes_value(true)
                .help("Genesis config. slots per epoch"),
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
                .help(" Amount to stake internal nodes (Sol)."),
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
                .possible_values(&ClusterType::STRINGS)
                .takes_value(true)
                .help(
                    "Selects the features that will be enabled for the cluster"
                ),
        )
        .arg(
            Arg::with_name("tpu_enable_udp")
                .long("tpu-enable-udp")
                .help("Runtime config. Enable UDP for tpu transactions."),
        )
        .arg(
            Arg::with_name("tpu_disable_quic")
                .long("tpu-disable-quic")
                .help("Genesis config. Disable quic for tpu packet forwarding"),
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
            Arg::with_name("limit_ledger_size")
                .long("limit-ledger-size")
                .takes_value(true)
                .help("Keep this amount of shreds in root slots"),
        )
        .arg(
            Arg::with_name("full_rpc")
                .long("full-rpc")
                .help("Support full RPC services on all nodes"),
        )
        .arg(
            Arg::with_name("skip_poh_verify")
                .long("skip-poh-verify")
                .help("If set, validators will skip verifying \
                    the ledger they already have saved to disk at \
                    boot (results in a much faster boot)"),
        )
        .arg(
            Arg::with_name("tmpfs_accounts")
                .long("tmpfs-accounts")
                .help("Put accounts directory on a swap-backed tmpfs volume. from gce.sh \
                tbh i have no idea if this is going to be supported out of box. TODO"),
        )
        .arg(
            Arg::with_name("no_snapshot_fetch")
                .long("no-snapshot-fetch")
                .help("If set, disables booting validators from a snapshot"),
        )
        .arg(
            Arg::with_name("accounts_db_skip_shrink")
                .long("accounts-db-skip-shrink")
                .help("not full sure what this does. TODO"),
        )
        .arg(
            Arg::with_name("skip_require_tower")
                .long("skip-require-tower")
                .help("Skips require tower"),
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
        hashes_per_tick: match matches.value_of("hashes_per_tick") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for hashes_per_tick"),
            ),
            None => None,
        },
        slots_per_epoch: match matches.value_of("slots_per_epoch") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for slots_per_epoch"),
            ),
            None => None,
        },
        target_lamports_per_signature: match matches.value_of("target_lamports_per_signature") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for target_lamports_per_signature"),
            ),
            None => None,
        },
        faucet_lamports: match matches.value_of("faucet_lamports") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for faucet_lamports"),
            ),
            None => None,
        },
        enable_warmup_epochs: matches.value_of("enable_warmup_epochs").unwrap() == "true",
        max_genesis_archive_unpacked_size: match matches
            .value_of("max_genesis_archive_unpacked_size")
        {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for max_genesis_archive_unpacked_size"),
            ),
            None => None,
        },
        cluster_type: match matches.value_of("cluster_type") {
            Some(value_str) => Some(value_str.parse().expect("Invalid value for cluster_type")),
            None => None,
        },
        bootstrap_validator_sol: match matches.value_of("bootstrap_validator_sol") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for bootstrap_validator_sol"),
            ),
            None => None,
        },
        bootstrap_validator_stake_sol: match matches
            .value_of("bootstrap_validator_stake_sol")
        {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for bootstrap_validator_stake_sol"),
            ),
            None => None,
        },
    };

    let mut runtime_config = RuntimeConfig {
        enable_udp: matches.is_present("tpu_enable_udp"),
        disable_quic: matches.is_present("tpu_disable_quic"),
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
            .expect("Invalid value for internal_node_stake_sol") as f64,
        limit_ledger_size: match matches.value_of("limit_ledger_size") {
                Some(value_str) => Some(
                    value_str
                        .parse()
                        .expect("Invalid value for limit_ledger_size"),
                ),
                None => None,
            },
        full_rpc: matches.is_present("full_rpc"),
        skip_poh_verify: matches.is_present("skip_poh_verify"),
        tmpfs_accounts: matches.is_present("tmps_accounts"),
        no_snapshot_fetch: matches.is_present("no_snapshot_fetch"),
        accounts_db_skip_shrink: matches.is_present("accounts_db_skip_shrink"),
        skip_require_tower: matches.is_present("skip_require_tower"),
        wait_for_supermajority: match matches.value_of("wait_for_supermajority") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for wait_for_supermajority"),
            ),
            None => None,
        },
        warp_slot: match matches.value_of("warp_slot") {
            Some(value_str) => Some(
                value_str
                    .parse()
                    .expect("Invalid value for warp_slot"),
            ),
            None => None,
        },
        shred_version: None, // set after genesis created
    };

    if ! match (runtime_config.wait_for_supermajority, runtime_config.warp_slot) {
        (Some(slot1), Some(slot2)) => slot1 == slot2,
        (None, None) => true, // Both are None, consider them equal
        _ => true,
    } {
        panic!("Error: When specifying both --wait-for-supermajority and --warp-slot, \
        they must use the same slot. ({:?} != {:?})", runtime_config.wait_for_supermajority, runtime_config.warp_slot);
    }

    info!("Runtime Config: {}", runtime_config);

    // Check if namespace exists
    let mut kub_controller = Kubernetes::new(setup_config.namespace, &mut runtime_config).await;
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

    match docker_image_config {
        Some(config) => {
            let docker = DockerConfig::new(config, build_config.deploy_method);
            let image_types = vec!["bootstrap", "validator"];
            for image_type in image_types {
                match docker.build_image(image_type).await {
                    Ok(_) => info!("Docker image built successfully"),
                    Err(err) => {
                        error!("Exiting........ {}", err);
                        return;
                    }
                }
            }

            // Need to push image to registry so Monogon nodes can pull image from registry to local
            match docker.push_image("bootstrap").await {
                Ok(_) => info!("Bootstrap Image pushed successfully to registry"),
                Err(err) => {
                    error!("{}", err);
                    return;
                }
            }

            // Need to push image to registry so Monogon nodes can pull image from registry to local
            match docker.push_image("validator").await {
                Ok(_) => info!("Validator Image pushed successfully to registry"),
                Err(err) => {
                    error!("{}", err);
                    return;
                }
            }
        }
        _ => (),
    }

    info!("Creating Genesis");
    let mut genesis = Genesis::new(genesis_flags);
    // genesis.generate();
    match genesis.generate_faucet() {
        Ok(_) => (),
        Err(err) => {
            error!("generate faucet error! {}", err);
            return;
        }
    }
    match genesis.generate_accounts("bootstrap", 1) {
        Ok(_) => (),
        Err(err) => {
            error!("generate accounts error! {}", err);
            return;
        }
    }

    match genesis.generate_accounts("validator", setup_config.num_validators) {
        Ok(_) => (),
        Err(err) => {
            error!("generate accounts error! {}", err);
            return;
        }
    }

    // we need to convert the binrary to base64 before we save it to a file
    // or we create the normal genesis.bin but then we take another step
    // and that's when we convert it to base64 and put into our config map
    // then when we load it from the rust code running in the pod,
    // we have to parse the base64, then convert to binary, and then load().
    match genesis.generate() {
        Ok(_) => (),
        Err(err) => {
            error!("generate genesis error! {}", err);
            return;
        }
    }

    // load genesis (test -> this should run in pod)
    match genesis.verify_genesis_from_file() {
        Ok(_) => (),
        Err(err) => {
            error!("Failed verify genesis from file! err: {}", err);
            return;
        }
    }

    kub_controller.set_shred_version(LedgerHelper::get_shred_version());

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
    let rs_name = match kub_controller
        .deploy_replicas_set(&bootstrap_replica_set)
        .await
    {
        Ok(rs) => {
            info!("bootstrap validator replicas_set deployed successfully");
            rs.metadata.name.unwrap()
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

    //TODO: handle this return val properly, don't just unwrap
    while !kub_controller
        .check_replica_set_ready(rs_name.as_str())
        .await
        .unwrap()
    {
        info!("replica set: {} not ready...", rs_name);
        thread::sleep(Duration::from_secs(1));
    }
    info!("replica set: {} Ready!", rs_name);

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
                info!("validator replicas_sets deployed successfully");
                rs.metadata.name.unwrap()
            }
            Err(err) => {
                error!(
                    "Error! Failed to deploy validator replicas_sets. err: {:?}",
                    err
                );
                return;
            }
        };

        let validator_service = kub_controller.create_validator_service(
            format!("validator-{}", validator_index).as_str(),
            &label_selector,
        );
        match kub_controller.deploy_service(&validator_service).await {
            Ok(_) => info!("validator service deployed successfully"),
            Err(err) => error!("Error! Failed to deploy validator service. err: {:?}", err),
        }

        // thread::sleep(Duration::from_secs(2));
    }

    // //TODO: handle this return val properly, don't just unwrap
    // //TODO: not sure this checks for all replica sets
    // while !kub_controller
    //     .check_replica_set_ready(validator_rs_name.as_str())
    //     .await
    //     .unwrap()
    // {
    //     info!("replica set: {} not ready...", validator_rs_name);
    //     thread::sleep(Duration::from_secs(1));
    // }
    // info!("replica set: {} Ready!", rs_name);

    // let _ = kub_controller
    //     .check_service_matching_replica_set("bootstrap-validator")
    //     .await;
    // let _ = kub_controller
    //     .check_service_matching_replica_set("validator")
    //     .await;
}
