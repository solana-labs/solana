use {
    clap::{crate_description, crate_name, value_t_or_exit, App, Arg, ArgMatches},
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
    serde_json,
    solana_k8s_cluster::{
        config::SetupConfig,
        setup::{BuildConfig, Deploy},
    },
    std::{collections::BTreeMap, process, thread, time::Duration},
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
                .help("Number of validator replicas to deploy"),
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
        .get_matches()
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "INFO");
    }
    solana_logger::setup();
    let matches = parse_matches();

    let setup_config = SetupConfig {
        namespace: matches.value_of("cluster_namespace").unwrap_or_default(),
        num_validators: value_t_or_exit!(matches, "number_of_validators", i32),
    };
    let build_config = BuildConfig {
        release_channel: matches.value_of("release_channel").unwrap_or_default(),
        deploy_method: matches.value_of("deploy_method").unwrap(),
        do_build: matches.is_present("do_build"),
        debug_build: matches.is_present("debug_build"),
        profile_build: matches.is_present("profile_build"),
    };
    let deploy = Deploy::new(build_config);
    deploy.prepare().await;

    process::exit(1);

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

    info!("namespace: {}", setup_config.namespace);

    let client = Client::try_default().await.unwrap();

    //Bootstrap validator deployment and service creation/deployment
    let mut bootstrap_validator_selector = BTreeMap::new(); // Create a JSON map for label selector
    bootstrap_validator_selector.insert(
        "app.kubernetes.io/name".to_string(),
        "bootstrap-validator".to_string(),
    );

    let bootstrap_validator_deployment = create_bootstrap_validator_deployment(
        setup_config.namespace,
        bootstrap_container_name,
        bootstrap_image_name,
        BOOTSTRAP_VALIDATOR_REPLICAS,
        &bootstrap_validator_selector,
    );
    let dep_name = match deploy_deployment(
        client.clone(),
        setup_config.namespace,
        &bootstrap_validator_deployment,
    )
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

    let bootstrap_validator_service =
        create_bootstrap_validator_service(setup_config.namespace, &bootstrap_validator_selector);
    match deploy_service(
        client.clone(),
        setup_config.namespace,
        &bootstrap_validator_service,
    )
    .await
    {
        Ok(_) => info!("bootstrap validator service deployed successfully"),
        Err(err) => error!(
            "Error! Failed to deploy bootstrap validator service. err: {:?}",
            err
        ),
    }

    //TODO: handle this return val properly, don't just unwrap
    while !check_deployment_ready(client.clone(), setup_config.namespace, dep_name.as_str())
        .await
        .unwrap()
    {
        info!("deployment: {} not ready...", dep_name);
        thread::sleep(Duration::from_secs(1));
    }
    info!("deployment: {} Ready!", dep_name);

    //Validator deployment and service creation/deployment
    let mut validator_selector = BTreeMap::new(); // Create a JSON map for label selector
    validator_selector.insert(
        "app.kubernetes.io/name".to_string(),
        "validator".to_string(),
    );

    let validator_deployment = create_validator_deployment(
        setup_config.namespace,
        validator_container_name,
        validator_image_name,
        setup_config.num_validators,
        &validator_selector,
    );
    match deploy_deployment(
        client.clone(),
        setup_config.namespace,
        &validator_deployment,
    )
    .await
    {
        Ok(_) => info!("validator deployment deployed successfully"),
        Err(err) => error!(
            "Error! Failed to deploy validator deployment. err: {:?}",
            err
        ),
    }

    let validator_service = create_validator_service(setup_config.namespace, &validator_selector);
    match deploy_service(client.clone(), setup_config.namespace, &validator_service).await {
        Ok(_) => info!("validator service deployed successfully"),
        Err(err) => error!("Error! Failed to deploy validator service. err: {:?}", err),
    }

    let _ = check_service_matching_deployment(
        client.clone(),
        "bootstrap-validator",
        setup_config.namespace,
    )
    .await;
    let _ = check_service_matching_deployment(client, "validator", setup_config.namespace).await;
}

async fn check_deployment_ready(
    client: Client,
    namespace: &str,
    deployment_name: &str,
) -> Result<bool, kube::Error> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment = deployments.get(deployment_name).await?;

    let desired_validators = deployment.spec.as_ref().unwrap().replicas.unwrap_or(1);
    let available_validators = deployment
        .status
        .as_ref()
        .unwrap()
        .available_replicas
        .unwrap_or(0);

    Ok(available_validators >= desired_validators)
}

async fn deploy_deployment(
    client: Client,
    namespace: &str,
    deployment: &Deployment,
) -> Result<Deployment, kube::Error> {
    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let post_params = PostParams::default();
    info!("creating deployment!");
    // Apply the Deployment
    api.create(&post_params, deployment).await
}

fn create_bootstrap_validator_deployment(
    namespace: &str,
    container_name: &str,
    image_name: &str,
    num_bootstrap_validators: i32,
    label_selector: &BTreeMap<String, String>,
) -> Deployment {
    let env_var = vec![EnvVar {
        name: "MY_POD_IP".to_string(),
        value_from: Some(EnvVarSource {
            field_ref: Some(ObjectFieldSelector {
                field_path: "status.podIP".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }];

    let command = vec!["/workspace/start-bootstrap-validator.sh".to_string()];

    create_deployment(
        "bootstrap-validator",
        namespace,
        container_name,
        image_name,
        num_bootstrap_validators,
        label_selector,
        env_var,
        &command,
    )
}

fn create_validator_deployment(
    namespace: &str,
    container_name: &str,
    image_name: &str,
    num_validators: i32,
    label_selector: &BTreeMap<String, String>,
) -> Deployment {
    let env_vars = vec![
        EnvVar {
            name: "NAMESPACE".to_string(),
            value_from: Some(EnvVarSource {
                field_ref: Some(ObjectFieldSelector {
                    field_path: "metadata.namespace".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "BOOTSTRAP_RPC_PORT".to_string(),
            value: Some(format!(
                "bootstrap-validator-service.$(NAMESPACE).svc.cluster.local:8899"
            )),
            ..Default::default()
        },
        EnvVar {
            name: "BOOTSTRAP_GOSSIP_PORT".to_string(),
            value: Some(format!(
                "bootstrap-validator-service.$(NAMESPACE).svc.cluster.local:8001"
            )),
            ..Default::default()
        },
        EnvVar {
            name: "BOOTSTRAP_FAUCET_PORT".to_string(),
            value: Some(format!(
                "bootstrap-validator-service.$(NAMESPACE).svc.cluster.local:9900"
            )),
            ..Default::default()
        },
    ];

    let command = vec!["/workspace/start-validator.sh".to_string()];

    create_deployment(
        "validator",
        namespace,
        container_name,
        image_name,
        num_validators,
        label_selector,
        env_vars,
        &command,
    )
}

fn create_deployment(
    app_name: &str,
    namespace: &str,
    container_name: &str,
    image_name: &str,
    num_validators: i32,
    label_selector: &BTreeMap<String, String>,
    env_vars: Vec<EnvVar>,
    command: &Vec<String>,
) -> Deployment {
    // Define the pod spec
    let pod_spec = PodTemplateSpec {
        metadata: Some(ObjectMeta {
            labels: Some(label_selector.clone()),
            ..Default::default()
        }),
        spec: Some(PodSpec {
            containers: vec![Container {
                name: container_name.to_string(),
                image: Some(image_name.to_string()),
                env: Some(env_vars),
                command: Some(command.clone()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    //Define the deployment spec
    let deployment_spec = DeploymentSpec {
        replicas: Some(num_validators),
        selector: LabelSelector {
            match_labels: Some(label_selector.clone()),
            ..Default::default()
        },
        template: pod_spec,
        ..Default::default()
    };

    //Build deployment
    Deployment {
        metadata: ObjectMeta {
            name: Some(format!("{}-deployment", app_name)),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(deployment_spec),
        ..Default::default()
    }
}

async fn deploy_service(
    client: Client,
    namespace: &str,
    service: &Service,
) -> Result<Service, kube::Error> {
    let post_params = PostParams::default();
    // Create an API instance for Services in the specified namespace
    let service_api: Api<Service> = Api::namespaced(client, namespace);

    // Create the Service object in the cluster
    service_api.create(&post_params, &service).await
}

fn create_validator_service(namespace: &str, label_selector: &BTreeMap<String, String>) -> Service {
    create_service("validator", namespace, label_selector)
}

fn create_bootstrap_validator_service(
    namespace: &str,
    label_selector: &BTreeMap<String, String>,
) -> Service {
    create_service("bootstrap-validator", namespace, label_selector)
}

fn create_service(
    service_name: &str,
    namespace: &str,
    label_selector: &BTreeMap<String, String>,
) -> Service {
    Service {
        metadata: ObjectMeta {
            name: Some(format!("{}-service", service_name).to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(label_selector.clone()),
            cluster_ip: Some("None".into()),
            // cluster_ips: None,
            ports: Some(vec![
                ServicePort {
                    port: 8899, // RPC Port
                    name: Some("rpc-port".to_string()),
                    ..Default::default()
                },
                ServicePort {
                    port: 8001, //Gossip Port
                    name: Some("gossip-port".to_string()),
                    ..Default::default()
                },
                ServicePort {
                    port: 9900, //Faucet Port
                    name: Some("faucet-port".to_string()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn check_service_matching_deployment(
    client: Client,
    app_name: &str,
    namespace: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get the Deployment
    let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment = deployment_api
        .get(format!("{}-deployment", app_name).as_str())
        .await?;
    // let deployment_json = serde_json::to_string_pretty(&deployment).unwrap();
    // info!("{}", deployment_json);

    // Get the Service
    let service_api: Api<Service> = Api::namespaced(client, namespace);
    let service = service_api
        .get(format!("{}-service", app_name).as_str())
        .await?;
    // let service_json = serde_json::to_string_pretty(&service).unwrap();
    // info!("{}", service_json);

    let deployment_labels = deployment
        .spec
        .and_then(|spec| {
            Some(spec.selector).and_then(|selector| {
                selector
                    .match_labels
                    .and_then(|val| val.get("app.kubernetes.io/name").cloned())
            })
        })
        .clone();

    let service_labels = service
        .spec
        .and_then(|spec| {
            spec.selector
                .and_then(|val| val.get("app.kubernetes.io/name").cloned())
        })
        .clone();

    info!(
        "dep, serve labels: {:?}, {:?}",
        deployment_labels, service_labels
    );

    let are_equal = match (deployment_labels, service_labels) {
        (Some(dep_label), Some(serv_label)) => dep_label == serv_label,
        _ => false,
    };

    if !are_equal {
        error!("Deployment and Service labels are not the same!");
    }

    Ok(())
}
