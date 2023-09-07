use k8s_openapi::api::core::v1::SecretVolumeSource;

use {
    crate::{boxed_error, ValidatorType, SOLANA_ROOT},
    base64::{
        engine::general_purpose,
        Engine as _,
    },
    k8s_openapi::{
        api::{
            apps::v1::{ReplicaSet, ReplicaSetSpec},
            core::v1::{
                ConfigMap, ConfigMapVolumeSource, Container, EnvVar, EnvVarSource,
                LocalObjectReference, Namespace, ObjectFieldSelector, PodSpec, PodTemplateSpec,
                Secret, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
            },
        },
        apimachinery::pkg::apis::meta::v1::LabelSelector,
        ByteString,
    },
    kube::{
        api::{Api, ObjectMeta, PostParams},
        Client,
    },
    log::*,
    std::{collections::BTreeMap, error::Error, path::PathBuf},
};

pub struct Kubernetes<'a> {
    client: Client,
    namespace: &'a str,
}

impl<'a> Kubernetes<'a> {
    pub async fn new(namespace: &'a str) -> Kubernetes<'a> {

        Kubernetes {
            client: Client::try_default().await.unwrap(),
            namespace: namespace,
        }
    }

    pub async fn create_config_map(
        &self,
        base64_content: String,
    ) -> Result<ConfigMap, kube::Error> {
        let mut metadata = ObjectMeta::default();
        metadata.name = Some("genesis-config".to_string());
        // Define the data for the ConfigMap
        let mut data = BTreeMap::<String, String>::new();
        data.insert("genesis.bin".to_string(), base64_content);
        // Create the ConfigMap object
        let config_map = ConfigMap {
            metadata,
            data: Some(data),
            ..Default::default()
        };

        let api: Api<ConfigMap> = Api::namespaced(self.client.clone(), self.namespace);
        api.create(&PostParams::default(), &config_map).await
    }

    pub async fn namespace_exists(&self) -> Result<bool, kube::Error> {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        let namespace_list = namespaces.list(&Default::default()).await?;

        for namespace in namespace_list.items {
            match namespace.metadata.name {
                Some(ns) => {
                    if ns == self.namespace.to_string() {
                        return Ok(true);
                    }
                }
                None => (),
            }
        }
        Ok(false)
    }

    pub fn create_selector(
        &mut self,
        key: &str,
        value: &str,
    ) -> BTreeMap<String, String> {
        let mut btree = BTreeMap::new();
        btree.insert(key.to_string(), value.to_string());
        btree
    }

    pub async fn create_bootstrap_validator_replicas_set(
        &self,
        container_name: &str,
        image_name: &str,
        num_bootstrap_validators: i32,
        config_map_name: Option<String>,
        secret_name: Option<String>,
        label_selector: &BTreeMap<String, String>,
    ) -> Result<ReplicaSet, Box<dyn Error>> {
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

        let accounts_volume = Volume {
            name: "bootstrap-accounts-volume".into(),
            secret: Some(SecretVolumeSource {
                secret_name: secret_name,
                ..Default::default()
            }),
            ..Default::default()
        };

        let accounts_volume_mount = VolumeMount {
            name: "bootstrap-accounts-volume".to_string(),
            mount_path: "/home/solana/bootstrap-accounts".to_string(),
            ..Default::default()
        };


        // let command = vec!["/workspace/start-bootstrap-validator.sh".to_string()];
        let command = vec!["sleep".to_string(), "3600".to_string()];
        // let command = vec!["nohup"]

        self.create_replicas_set(
            "bootstrap-validator",
            label_selector,
            container_name,
            image_name,
            num_bootstrap_validators,
            env_var,
            &command,
            config_map_name,
            accounts_volume,
            accounts_volume_mount,
        )
        .await
    }

    async fn create_replicas_set(
        &self,
        app_name: &str,
        label_selector: &BTreeMap<String, String>,
        container_name: &str,
        image_name: &str,
        num_validators: i32,
        env_vars: Vec<EnvVar>,
        command: &Vec<String>,
        config_map_name: Option<String>,
        accounts_volume: Volume,
        accounts_volume_mount: VolumeMount,
    ) -> Result<ReplicaSet, Box<dyn Error>> {
        let config_map_name = match config_map_name {
            Some(name) => name,
            None => return Err(boxed_error!("config_map_name is None!")),
        };

        let genesis_volume = Volume {
            name: "genesis-config-volume".into(),
            config_map: Some(ConfigMapVolumeSource {
                name: Some(config_map_name.clone()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let genesis_volume_mount = VolumeMount {
            name: "genesis-config-volume".to_string(),
            mount_path: "/home/solana/genesis".to_string(),
            ..Default::default()
        };

        // let accounts_volume = Volume {
        //     name: "bootstrap-accounts-volume".into(),
        //     secret: Some(SecretVolumeSource {
        //         secret_name: Some("bootstrap-accounts-secret".to_string()),
        //         ..Default::default()
        //     }),
        //     ..Default::default()
        // };

        // let accounts_volume_mount = VolumeMount {
        //     name: "bootstrap-accounts-volume".to_string(),
        //     mount_path: "/home/solana/bootstrap-accounts".to_string(),
        //     ..Default::default()
        // };

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
                    image_pull_policy: Some("IfNotPresent".to_string()), // Set the image pull policy to "Never"
                    env: Some(env_vars),
                    command: Some(command.clone()),
                    volume_mounts: Some(vec![genesis_volume_mount, accounts_volume_mount]),

                    ..Default::default()
                }],
                volumes: Some(vec![genesis_volume, accounts_volume]),
                // image_pull_secrets: Some(vec![LocalObjectReference {
                //     name: Some("dockerhub-login".to_string()),
                //     ..Default::default()
                // }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let replicas_set_spec = ReplicaSetSpec {
            replicas: Some(num_validators),
            selector: LabelSelector {
                match_labels: Some(label_selector.clone()),
                ..Default::default()
            },
            template: Some(pod_spec),
            ..Default::default()
        };

        Ok(ReplicaSet {
            metadata: ObjectMeta {
                name: Some(format!("{}-replicaset", app_name)),
                namespace: Some(self.namespace.to_string()),
                ..Default::default()
            },
            spec: Some(replicas_set_spec),
            ..Default::default()
        })
    }

    pub async fn deploy_secret(&self, secret: &Secret) -> Result<Secret, kube::Error> {
        let secrets_api: Api<Secret> = Api::namespaced(self.client.clone(), self.namespace);
        secrets_api.create(&PostParams::default(), &secret).await
    }

    pub fn create_bootstrap_secret(
        &self,
        secret_name: &str,
    ) -> Result<Secret, Box<dyn Error>> {
        let key_path = SOLANA_ROOT.join("config-k8s/bootstrap-validator");

        let identity_keypair = std::fs::read(key_path.join("identity.json"))
            .expect(format!("Failed to read identity.json file! at: {:?}", key_path).as_str());
        let vote_keypair = std::fs::read(key_path.join("vote-account.json"))
            .expect(format!("Failed to read vote-account.json file! at: {:?}", key_path).as_str());
        let stake_keypair = std::fs::read(key_path.join("stake-account.json"))
            .expect(format!("Failed to read stake-account.json file! at: {:?}", key_path).as_str());

        let mut data = BTreeMap::new();
        data.insert("identity.json".to_string(), ByteString(general_purpose::STANDARD.encode(identity_keypair).as_bytes().to_vec()));
        data.insert("vote.json".to_string(), ByteString(general_purpose::STANDARD.encode(vote_keypair).as_bytes().to_vec()));
        data.insert("stake.json".to_string(), ByteString(general_purpose::STANDARD.encode(stake_keypair).as_bytes().to_vec()));

        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_name.to_string()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };

        Ok(secret)
    }

    pub fn create_validator_secret(
        &self,
        validator_index: i32,
    ) -> Result<Secret, Box<dyn Error>> {
        let secret_name = format!("validator-accounts-secret-{}", validator_index);
        let key_path = SOLANA_ROOT.join("config-k8s");

        let mut data: BTreeMap<String, ByteString> = BTreeMap::new();
        let accounts = vec!["identity", "vote", "stake"];
        for account in accounts {
            let file_name: String;
            if account == "identity" {
                file_name = format!("validator-{}-{}.json", account, validator_index);
            } else {
                file_name = format!("validator-{}-account-{}.json", account, validator_index);
            }
            let keypair = std::fs::read(key_path.join(file_name.clone()))
                .expect(format!("Failed to read {} file! at: {:?}", file_name, key_path).as_str());
            data.insert(format!("{}.json", account), ByteString(general_purpose::STANDARD.encode(keypair).as_bytes().to_vec()));
        }
        let secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_name.to_string()),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        };

        Ok(secret)

    }

    pub async fn deploy_replicas_set(
        &self,
        replica_set: &ReplicaSet,
    ) -> Result<ReplicaSet, kube::Error> {
        let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), self.namespace);
        let post_params = PostParams::default();
        info!("creating replica set!");
        // Apply the ReplicaSet
        api.create(&post_params, replica_set).await
    }

    fn create_service(
        &self,
        service_name: &str,
        label_selector: &BTreeMap<String, String>,
    ) -> Service {
        Service {
            metadata: ObjectMeta {
                name: Some(format!("{}-service", service_name).to_string()),
                namespace: Some(self.namespace.to_string()),
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

    pub async fn deploy_service(&self, service: &Service) -> Result<Service, kube::Error> {
        let post_params = PostParams::default();
        // Create an API instance for Services in the specified namespace
        let service_api: Api<Service> = Api::namespaced(self.client.clone(), self.namespace);

        // Create the Service object in the cluster
        service_api.create(&post_params, &service).await
    }

    pub async fn check_replica_set_ready(
        &self,
        replica_set_name: &str,
    ) -> Result<bool, kube::Error> {
        let replica_sets: Api<ReplicaSet> = Api::namespaced(self.client.clone(), self.namespace);
        let replica_set = replica_sets.get(replica_set_name).await?;

        let desired_validators = replica_set.spec.as_ref().unwrap().replicas.unwrap_or(1);
        let available_validators = replica_set
            .status
            .as_ref()
            .unwrap()
            .available_replicas
            .unwrap_or(0);

        Ok(available_validators >= desired_validators)
    }

    pub async fn create_validator_replicas_set(
        &self,
        container_name: &str,
        validator_index: i32,
        image_name: &str,
        num_validators: i32,
        config_map_name: Option<String>,
        secret_name: Option<String>,
        label_selector: &BTreeMap<String, String>,
    ) -> Result<ReplicaSet, Box<dyn Error>> {
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

        let accounts_volume = Volume {
            name: format!("validator-accounts-volume-{}", validator_index),
            secret: Some(SecretVolumeSource {
                secret_name: secret_name,
                ..Default::default()
            }),
            ..Default::default()
        };

        let accounts_volume_mount = VolumeMount {
            name: format!("validator-accounts-volume-{}", validator_index),
            mount_path: "/home/solana/validator-accounts".to_string(),
            ..Default::default()
        };



        // let command = vec!["/workspace/start-validator.sh".to_string()];
        let command = vec!["sleep".to_string(), "3600".to_string()];

        self.create_replicas_set(
            format!("validator-{}", validator_index).as_str(),
            label_selector,
            container_name,
            image_name,
            num_validators,
            env_vars,
            &command,
            config_map_name,
            accounts_volume,
            accounts_volume_mount,
        )
        .await
    }

    pub fn create_validator_service(
        &self, 
        service_name: &str, 
        label_selector: &BTreeMap<String, String>,
    ) -> Service {
        self.create_service(service_name, label_selector)
    }

    pub async fn check_service_matching_replica_set(
        &self,
        app_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get the replica_set
        let replica_set_api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), self.namespace);
        let replica_set = replica_set_api
            .get(format!("{}-replicaset", app_name).as_str())
            .await?;

        // Get the Service
        let service_api: Api<Service> = Api::namespaced(self.client.clone(), self.namespace);
        let service = service_api
            .get(format!("{}-service", app_name).as_str())
            .await?;

        let replica_set_labels = replica_set
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
            "ReplicaSet, Service labels: {:?}, {:?}",
            replica_set_labels, service_labels
        );

        let are_equal = match (replica_set_labels, service_labels) {
            (Some(rs_label), Some(serv_label)) => rs_label == serv_label,
            _ => false,
        };

        if !are_equal {
            error!("ReplicaSet and Service labels are not the same!");
        }

        Ok(())
    }
}
