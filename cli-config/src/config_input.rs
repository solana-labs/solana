use {
    crate::Config, solana_clap_utils::input_validators::normalize_to_url_if_moniker,
    solana_sdk::commitment_config::CommitmentConfig, std::str::FromStr,
};
use solana_client::rpc_client::RpcClient;

use crate::CONFIG_FILE;

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
    pub fee: u64,
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

    fn default_fee() -> u64 {
        Self::calculate_recent_fee(None)
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

    pub fn compute_fee_config(
        fee_cmd: &str,
        fee_cfg: &str,
    ) -> (SettingType, u64) {
        Self::first_setting_is_some(vec![
            (
                SettingType::Explicit,
                u64::from_str(fee_cmd).ok(),
            ),
            (
                SettingType::Explicit,
                u64::from_str(fee_cfg).ok(),
            ),
            (SettingType::SystemDefault, Some(Self::default_fee())),
        ])
    }
    
    pub fn calculate_recent_fee(
        commitment_config: Option<CommitmentConfig>,
    ) -> u64 {
        let default_config = "~/.config/solana/cli/config.yml".to_string();
        let config_file = CONFIG_FILE.as_ref()
            .unwrap_or(&default_config);
        let config = Config::load(&config_file).unwrap_or(Config::default());
        println!("config.json_rpc_url: {}", config.json_rpc_url);
        if config.json_rpc_url.len() == 0 {
            return 0;
        }
        if config.json_rpc_url.contains("api.solana.com") {
            return 0;
        }
        let rpc_client = RpcClient::new_with_commitment(
            config.json_rpc_url,
            commitment_config.unwrap_or(
            CommitmentConfig::finalized()
            ),
        );
        let recent_fees = rpc_client.get_recent_prioritization_fees(
            &[]
        );
        if recent_fees.is_err() {
            println!("recent_fees: {:?}", recent_fees.err());
            return 0;
        }
        let recent_fees = recent_fees.unwrap();
        println!("recent_fees: {:?}", recent_fees.len());
        let recent_fees = recent_fees
        .iter()
        .map(|fee| fee.prioritization_fee)
        .sum::<u64>()
        .checked_div(recent_fees.len() as u64).unwrap_or(0);
    println!("recent_fees: {}", recent_fees);
    recent_fees
    }


}

impl Default for ConfigInput {
    fn default() -> ConfigInput {
        ConfigInput {
            json_rpc_url: Self::default_json_rpc_url(),
            websocket_url: Self::default_websocket_url(),
            keypair_path: Self::default_keypair_path(),
            commitment: CommitmentConfig::confirmed(),
            fee: Self::default_fee(),
        }
    }
}
