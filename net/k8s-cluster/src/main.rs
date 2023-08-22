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
    std::collections::BTreeMap,
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
            Arg::with_name("app_name")
                .long("app-name")
                .takes_value(true)
                .required(true)
                .help("Name of the application"),
        )
        .arg(
            Arg::with_name("number_of_replicas")
                .long("replicas")
                .takes_value(true)
                .default_value("1")
                .help("Number of validator replicas to deploy"),
        )
        .arg(
            Arg::with_name("container_name")
                .long("container")
                .takes_value(true)
                .required(true)
                .help("Validator Container name"),
        )
        .arg(
            Arg::with_name("image_name")
                .long("image")
                .takes_value(true)
                .required(true)
                .help("Docker Image of Validator to deploy"),
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
    let namespace = matches.value_of("cluster_namespace").unwrap_or_default();
    let app_name = matches
        .value_of("app_name")
        .expect("Application name is required");
    let replicas = value_t_or_exit!(matches, "number_of_replicas", i32);
    let container_name = matches
        .value_of("container_name")
        .expect("Container name is required");
    let image_name = matches
        .value_of("image_name")
        .expect("Image name is required");

    info!("namespace: {}", namespace);

    let client = Client::try_default().await.unwrap();

    let mut label_selector = BTreeMap::new(); // Create a JSON map for label selector
    label_selector.insert("app.kubernetes.io/name".to_string(), app_name.to_string());

    let _ = create_deployment(
        client.clone(),
        app_name,
        namespace,
        container_name,
        image_name,
        replicas,
        &label_selector,
    )
    .await;

    info!("Deployment created successfully in the specified namespace!");

    let _ = create_service(client.clone(), app_name, namespace, &label_selector).await;
    info!("Service created successfully in the specified namespace!");

    let _ = check_service_matching_deployment(client, app_name, namespace).await;
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

async fn create_deployment(
    client: Client,
    app_name: &str,
    namespace: &str,
    container_name: &str,
    image_name: &str,
    replicas: i32,
    label_selector: &BTreeMap<String, String>,
) -> Result<Deployment, kube::Error> {
    let env_var = EnvVar {
        name: "MY_POD_IP".to_string(),
        value_from: Some(EnvVarSource {
            field_ref: Some(ObjectFieldSelector {
                field_path: "status.podIP".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

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
                env: Some(vec![env_var]),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    //Define the deployment spec
    let deployment_spec = DeploymentSpec {
        replicas: Some(replicas),
        selector: LabelSelector {
            match_labels: Some(label_selector.clone()),
            ..Default::default()
        },
        template: pod_spec,
        ..Default::default()
    };

    //Build deployment
    let deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(format!("{}-deployment", app_name)),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(deployment_spec),
        ..Default::default()
    };

    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let post_params = PostParams::default();
    info!("creating deployment!");
    // Apply the Deployment
    api.create(&post_params, &deployment).await
}

async fn create_service(
    client: Client,
    app_name: &str,
    namespace: &str,
    label_selector: &BTreeMap<String, String>,
) -> Result<Service, kube::Error> {
    let service = Service {
        metadata: ObjectMeta {
            name: Some(format!("{}-service", app_name).to_string()),
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
    };
    let post_params = PostParams::default();
    // Create an API instance for Services in the specified namespace
    let service_api: Api<Service> = Api::namespaced(client, namespace);

    // Create the Service object in the cluster
    service_api.create(&post_params, &service).await
}
