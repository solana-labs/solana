use bincode::deserialize;
use clap::{
    crate_description, crate_name, crate_version, App, AppSettings, Arg, ArgMatches, SubCommand,
};
use reqwest::Client;
use serde_derive::{Deserialize, Serialize};
use serde_json::{Map, Value};
use solana_client::rpc_client::RpcClient;
use solana_config_api::{config_instruction, config_instruction::ConfigKeys, ConfigState};
use solana_sdk::account::Account;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use std::error;
use std::process::exit;

pub const MAX_SHORT_FIELD_LENGTH: usize = 70;
pub const MAX_LONG_FIELD_LENGTH: usize = 300;
pub const MAX_VALIDATOR_INFO: u64 = 576;
pub const JSON_RPC_URL: &str = "http://testnet.solana.com:8899";

// Config account key: Va1idator1nfo111111111111111111111111111111
pub const REGISTER_CONFIG_KEY: [u8; 32] = [
    7, 81, 151, 1, 116, 72, 242, 172, 93, 194, 60, 158, 188, 122, 199, 140, 10, 39, 37, 122, 198,
    20, 69, 141, 224, 164, 241, 111, 128, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    REGISTER_CONFIG_KEY,
    "Va1idator1nfo111111111111111111111111111111"
);

#[derive(Debug, Deserialize, Serialize)]
struct ValidatorInfo {
    info: String,
}

impl ConfigState for ValidatorInfo {
    fn max_space() -> u64 {
        MAX_VALIDATOR_INFO
    }
}

// Return an error if a pubkey cannot be parsed.
fn is_pubkey(string: String) -> Result<(), String> {
    match string.parse::<Pubkey>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

// Return an error if a url cannot be parsed.
fn is_url(string: String) -> Result<(), String> {
    match url::Url::parse(&string) {
        Ok(url) => {
            if url.has_host() {
                Ok(())
            } else {
                Err("no host provided".to_string())
            }
        }
        Err(err) => Err(format!("{:?}", err)),
    }
}

// Return an error if url field is too long or cannot be parsed.
fn check_url(string: String) -> Result<(), String> {
    is_url(string.clone())?;
    if string.len() > MAX_SHORT_FIELD_LENGTH {
        Err(format!(
            "url longer than {:?}-byte limit",
            MAX_SHORT_FIELD_LENGTH
        ))
    } else {
        Ok(())
    }
}

// Return an error if a validator field is longer than the max length.
fn is_short_field(string: String) -> Result<(), String> {
    if string.len() > MAX_SHORT_FIELD_LENGTH {
        Err(format!(
            "validator field longer than {:?}-byte limit",
            MAX_SHORT_FIELD_LENGTH
        ))
    } else {
        Ok(())
    }
}

// Return an error if a validator details are longer than the max length.
fn check_details_length(string: String) -> Result<(), String> {
    if string.len() > MAX_LONG_FIELD_LENGTH {
        Err(format!(
            "validator details longer than {:?}-byte limit",
            MAX_LONG_FIELD_LENGTH
        ))
    } else {
        Ok(())
    }
}

fn verify_keybase(
    validator_pubkey: &Pubkey,
    keybase_username: &Value,
) -> Result<(), Box<dyn error::Error>> {
    if let Some(keybase_username) = keybase_username.as_str() {
        let url = format!(
            "https://keybase.pub/{}/solana/validator-{:?}",
            keybase_username, validator_pubkey
        );
        let client = Client::new();
        if client.head(&url).send()?.status().is_success() {
            Ok(())
        } else {
            Err(format!("keybase_username could not be confirmed at: {}. Please add this pubkey file to your keybase profile to connect", url))?
        }
    } else {
        Err(format!(
            "keybase_username could not be parsed as String: {}",
            keybase_username
        ))?
    }
}

fn parse_args(matches: &ArgMatches<'_>) -> Value {
    let mut map = Map::new();
    map.insert(
        "name".to_string(),
        Value::String(matches.value_of("name").unwrap().to_string()),
    );
    if let Some(url) = matches.value_of("website") {
        map.insert("website".to_string(), Value::String(url.to_string()));
    }
    if let Some(details) = matches.value_of("details") {
        map.insert("details".to_string(), Value::String(details.to_string()));
    }
    if let Some(keybase_username) = matches.value_of("keybase_username") {
        map.insert(
            "keybaseUsername".to_string(),
            Value::String(keybase_username.to_string()),
        );
    }
    Value::Object(map)
}

fn parse_validator_info(
    pubkey: &Pubkey,
    account_data: &[u8],
) -> Result<(Pubkey, String), Box<dyn error::Error>> {
    let key_list: ConfigKeys = deserialize(&account_data)?;
    if !key_list.keys.is_empty() {
        let (validator_pubkey, _) = key_list.keys[1];
        let meta_length = ConfigKeys::serialized_size(key_list.keys);
        let validator_info: String = deserialize(&account_data[meta_length..])?;
        Ok((validator_pubkey, validator_info))
    } else {
        Err(format!(
            "account {} found, but could not be parsed as ValidatorInfo",
            pubkey
        ))?
    }
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("publish")
                .about("Publish Validator info on Solana")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("json_rpc_url")
                        .short("u")
                        .long("url")
                        .value_name("URL")
                        .takes_value(true)
                        .default_value(JSON_RPC_URL)
                        .validator(is_url)
                        .help("JSON RPC URL for the solana cluster"),
                )
                .arg(
                    Arg::with_name("validator_keypair")
                        .index(1)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("/path/to/validator-keypair.json"),
                )
                .arg(
                    Arg::with_name("info_pubkey")
                        .short("p")
                        .long("info-pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey)
                        .help("The pubkey of the Validator info account to update"),
                )
                .arg(
                    Arg::with_name("name")
                        .index(2)
                        .value_name("NAME")
                        .takes_value(true)
                        .required(true)
                        .validator(is_short_field)
                        .help("Validator name"),
                )
                .arg(
                    Arg::with_name("website")
                        .short("w")
                        .long("website")
                        .value_name("URL")
                        .takes_value(true)
                        .validator(check_url)
                        .help("Validator website url"),
                )
                .arg(
                    Arg::with_name("keybase_username")
                        .short("k")
                        .long("keybase")
                        .value_name("USERNAME")
                        .takes_value(true)
                        .validator(is_short_field)
                        .help("Validator Keybase username"),
                )
                .arg(
                    Arg::with_name("details")
                        .short("d")
                        .long("details")
                        .value_name("DETAILS")
                        .takes_value(true)
                        .validator(check_details_length)
                        .help(&format!(
                            "Validator description, max characters: {}",
                            MAX_LONG_FIELD_LENGTH
                        )),
                ),
        )
        .subcommand(
            SubCommand::with_name("get")
                .about("Get and parse Solana Validator info")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("json_rpc_url")
                        .short("u")
                        .long("url")
                        .value_name("URL")
                        .takes_value(true)
                        .default_value(JSON_RPC_URL)
                        .validator(is_url)
                        .help("JSON RPC URL for the solana cluster"),
                )
                .arg(
                    Arg::with_name("info_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .validator(is_pubkey)
                        .help("The pubkey of the Validator info account; without this argument, returns all"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("publish", Some(matches)) => {
            let json_rpc_url = matches.value_of("json_rpc_url").unwrap();
            let rpc_client = RpcClient::new(json_rpc_url.to_string());

            // Load validator-keypair
            let mut path = dirs::home_dir().expect("home directory");
            let id_path = if matches.is_present("validator_keypair") {
                matches.value_of("validator_keypair").unwrap()
            } else {
                path.extend(&[".config", "solana", "validator-keypair.json"]);
                if !path.exists() {
                    println!("No validator keypair file found. Run solana-keygen to create one.");
                    exit(1);
                }
                path.to_str().unwrap()
            };
            let validator_keypair = read_keypair(id_path)?;

            // Check for existing validator-info account
            let all_config = rpc_client.get_program_accounts(&solana_config_api::id())?;
            let existing_account = all_config
                .iter()
                .filter(|(_, account)| {
                    let key_list: ConfigKeys =
                        deserialize(&account.data).map_err(|_| false).unwrap();
                    key_list.keys.contains(&(id(), false))
                })
                .find(|(pubkey, account)| {
                    let (validator_pubkey, _) =
                        parse_validator_info(&pubkey, &account.data).unwrap();
                    validator_pubkey == validator_keypair.pubkey()
                });

            // Create validator-info keypair to use if info_pubkey not provided or does not exist
            let info_keypair = Keypair::new();
            let mut info_pubkey = if let Some(pubkey) = matches.value_of("info_pubkey") {
                pubkey.parse::<Pubkey>().unwrap()
            } else if let Some(validator_info) = existing_account {
                validator_info.0
            } else {
                info_keypair.pubkey()
            };

            // Prepare validator info
            let keys = vec![(id(), false), (validator_keypair.pubkey(), true)];
            let validator_info = parse_args(&matches);
            if let Some(string) = validator_info.get("keybaseUsername") {
                verify_keybase(&validator_keypair.pubkey(), &string)?;
            }
            let validator_string = serde_json::to_string(&validator_info)?;
            let validator_info = ValidatorInfo {
                info: validator_string,
            };

            // Check existence of validator-info account
            let balance = rpc_client.poll_get_balance(&info_pubkey).unwrap_or(0);

            let (message, signers): (Message, Vec<&Keypair>) = if balance == 0 {
                if info_pubkey != info_keypair.pubkey() {
                    println!(
                        "Account {:?} does not exist. Generating new keypair...",
                        info_pubkey
                    );
                    info_pubkey = info_keypair.pubkey();
                }
                println!(
                    "Publishing info for Validator {:?}",
                    validator_keypair.pubkey()
                );
                let instructions = vec![
                    config_instruction::create_account::<ValidatorInfo>(
                        &validator_keypair.pubkey(),
                        &info_keypair.pubkey(),
                        1,
                        keys.clone(),
                    ),
                    config_instruction::store(&info_keypair.pubkey(), true, keys, &validator_info),
                ];
                let signers = vec![&validator_keypair, &info_keypair];
                let message = Message::new(instructions);
                (message, signers)
            } else {
                println!(
                    "Updating Validator {:?} info at: {:?}",
                    validator_keypair.pubkey(),
                    info_pubkey
                );
                let instructions = vec![config_instruction::store(
                    &info_pubkey,
                    false,
                    keys,
                    &validator_info,
                )];
                let message =
                    Message::new_with_payer(instructions, Some(&validator_keypair.pubkey()));
                let signers = vec![&validator_keypair];
                (message, signers)
            };

            // Submit transaction
            let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
            let mut transaction = Transaction::new(&signers, message, recent_blockhash);
            let signature_str =
                rpc_client.send_and_confirm_transaction(&mut transaction, &signers)?;

            println!("Success! Validator info published at: {:?}", info_pubkey);
            println!("{}", signature_str);
        }
        ("get", Some(matches)) => {
            let json_rpc_url = matches.value_of("json_rpc_url").unwrap();
            let rpc_client = RpcClient::new(json_rpc_url.to_string());

            if matches.is_present("info_pubkey") {
                if let Some(pubkey) = matches.value_of("info_pubkey") {
                    let info_pubkey = pubkey.parse::<Pubkey>().unwrap();
                    let validator_info_data = rpc_client.get_account_data(&info_pubkey)?;
                    let (validator_pubkey, validator_info) =
                        parse_validator_info(&info_pubkey, &validator_info_data)?;
                    println!("Validator pubkey: {:?}", validator_pubkey);
                    println!("Info: {}", validator_info);
                }
            } else {
                let all_config = rpc_client.get_program_accounts(&solana_config_api::id())?;
                let all_validator_info: Vec<&(Pubkey, Account)> = all_config
                    .iter()
                    .filter(|(_, account)| {
                        let key_list: ConfigKeys =
                            deserialize(&account.data).map_err(|_| false).unwrap();
                        key_list.keys.contains(&(id(), false))
                    })
                    .collect();
                if all_validator_info.is_empty() {
                    println!("No validator info accounts found");
                }
                for (info_pubkey, account) in all_validator_info.iter() {
                    println!("Validator info from {:?}", info_pubkey);
                    let (validator_pubkey, validator_info) =
                        parse_validator_info(&info_pubkey, &account.data)?;
                    println!("  Validator pubkey: {:?}", validator_pubkey);
                    println!("  Info: {}", validator_info);
                }
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{serialize, serialized_size};
    use serde_json::json;

    #[test]
    fn test_check_url() {
        let url = "http://test.com";
        assert_eq!(check_url(url.to_string()), Ok(()));
        let long_url = "http://7cLvFwLCbyHuXQ1RGzhCMobAWYPMSZ3VbUml1qWi1nkc3FD7zj9hzTZzMvYJ.com";
        assert!(check_url(long_url.to_string()).is_err());
        let non_url = "not parseable";
        assert!(check_url(non_url.to_string()).is_err());
    }

    #[test]
    fn test_is_short_field() {
        let name = "Alice Validator";
        assert_eq!(is_short_field(name.to_string()), Ok(()));
        let long_name = "Alice 7cLvFwLCbyHuXQ1RGzhCMobAWYPMSZ3VbUml1qWi1nkc3FD7zj9hzTZzMvYJt6rY9";
        assert!(is_short_field(long_name.to_string()).is_err());
    }

    #[test]
    fn test_parse_args() {
        let matches = App::new("test")
            .arg(Arg::with_name("name").short("n").takes_value(true))
            .arg(Arg::with_name("website").short("w").takes_value(true))
            .arg(
                Arg::with_name("keybase_username")
                    .short("k")
                    .takes_value(true),
            )
            .arg(Arg::with_name("details").short("d").takes_value(true))
            .get_matches_from(vec!["test", "-n", "Alice", "-k", "alice_keybase"]);
        let expected = json!({
            "name": "Alice",
            "keybaseUsername": "alice_keybase",
        });
        assert_eq!(parse_args(&matches), expected);
    }

    #[test]
    fn test_validator_info_serde() {
        let mut info = Map::new();
        info.insert("name".to_string(), Value::String("Alice".to_string()));
        let info_string = serde_json::to_string(&Value::Object(info)).unwrap();

        let validator_info = ValidatorInfo {
            info: info_string.clone(),
        };

        assert_eq!(serialized_size(&validator_info).unwrap(), 24);
        assert_eq!(
            serialize(&validator_info).unwrap(),
            vec![
                16, 0, 0, 0, 0, 0, 0, 0, 123, 34, 110, 97, 109, 101, 34, 58, 34, 65, 108, 105, 99,
                101, 34, 125
            ]
        );

        let deserialized: ValidatorInfo = deserialize(&[
            16, 0, 0, 0, 0, 0, 0, 0, 123, 34, 110, 97, 109, 101, 34, 58, 34, 65, 108, 105, 99, 101,
            34, 125,
        ])
        .unwrap();
        assert_eq!(deserialized.info, info_string);
    }

    #[test]
    fn test_parse_validator_info() {
        let pubkey = Pubkey::new_rand();
        let keys = vec![(id(), false), (pubkey, true)];
        let config = ConfigKeys { keys };

        let mut info = Map::new();
        info.insert("name".to_string(), Value::String("Alice".to_string()));
        let info_string = serde_json::to_string(&Value::Object(info)).unwrap();
        let validator_info = ValidatorInfo {
            info: info_string.clone(),
        };
        let data = serialize(&(config, validator_info)).unwrap();

        assert_eq!(
            parse_validator_info(&Pubkey::default(), &data).unwrap(),
            (pubkey, info_string)
        );
    }

    #[test]
    fn test_validator_info_max_space() {
        // 70-character string
        let max_short_string =
            "Max Length String KWpP299aFCBWvWg1MHpSuaoTsud7cv8zMJsh99aAtP8X1s26yrR1".to_string();
        // 300-character string
        let max_long_string = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Ut libero quam, volutpat et aliquet eu, varius in mi. Aenean vestibulum ex in tristique faucibus. Maecenas in imperdiet turpis. Nullam feugiat aliquet erat. Morbi malesuada turpis sed dui pulvinar lobortis. Pellentesque a lectus eu leo nullam.".to_string();
        let mut info = Map::new();
        info.insert("name".to_string(), Value::String(max_short_string.clone()));
        info.insert(
            "website".to_string(),
            Value::String(max_short_string.clone()),
        );
        info.insert(
            "keybaseUsername".to_string(),
            Value::String(max_short_string),
        );
        info.insert("details".to_string(), Value::String(max_long_string));
        let info_string = serde_json::to_string(&Value::Object(info)).unwrap();

        let validator_info = ValidatorInfo {
            info: info_string.clone(),
        };

        assert_eq!(
            serialized_size(&validator_info).unwrap(),
            ValidatorInfo::max_space()
        );
    }
}
