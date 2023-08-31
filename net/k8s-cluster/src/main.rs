use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg, ArgMatches, value_t},
    k8s_openapi::{
        api::{
            apps::v1::{Deployment, DeploymentSpec},
            core::v1::{
                Container, EnvVar, EnvVarSource, ObjectFieldSelector, PodSpec, PodTemplateSpec,
                Service, ServicePort, ServiceSpec,
            },
        },
        apimachinery::pkg::apis::meta::v1::LabelSelector,
    },
    kube::{
        api::{Api, ObjectMeta, PostParams},
        Client,
    },
    log::*,
    serde_json, serde_yaml,
    solana_k8s_cluster::{
        docker::{DockerConfig, DockerImageConfig},
        genesis::{Genesis, SetupConfig},
        get_solana_root, initialize_globals,
        kubernetes::Kubernetes,
        release::{BuildConfig, Deploy},
        ValidatorType,
    },
    solana_sdk::genesis_config::GenesisConfig,
    std::{collections::BTreeMap, fs::File, io::Write, process, thread, time::Duration},
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
                .help("Build Docker images"),
        )
        .arg(
            Arg::with_name("base_image")
                .long("base-image")
                .takes_value(true)
                .default_value("ubuntu:20.04")
                .help("Docker base image"),
        )
        .arg(
            Arg::with_name("image_tag")
                .long("image-tag")
                .takes_value(true)
                .default_value("k8s-cluster-image")
                .help("Docker image tag. tag will be prepended with `bootstrap-` and `validator-` to distinguish between both images"),
        )
        .arg(
            Arg::with_name("prebuild_genesis")
                .long("prebuild-genesis")
                .help("Prebuild gensis. Generates keys for validators and writes to file"),
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

    // Check if namespace exists
    let mut kub_controller = Kubernetes::new(setup_config.namespace).await;
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
    let docker_image_config = DockerImageConfig {
        base_image: matches.value_of("base_image").unwrap_or_default(),
        tag: matches.value_of("image_tag").unwrap_or_default(),
    };

    let deploy = Deploy::new(build_config.clone());
    match deploy.prepare().await {
        Ok(_) => info!("Validator setup prepared successfully"),
        Err(err) => {
            error!("Exiting........ {}", err);
            return;
        }
    }

    if build_config.docker_build {
        let docker = DockerConfig::new(docker_image_config, build_config.deploy_method);
        let image_types = vec!["bootstrap", "validator"];
        for image_type in image_types {
            match docker.build_image(image_type).await {
                Ok(_) => info!("Validator Docker image built successfully"),
                Err(err) => {
                    error!("Exiting........ {}", err);
                    return;
                }
            }
        }
    }

    let mut genesis = Genesis::new(setup_config.clone());
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

    let base64_genesis_string = match genesis.load_genesis_to_base64_from_file() {
        Ok(genesis_string) => genesis_string,
        Err(err) => {
            error!("Failed to load genesis from file! {}", err);
            return;
        }
    };

    let loaded_config =
        GenesisConfig::load_from_base64_string(base64_genesis_string.as_str()).expect("load");
    info!("loaded_config_hash: {}", loaded_config.hash());

    let config_map = match kub_controller
        .create_config_map(base64_genesis_string)
        .await
    {
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
        .expect("Bootstrap container name is required");
    let bootstrap_image_name = matches
        .value_of("bootstrap_image_name")
        .expect("Bootstrap image name is required");
    let validator_container_name = matches
        .value_of("validator_container_name")
        .expect("Validator container name is required");
    let validator_image_name = matches
        .value_of("validator_image_name")
        .expect("Validator image name is required");

    kub_controller.create_selector(
        &ValidatorType::Bootstrap,
        "app.kubernetes.io/name",
        "bootstrap-validator",
    );
    let bootstrap_deployment = match kub_controller.create_bootstrap_validator_deployment(
        bootstrap_container_name,
        bootstrap_image_name,
        BOOTSTRAP_VALIDATOR_REPLICAS,
        config_map.metadata.name.clone(),
    ) {
        Ok(deployment) => deployment,
        Err(err) => {
            error!("Error creating bootstrap validator deployment: {}", err);
            return;
        }
    };
    let dep_name = match kub_controller
        .deploy_deployment(&bootstrap_deployment)
        .await
    {
        Ok(dep) => {
            info!("bootstrap validator deployment deployed successfully");
            dep.metadata.name.unwrap()
        }
        Err(err) => {
            error!(
                "Error! Failed to deploy bootstrap validator deployment. err: {:?}",
                err
            );
            err.to_string() //TODO: fix this, should handle this error better. shoudn't just return a string. should exit or something better
        }
    };

    let bootstrap_service = kub_controller.create_bootstrap_validator_service();
    match kub_controller.deploy_service(&bootstrap_service).await {
        Ok(_) => info!("bootstrap validator service deployed successfully"),
        Err(err) => error!(
            "Error! Failed to deploy bootstrap validator service. err: {:?}",
            err
        ),
    }

    //TODO: handle this return val properly, don't just unwrap
    while !kub_controller
        .check_deployment_ready(dep_name.as_str())
        .await
        .unwrap()
    {
        info!("deployment: {} not ready...", dep_name);
        thread::sleep(Duration::from_secs(1));
    }
    info!("deployment: {} Ready!", dep_name);

    kub_controller.create_selector(
        &ValidatorType::Standard,
        "app.kubernetes.io/name",
        "validator",
    );
    let validator_deployment = match kub_controller.create_validator_deployment(
        validator_container_name,
        validator_image_name,
        setup_config.num_validators,
        config_map.metadata.name,
    ) {
        Ok(deployment) => deployment,
        Err(err) => {
            error!("Error creating validator deployment: {}", err);
            return;
        }
    };

    match kub_controller
        .deploy_deployment(&validator_deployment)
        .await
    {
        Ok(_) => info!("validator deployment deployed successfully"),
        Err(err) => error!(
            "Error! Failed to deploy validator deployment. err: {:?}",
            err
        ),
    }

    let validator_service = kub_controller.create_validator_service();
    match kub_controller.deploy_service(&validator_service).await {
        Ok(_) => info!("validator service deployed successfully"),
        Err(err) => error!("Error! Failed to deploy validator service. err: {:?}", err),
    }

    let _ = kub_controller
        .check_service_matching_deployment("bootstrap-validator")
        .await;
    let _ = kub_controller
        .check_service_matching_deployment("validator")
        .await;
}
