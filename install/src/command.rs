use crate::config::Config;
use solana_sdk::pubkey::Pubkey;
use std::time::Duration;

pub fn init(
    config_file: &str,
    data_dir: &str,
    json_rpc_url: &str,
    update_pubkey: &Pubkey,
) -> Result<(), String> {
    println!("init {:?} {:?}", json_rpc_url, update_pubkey);
    Config {
        data_dir: data_dir.to_string(),
        json_rpc_url: json_rpc_url.to_string(),
        update_pubkey: *update_pubkey,
        current_install: None,
    }
    .save(config_file)?;

    Err("Not implemented".to_string())
}

pub fn info(config_file: &str, local_info_only: bool) -> Result<(), String> {
    let config = Config::load(config_file)?;
    println!("config dir: {:?}", config_file);
    println!("configuration: {:?}", config);
    if local_info_only {
        return Ok(());
    }

    // TODO: fetch info about current update manifest from the cluster

    Err("Not implemented".to_string())
}

pub fn deploy(
    config_file: &str,
    download_url: &str,
    update_manifest_keypair: &str,
) -> Result<(), String> {
    println!("deploy {:?} {:?}", download_url, update_manifest_keypair);
    let _config = Config::load(config_file)?;
    Err("Not implemented".to_string())
}

pub fn update(config_file: &str) -> Result<(), String> {
    println!(
        "update: BUILD_SECONDS_SINCE_UNIX_EPOCH={:?}",
        Duration::from_secs(
            u64::from_str_radix(crate::build_env::BUILD_SECONDS_SINCE_UNIX_EPOCH, 10).unwrap()
        )
    );
    let _config = Config::load(config_file)?;
    Err("Not implemented".to_string())
}

pub fn run(
    config_file: &str,
    program_name: &str,
    program_arguments: Vec<&str>,
) -> Result<(), String> {
    println!("run {:?} {:?}", program_name, program_arguments);
    let _config = Config::load(config_file)?;
    Err("Not implemented".to_string())
}
