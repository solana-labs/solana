use {
    k8s_openapi::{
        api::{
            apps::v1::{ReplicaSet, ReplicaSetSpec},
            core::v1::{
                Container, Secret, Volume, VolumeMount, Probe, EnvVar, PodTemplateSpec,
                PodSecurityContext, PodSpec, NodeAffinity, NodeSelector, NodeSelectorTerm,
                NodeSelectorRequirement, Affinity
            }
        },
        apimachinery::pkg::apis::meta::v1::LabelSelector,
        ByteString,
    },
    kube::api::ObjectMeta,
    std::{collections::BTreeMap, error::Error},
};

pub fn create_secret(name: &str, data: BTreeMap<String, ByteString>) -> Secret {
    Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    }
}

pub fn create_selector(key: &str, value: &str) -> BTreeMap<String, String> {
    let mut btree = BTreeMap::new();
    btree.insert(key.to_string(), value.to_string());
    btree
}

#[allow(clippy::too_many_arguments)]
pub fn create_replica_set(
    name: &str,
    namespace: &str,
    label_selector: &BTreeMap<String, String>,
    container_name: &str,
    image_name: &str,
    environment_variables: Vec<EnvVar>,
    command: &[String],
    volumes: Option<Vec<Volume>>,
    volume_mounts: Option<Vec<VolumeMount>>,
    readiness_probe: Option<Probe>,
    nodes: Option<Vec<String>>,
) -> Result<ReplicaSet, Box<dyn Error>> {
    let node_affinity = NodeAffinity {
        required_during_scheduling_ignored_during_execution: Some(NodeSelector {
            node_selector_terms: vec![NodeSelectorTerm {
                match_expressions: Some(vec![
                    NodeSelectorRequirement {
                        key: "kubernetes.io/hostname".to_string(),
                        operator: "In".to_string(),
                        values: nodes,
                    },
                ]),
                ..Default::default()
            }],
        }),
        ..Default::default()
    };

    let pod_spec = PodTemplateSpec {
        metadata: Some(ObjectMeta {
            labels: Some(label_selector.clone()),
            ..Default::default()
        }),
        spec: Some(PodSpec {
            containers: vec![Container {
                name: container_name.to_string(),
                image: Some(image_name.to_string()),
                image_pull_policy: Some("Always".to_string()),
                env: Some(environment_variables),
                command: Some(command.to_owned()),
                volume_mounts,
                readiness_probe: readiness_probe,
                ..Default::default()
            }],
            volumes,
            security_context: Some(PodSecurityContext {
                run_as_user: Some(1000),
                run_as_group: Some(1000),
                ..Default::default()
            }),
            affinity: Some(Affinity {
                node_affinity: Some(node_affinity),
                ..Default::default()
            }),
            ..Default::default()
        }),
    };

    let replicas_set_spec = ReplicaSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: Some(label_selector.clone()),
            ..Default::default()
        },
        template: Some(pod_spec),
        ..Default::default()
    };

    Ok(ReplicaSet {
        metadata: ObjectMeta {
            name: Some(format!("{}-replicaset", name)),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(replicas_set_spec),
        ..Default::default()
    })
}
