use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentConfig {
    pub confirmations: Option<Value>,
    pub percentage: Option<f64>,
}

impl CommitmentConfig {
    pub fn recent() -> Self {
        Self {
            confirmations: Some(1.into()),
            percentage: None,
        }
    }

    pub fn ok(&self) -> Option<Self> {
        if self == &Self::default() {
            None
        } else {
            Some(self.clone())
        }
    }
}
