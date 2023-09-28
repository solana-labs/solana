use {
    clap::{crate_description, crate_name, App, Arg, ArgMatches},
    log::*,
    solana_k8s_cluster::{
        docker::{DockerConfig, DockerImageConfig},
        initialize_globals,
        release::{BuildConfig, Deploy},
    },
};

fn parse_matches() -> ArgMatches<'static> {
    App::new(crate_name!())
        .about(crate_description!())
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

    let build_config = BuildConfig {
        release_channel: matches.value_of("release_channel").unwrap_or_default(),
        deploy_method: matches.value_of("deploy_method").unwrap(),
        do_build: matches.is_present("do_build"),
        debug_build: matches.is_present("debug_build"),
        profile_build: matches.is_present("profile_build"),
        docker_build: matches.is_present("docker_build"),
    };

    // Download validator version and Build docker image
    let docker_image_config = DockerImageConfig {
        base_image: matches.value_of("base_image").unwrap_or_default(),
        image_name: matches.value_of("image_name").unwrap(),
        tag: matches.value_of("image_tag").unwrap_or_default(),
        registry: matches.value_of("registry_name").unwrap(),
    };

    let deploy = Deploy::new(build_config.clone());
    match deploy.prepare().await {
        Ok(_) => info!("Validator setup prepared successfully"),
        Err(err) => {
            error!("Exiting........ {}", err);
            return;
        }
    }

    let docker = DockerConfig::new(docker_image_config, build_config.deploy_method);
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
