use bincode::Config;

use {
    base64::{Engine as _, engine::{self, general_purpose}},
    k8s_openapi::{
        api::{
            apps::v1::{Deployment, DeploymentSpec},
            core::v1::{
                Container, EnvVar, EnvVarSource, ObjectFieldSelector, PodSpec, PodTemplateSpec,
                Service, ServicePort, ServiceSpec, ConfigMap, Namespace,
            },
        },
        apimachinery::pkg::apis::meta::v1::LabelSelector,
    },
    kube::{
        api::{Api, ObjectMeta, PostParams},
        Client,
    },
    solana_sdk::{
        genesis_config::DEFAULT_GENESIS_FILE,
    },
    std::{
        error::Error,
        fs::File,
        io::Read,
        path::PathBuf, 
        collections::BTreeMap,
    },
};


pub struct Kubernetes {
    pub i: usize, 
    pub client: Client,
}

impl Kubernetes {
    pub async fn new() -> Self {
        Kubernetes {
            i: 0,
            client: Client::try_default().await.unwrap()
        }
    }

    pub async fn create_config_map(
        &self,
        namespace: &str,
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

        let api: Api<ConfigMap> = Api::namespaced(self.client.clone(), namespace);
        api.create(&PostParams::default(), &config_map).await
    }

    pub async fn namespace_exists(
        &self,
        namespace_to_find: &str
    ) -> Result<bool, kube::Error> {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        let namespace_list = namespaces.list(&Default::default()).await?;
    
        for namespace in namespace_list.items {
            match namespace.metadata.name {
                Some(ns) => {
                    if ns == namespace_to_find.to_string() {
                        return Ok(true)
                    }
                }
                None => ()
            }
        }
        Ok(false)
    }

}