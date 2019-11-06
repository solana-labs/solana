#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentConfig {
    pub commitment: CommitmentLevel,
}

impl Default for CommitmentConfig {
    fn default() -> Self {
        CommitmentConfig {
            commitment: CommitmentLevel::Max,
        }
    }
}

impl CommitmentConfig {
    pub fn recent() -> Self {
        Self {
            commitment: CommitmentLevel::Recent,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CommitmentLevel {
    Max,
    Recent,
}
