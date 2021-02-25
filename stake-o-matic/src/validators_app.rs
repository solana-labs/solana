use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug)]
pub enum ClusterJson {
    MainnetBeta,
    Testnet,
}

impl Default for ClusterJson {
    fn default() -> Self {
        Self::MainnetBeta
    }
}

impl AsRef<str> for ClusterJson {
    fn as_ref(&self) -> &str {
        match self {
            Self::MainnetBeta => "mainnet.json",
            Self::Testnet => "testnet.json",
        }
    }
}

const DEFAULT_BASE_URL: &str = "https://www.validators.app/api/v1/";
const TOKEN_HTTP_HEADER_NAME: &str = "Token";

#[derive(Debug)]
pub struct ClientConfig {
    pub base_url: String,
    pub cluster: ClusterJson,
    pub api_token: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            cluster: ClusterJson::default(),
            api_token: String::default(),
        }
    }
}

#[derive(Debug)]
enum Endpoint {
    Ping,
    Validators,
}

impl Endpoint {
    fn with_cluster(path: &str, cluster: &ClusterJson) -> String {
        format!("{}/{}", path, cluster.as_ref())
    }
    pub fn path(&self, cluster: &ClusterJson) -> String {
        match self {
            Self::Ping => "ping.json".to_string(),
            Self::Validators => Self::with_cluster("validators", cluster),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct PingResponse {
    answer: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ValidatorsResponseEntry {
    pub account: Option<String>,
    pub active_stake: Option<u64>,
    pub commission: Option<u8>,
    pub created_at: Option<String>,
    pub data_center_concentration_score: Option<i64>,
    pub data_center_host: Option<String>,
    pub data_center_key: Option<String>,
    pub delinquent: Option<bool>,
    pub details: Option<String>,
    pub keybase_id: Option<String>,
    pub name: Option<String>,
    pub network: Option<String>,
    pub ping_time: Option<f64>,
    pub published_information_score: Option<i64>,
    pub root_distance_score: Option<i64>,
    pub security_report_score: Option<i64>,
    pub skipped_slot_percent: Option<String>,
    pub skipped_slot_score: Option<i64>,
    pub skipped_slots: Option<u64>,
    pub software_version: Option<String>,
    pub software_version_score: Option<i64>,
    pub stake_concentration_score: Option<i64>,
    pub total_score: Option<i64>,
    pub updated_at: Option<String>,
    pub url: Option<String>,
    pub vote_account: Option<String>,
    pub vote_distance_score: Option<i64>,
    pub www_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ValidatorsResponse(Vec<ValidatorsResponseEntry>);

impl AsRef<Vec<ValidatorsResponseEntry>> for ValidatorsResponse {
    fn as_ref(&self) -> &Vec<ValidatorsResponseEntry> {
        &self.0
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum SortKind {
    Score,
    Name,
    Stake,
}

impl std::fmt::Display for SortKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Score => write!(f, "score"),
            Self::Name => write!(f, "name"),
            Self::Stake => write!(f, "stake"),
        }
    }
}

pub type Limit = u32;

pub struct Client {
    base_url: reqwest::Url,
    cluster: ClusterJson,
    api_token: String,
    client: reqwest::blocking::Client,
}

impl Client {
    pub fn new<T: AsRef<str>>(api_token: T) -> Self {
        let config = ClientConfig {
            api_token: api_token.as_ref().to_string(),
            ..ClientConfig::default()
        };
        Self::new_with_config(config)
    }

    pub fn new_with_config(config: ClientConfig) -> Self {
        let ClientConfig {
            base_url,
            cluster,
            api_token,
        } = config;
        Self {
            base_url: reqwest::Url::parse(&base_url).unwrap(),
            cluster,
            api_token,
            client: reqwest::blocking::Client::new(),
        }
    }

    fn request(
        &self,
        endpoint: Endpoint,
        query: &HashMap<String, String>,
    ) -> reqwest::Result<reqwest::blocking::Response> {
        let url = self.base_url.join(&endpoint.path(&self.cluster)).unwrap();
        let request = self
            .client
            .get(url)
            .header(TOKEN_HTTP_HEADER_NAME, &self.api_token)
            .query(&query)
            .build()?;
        self.client.execute(request)
    }

    #[allow(dead_code)]
    pub fn ping(&self) -> reqwest::Result<()> {
        let response = self.request(Endpoint::Ping, &HashMap::new())?;
        response.json::<PingResponse>().map(|_| ())
    }

    pub fn validators(
        &self,
        sort: Option<SortKind>,
        limit: Option<Limit>,
    ) -> reqwest::Result<ValidatorsResponse> {
        let mut query = HashMap::new();
        if let Some(sort) = sort {
            query.insert("sort".into(), sort.to_string());
        }
        if let Some(limit) = limit {
            query.insert("limit".into(), limit.to_string());
        }
        let response = self.request(Endpoint::Validators, &query)?;
        response.json::<ValidatorsResponse>()
    }
}
