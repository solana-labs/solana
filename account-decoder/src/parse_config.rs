use {
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError},
        validator_info,
    },
    bincode::deserialize,
    serde_json::Value,
    solana_config_program::{get_config_data, ConfigKeys},
    solana_sdk::{
        pubkey::Pubkey,
        stake::config::{
            Config as StakeConfig, {self as stake_config},
        },
    },
};

pub fn parse_config(data: &[u8], pubkey: &Pubkey) -> Result<ConfigAccountType, ParseAccountError> {
    let parsed_account = if pubkey == &stake_config::id() {
        get_config_data(data)
            .ok()
            .and_then(|data| deserialize::<StakeConfig>(data).ok())
            .map(|config| ConfigAccountType::StakeConfig(config.into()))
    } else {
        deserialize::<ConfigKeys>(data).ok().and_then(|key_list| {
            if !key_list.keys.is_empty() && key_list.keys[0].0 == validator_info::id() {
                parse_config_data::<String>(data, key_list.keys).and_then(|validator_info| {
                    Some(ConfigAccountType::ValidatorInfo(UiConfig {
                        keys: validator_info.keys,
                        config_data: serde_json::from_str(&validator_info.config_data).ok()?,
                    }))
                })
            } else {
                None
            }
        })
    };
    parsed_account.ok_or(ParseAccountError::AccountNotParsable(
        ParsableAccount::Config,
    ))
}

fn parse_config_data<T>(data: &[u8], keys: Vec<(Pubkey, bool)>) -> Option<UiConfig<T>>
where
    T: serde::de::DeserializeOwned,
{
    let config_data: T = deserialize(get_config_data(data).ok()?).ok()?;
    let keys = keys
        .iter()
        .map(|key| UiConfigKey {
            pubkey: key.0.to_string(),
            signer: key.1,
        })
        .collect();
    Some(UiConfig { keys, config_data })
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum ConfigAccountType {
    StakeConfig(UiStakeConfig),
    ValidatorInfo(UiConfig<Value>),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfigKey {
    pub pubkey: String,
    pub signer: bool,
}

#[deprecated(
    since = "1.16.7",
    note = "Please use `solana_sdk::stake::state::warmup_cooldown_rate()` instead"
)]
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStakeConfig {
    pub warmup_cooldown_rate: f64,
    pub slash_penalty: u8,
}

impl From<StakeConfig> for UiStakeConfig {
    fn from(config: StakeConfig) -> Self {
        Self {
            warmup_cooldown_rate: config.warmup_cooldown_rate,
            slash_penalty: config.slash_penalty,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig<T> {
    pub keys: Vec<UiConfigKey>,
    pub config_data: T,
}

#[cfg(test)]
mod test {
    use {
        super::*, crate::validator_info::ValidatorInfo, serde_json::json,
        solana_config_program::create_config_account, solana_sdk::account::ReadableAccount,
    };

    #[test]
    fn test_parse_config() {
        let stake_config = StakeConfig {
            warmup_cooldown_rate: 0.25,
            slash_penalty: 50,
        };
        let stake_config_account = create_config_account(vec![], &stake_config, 10);
        assert_eq!(
            parse_config(stake_config_account.data(), &stake_config::id()).unwrap(),
            ConfigAccountType::StakeConfig(UiStakeConfig {
                warmup_cooldown_rate: 0.25,
                slash_penalty: 50,
            }),
        );

        let validator_info = ValidatorInfo {
            info: serde_json::to_string(&json!({
                "name": "Solana",
            }))
            .unwrap(),
        };
        let info_pubkey = solana_sdk::pubkey::new_rand();
        let validator_info_config_account = create_config_account(
            vec![(validator_info::id(), false), (info_pubkey, true)],
            &validator_info,
            10,
        );
        assert_eq!(
            parse_config(validator_info_config_account.data(), &info_pubkey).unwrap(),
            ConfigAccountType::ValidatorInfo(UiConfig {
                keys: vec![
                    UiConfigKey {
                        pubkey: validator_info::id().to_string(),
                        signer: false,
                    },
                    UiConfigKey {
                        pubkey: info_pubkey.to_string(),
                        signer: true,
                    }
                ],
                config_data: serde_json::from_str(r#"{"name":"Solana"}"#).unwrap(),
            }),
        );

        let bad_data = vec![0; 4];
        assert!(parse_config(&bad_data, &info_pubkey).is_err());
    }
}
