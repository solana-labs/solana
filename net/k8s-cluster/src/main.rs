use {
    clap::{crate_description, crate_name, App, Arg, ArgMatches, value_t_or_exit},
    kube::{
        api::{ListParams, Api, PostParams, ObjectMeta},
        Client, Config,
    },
    k8s_openapi::{
        api::{
            core::v1::{
                Pod,
                Container,
                PodSpec,
                PodTemplateSpec,
            },
            apps::v1::{
                Deployment,
                DeploymentSpec
            }
        },
        apimachinery::pkg::apis::meta::v1::LabelSelector,
    },
    log::*,
    serde_json,
    std::collections::BTreeMap, // Import BTreeMap
    // anyhow::Result,
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
        ).arg(
            Arg::with_name("container_name")
                .long("container")
                .takes_value(true)
                .required(true)
                .help("Validator Container name"),
        ).arg(
            Arg::with_name("image_name")
                .long("image")
                .takes_value(true)
                .required(true)
                .help("Docker Image of Validator to deploy"),
        )


        .get_matches()

}

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "INFO");
    }
    solana_logger::setup();
    let matches = parse_matches();
    let namespace = matches.value_of("cluster_namespace").unwrap_or_default();
    let app_name = matches.value_of("app_name").expect("Application name is required");
    let replicas = value_t_or_exit!(matches, "number_of_replicas", i32);
    let container_name = matches.value_of("container_name").expect("Container name is required");
    let image_name = matches.value_of("image_name").expect("Image name is required");

    info!("namespace: {}", namespace);


    let deployment = define_deployment(app_name, namespace, container_name, image_name, replicas);
    // let pod_spec = define_pod_spec(container_name, image_name, Some(label_selector));
    // info!("dep spec: {:?}", deployment);
    // Convert the JSON value to a formatted string
    // let deployment_string = format!("{:?}", deployment);
    // let formatted_json = serde_json::to_string_pretty(&deployment_string)
    //     .expect("Failed to convert JSON to formatted string");

    // // Use the info! macro to log the formatted JSON
    // info!("Formatted JSON:\n{}", formatted_json);

    let _ = run_deployer(namespace, &deployment);
    let _ = run_controller(namespace);

    let _ = get_deployment_info(app_name, namespace);



}

#[tokio::main]
async fn get_deployment_info(
    app_name: &str,
    namespace: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let api: Api<Deployment> = Api::namespaced(client, namespace);

    let deployment = api.get(format!("{}-deployment", app_name).as_str()).await?;
    let deployment_json = serde_json::to_string_pretty(&deployment)?;
    println!("{}", deployment_json);
    // let deployment_json = serde_json::to_value(&deployment)?;

    Ok(())
}

#[tokio::main]
async fn run_controller(
    namespace: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    // let pods: Api<Pod> = Api::default_namespaced(client);
    let pods: Api<Pod> = Api::namespaced(client, namespace);
    let pod_list = pods.list(&ListParams::default()).await?;
    let names = pod_list.into_iter()
        .map(|pod| pod.metadata.name.unwrap_or("".into()))
        .collect::<Vec<String>>();
    info!("Pods in ns {}, {names:?}", namespace);
    Ok(())
}

#[tokio::main]
async fn run_deployer(
    namespace: &str,
    deployment: &Deployment,
) -> Result<(), Box<dyn std::error::Error>> {

    let client = Client::try_default().await?;


    let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let post_params = PostParams::default();
    // Apply the Deployment
    api.create(&post_params, &deployment).await?;

    println!("Deployment created successfully in the specified namespace!");

    Ok(())
}


fn define_deployment(
    app_name: &str,
    namespace: &str,
    container_name: &str,
    image_name: &str,
    replicas: i32,
) -> Deployment {
    let mut label_selector = BTreeMap::new();  // Create a JSON map for label selector
    label_selector.insert("app".to_string(), app_name.to_string());
    // Define the Deployment

    let pod_spec = PodTemplateSpec {
        metadata: Some(ObjectMeta {
            labels: Some(label_selector.clone()),
            ..Default::default()
        }),
        spec: Some(PodSpec {
            containers: vec![Container {
                name: container_name.to_string(),
                image: Some(image_name.to_string()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    let deployment_spec = DeploymentSpec {
        replicas: Some(replicas),
        selector: LabelSelector {
            match_labels: Some(label_selector),
            ..Default::default()
        },
        template: pod_spec,
        ..Default::default()
    };

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