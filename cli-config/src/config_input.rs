use {
    crate::Config, solana_clap_utils::input_validators::normalize_to_url_if_moniker,
    solana_sdk::commitment_config::CommitmentConfig, std::str::FromStr,
};

pub enum SettingType {
    Explicit,
    Computed,
    SystemDefault,
}

pub struct ConfigInput {
    pub json_rpc_url: String,
    pub websocket_url: String,
    pub keypair_path: String,
    pub commitment: CommitmentConfig,
}

impl ConfigInput {
    fn default_keypair_path() -> String {
        Config::default().keypair_path
    }

    fn default_json_rpc_url() -> String {
        Config::default().json_rpc_url
    }

    fn default_websocket_url() -> String {
        Config::default().websocket_url
    }

    fn default_commitment() -> CommitmentConfig {
        CommitmentConfig::confirmed()
    }

    fn first_nonempty_setting(
        settings: std::vec::Vec<(SettingType, String)>,
    ) -> (SettingType, String) {
        settings
            .into_iter()
            .find(|(_, value)| !value.is_empty())
            .expect("no nonempty setting")
    }

    fn first_setting_is_some<T>(
        settings: std::vec::Vec<(SettingType, Option<T>)>,
    ) -> (SettingType, T) {
        let (setting_type, setting_option) = settings
            .into_iter()
            .find(|(_, value)| value.is_some())
            .expect("all settings none");
        (setting_type, setting_option.unwrap())
    }

    pub fn compute_websocket_url_setting(
        websocket_cmd_url: &str,
        websocket_cfg_url: &str,
        json_rpc_cmd_url: &str,
        json_rpc_cfg_url: &str,
    ) -> (SettingType, String) {
        Self::first_nonempty_setting(vec![
            (SettingType::Explicit, websocket_cmd_url.to_string()),
            (SettingType::Explicit, websocket_cfg_url.to_string()),
            (
                SettingType::Computed,
                Config::compute_websocket_url(&normalize_to_url_if_moniker(json_rpc_cmd_url)),
            ),
            (
                SettingType::Computed,
                Config::compute_websocket_url(&normalize_to_url_if_moniker(json_rpc_cfg_url)),
            ),
            (SettingType::SystemDefault, Self::default_websocket_url()),
        ])
    }

    pub fn compute_json_rpc_url_setting(
        json_rpc_cmd_url: &str,
        json_rpc_cfg_url: &str,
    ) -> (SettingType, String) {
        let (setting_type, url_or_moniker) = Self::first_nonempty_setting(vec![
            (SettingType::Explicit, json_rpc_cmd_url.to_string()),
            (SettingType::Explicit, json_rpc_cfg_url.to_string()),
            (SettingType::SystemDefault, Self::default_json_rpc_url()),
        ]);
        (setting_type, normalize_to_url_if_moniker(url_or_moniker))
    }

    pub fn compute_keypair_path_setting(
        keypair_cmd_path: &str,
        keypair_cfg_path: &str,
    ) -> (SettingType, String) {
        Self::first_nonempty_setting(vec![
            (SettingType::Explicit, keypair_cmd_path.to_string()),
            (SettingType::Explicit, keypair_cfg_path.to_string()),
            (SettingType::SystemDefault, Self::default_keypair_path()),
        ])
    }

    pub fn compute_commitment_config(
        commitment_cmd: &str,
        commitment_cfg: &str,
    ) -> (SettingType, CommitmentConfig) {
        Self::first_setting_is_some(vec![
            (
                SettingType::Explicit,
                CommitmentConfig::from_str(commitment_cmd).ok(),
            ),
            (
                SettingType::Explicit,
                CommitmentConfig::from_str(commitment_cfg).ok(),
            ),
            (SettingType::SystemDefault, Some(Self::default_commitment())),
        ])
    }
}

impl Default for ConfigInput {
    fn default() -> ConfigInput {
        ConfigInput {
            json_rpc_url: Self::default_json_rpc_url(),
            websocket_url: Self::default_websocket_url(),
            keypair_path: Self::default_keypair_path(),
            commitment: CommitmentConfig::confirmed(),
        }
    }
}
