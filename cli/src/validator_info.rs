use crate::{
    cli::{check_account_for_fee, CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    display::println_name_value,
    input_parsers::pubkey_of,
    input_validators::{is_pubkey, is_url},
};
use bincode::deserialize;
use clap::{App, Arg, ArgMatches, SubCommand};
use reqwest::Client;
use serde_derive::{Deserialize, Serialize};
use serde_json::{Map, Value};
use solana_client::rpc_client::RpcClient;
use solana_config_api::{config_instruction, get_config_data, ConfigKeys, ConfigState};
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    transaction::Transaction,
};
use std::error;

pub const MAX_SHORT_FIELD_LENGTH: usize = 70;
pub const MAX_LONG_FIELD_LENGTH: usize = 300;
pub const MAX_VALIDATOR_INFO: u64 = 576;

// Config account key: Va1idator1nfo111111111111111111111111111111
pub const REGISTER_CONFIG_KEY: [u8; 32] = [
    7, 81, 151, 1, 116, 72, 242, 172, 93, 194, 60, 158, 188, 122, 199, 140, 10, 39, 37, 122, 198,
    20, 69, 141, 224, 164, 241, 111, 128, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    REGISTER_CONFIG_KEY,
    "Va1idator1nfo111111111111111111111111111111"
);

#[derive(Debug, Deserialize, PartialEq, Serialize, Default)]
pub struct ValidatorInfo {
    info: String,
}

impl ConfigState for ValidatorInfo {
    fn max_space() -> u64 {
        MAX_VALIDATOR_INFO
    }
}

// Return an error if a validator details are longer than the max length.
pub fn check_details_length(string: String) -> Result<(), String> {
    if string.len() > MAX_LONG_FIELD_LENGTH {
        Err(format!(
            "validator details longer than {:?}-byte limit",
            MAX_LONG_FIELD_LENGTH
        ))
    } else {
        Ok(())
    }
}

// Return an error if url field is too long or cannot be parsed.
pub fn check_url(string: String) -> Result<(), String> {
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
pub fn is_short_field(string: String) -> Result<(), String> {
    if string.len() > MAX_SHORT_FIELD_LENGTH {
        Err(format!(
            "validator field longer than {:?}-byte limit",
            MAX_SHORT_FIELD_LENGTH
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
            Err(format!("keybase_username could not be confirmed at: {}. Please add this pubkey file to your keybase profile to connect", url).into())
        }
    } else {
        Err(format!(
            "keybase_username could not be parsed as String: {}",
            keybase_username
        )
        .into())
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
    account: &Account,
) -> Result<(Pubkey, Map<String, serde_json::value::Value>), Box<dyn error::Error>> {
    if account.owner != solana_config_api::id() {
        return Err(format!("{} is not a validator info account", pubkey).into());
    }
    let key_list: ConfigKeys = deserialize(&account.data)?;
    if !key_list.keys.is_empty() {
        let (validator_pubkey, _) = key_list.keys[1];
        let validator_info_string: String = deserialize(&get_config_data(&account.data)?)?;
        let validator_info: Map<_, _> = serde_json::from_str(&validator_info_string)?;
        Ok((validator_pubkey, validator_info))
    } else {
        Err(format!("{} could not be parsed as a validator info account", pubkey).into())
    }
}

pub trait ValidatorInfoSubCommands {
    fn validator_info_subcommands(self) -> Self;
}

impl ValidatorInfoSubCommands for App<'_, '_> {
    fn validator_info_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("validator-info")
                .about("Publish/get Validator info on Solana")
                .subcommand(
                    SubCommand::with_name("publish")
                        .about("Publish Validator info on Solana")
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
                                .index(1)
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
                                .short("n")
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
                                .help("Validator description")
                        )
                        .arg(
                            Arg::with_name("force")
                                .long("force")
                                .takes_value(false)
                                .hidden(true) // Don't document this argument to discourage its use
                                .help("Override keybase username validity check"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("get")
                        .about("Get and parse Solana Validator info")
                        .arg(
                            Arg::with_name("info_pubkey")
                                .index(1)
                                .value_name("PUBKEY")
                                .takes_value(true)
                                .validator(is_pubkey)
                                .help("The pubkey of the Validator info account; without this argument, returns all"),
                        ),
                )
        )
    }
}

pub fn parse_validator_info_command(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let info_pubkey = pubkey_of(matches, "info_pubkey");
    // Prepare validator info
    let validator_info = parse_args(&matches);
    Ok(CliCommandInfo {
        command: CliCommand::SetValidatorInfo {
            validator_info,
            force_keybase: matches.is_present("force"),
            info_pubkey,
        },
        require_keypair: true,
    })
}

pub fn parse_get_validator_info_command(
    matches: &ArgMatches<'_>,
) -> Result<CliCommandInfo, CliError> {
    let info_pubkey = pubkey_of(matches, "info_pubkey");
    Ok(CliCommandInfo {
        command: CliCommand::GetValidatorInfo(info_pubkey),
        require_keypair: false,
    })
}

pub fn process_set_validator_info(
    rpc_client: &RpcClient,
    config: &CliConfig,
    validator_info: &Value,
    force_keybase: bool,
    info_pubkey: Option<Pubkey>,
) -> ProcessResult {
    // Validate keybase username
    if let Some(string) = validator_info.get("keybaseUsername") {
        let result = verify_keybase(&config.keypair.pubkey(), &string);
        if result.is_err() {
            if force_keybase {
                println!("--force supplied, ignoring: {:?}", result);
            } else {
                result.map_err(|err| {
                    CliError::BadParameter(format!("Invalid validator keybase username: {:?}", err))
                })?;
            }
        }
    }
    let validator_string = serde_json::to_string(&validator_info).unwrap();
    let validator_info = ValidatorInfo {
        info: validator_string,
    };
    // Check for existing validator-info account
    let all_config = rpc_client.get_program_accounts(&solana_config_api::id())?;
    let existing_account = all_config
        .iter()
        .filter(|(_, account)| {
            let key_list: ConfigKeys = deserialize(&account.data).map_err(|_| false).unwrap();
            key_list.keys.contains(&(id(), false))
        })
        .find(|(pubkey, account)| {
            let (validator_pubkey, _) = parse_validator_info(&pubkey, &account).unwrap();
            validator_pubkey == config.keypair.pubkey()
        });

    // Create validator-info keypair to use if info_pubkey not provided or does not exist
    let info_keypair = Keypair::new();
    let mut info_pubkey = if let Some(pubkey) = info_pubkey {
        pubkey
    } else if let Some(validator_info) = existing_account {
        validator_info.0
    } else {
        info_keypair.pubkey()
    };

    // Check existence of validator-info account
    let balance = rpc_client
        .poll_get_balance_with_commitment(&info_pubkey, CommitmentConfig::default())
        .unwrap_or(0);

    let keys = vec![(id(), false), (config.keypair.pubkey(), true)];
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
            config.keypair.pubkey()
        );
        let mut instructions = config_instruction::create_account::<ValidatorInfo>(
            &config.keypair.pubkey(),
            &info_keypair.pubkey(),
            1,
            keys.clone(),
        );
        instructions.extend_from_slice(&[config_instruction::store(
            &info_keypair.pubkey(),
            true,
            keys,
            &validator_info,
        )]);
        let signers = vec![&config.keypair, &info_keypair];
        let message = Message::new(instructions);
        (message, signers)
    } else {
        println!(
            "Updating Validator {:?} info at: {:?}",
            config.keypair.pubkey(),
            info_pubkey
        );
        let instructions = vec![config_instruction::store(
            &info_pubkey,
            false,
            keys,
            &validator_info,
        )];
        let message = Message::new_with_payer(instructions, Some(&config.keypair.pubkey()));
        let signers = vec![&config.keypair];
        (message, signers)
    };

    // Submit transaction
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new(&signers, message, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &signers)?;

    println!("Success! Validator info published at: {:?}", info_pubkey);
    println!("{}", signature_str);
    Ok("".to_string())
}

pub fn process_get_validator_info(rpc_client: &RpcClient, pubkey: Option<Pubkey>) -> ProcessResult {
    let validator_info: Vec<(Pubkey, Account)> = if let Some(validator_info_pubkey) = pubkey {
        vec![(
            validator_info_pubkey,
            rpc_client.get_account(&validator_info_pubkey)?,
        )]
    } else {
        let all_config = rpc_client.get_program_accounts(&solana_config_api::id())?;
        all_config
            .into_iter()
            .filter(|(_, validator_info_account)| {
                let key_list: ConfigKeys = deserialize(&validator_info_account.data)
                    .map_err(|_| false)
                    .unwrap();
                key_list.keys.contains(&(id(), false))
            })
            .collect()
    };

    if validator_info.is_empty() {
        println!("No validator info accounts found");
    }
    for (validator_info_pubkey, validator_info_account) in validator_info.iter() {
        let (validator_pubkey, validator_info) =
            parse_validator_info(&validator_info_pubkey, &validator_info_account)?;
        println!();
        println_name_value("Validator Identity Pubkey:", &validator_pubkey.to_string());
        println_name_value("  info pubkey:", &validator_info_pubkey.to_string());
        for (key, value) in validator_info.iter() {
            println_name_value(&format!("  {}:", key), &value.as_str().unwrap_or("?"));
        }
    }

    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::app;
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
        let matches = app("test", "desc", "version").get_matches_from(vec![
            "test",
            "validator-info",
            "publish",
            "Alice",
            "-n",
            "alice_keybase",
        ]);
        let subcommand_matches = matches.subcommand();
        assert_eq!(subcommand_matches.0, "validator-info");
        assert!(subcommand_matches.1.is_some());
        let subcommand_matches = subcommand_matches.1.unwrap().subcommand();
        assert_eq!(subcommand_matches.0, "publish");
        assert!(subcommand_matches.1.is_some());
        let matches = subcommand_matches.1.unwrap();
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
        let info_string = serde_json::to_string(&Value::Object(info.clone())).unwrap();
        let validator_info = ValidatorInfo {
            info: info_string.clone(),
        };
        let data = serialize(&(config, validator_info)).unwrap();

        assert_eq!(
            parse_validator_info(
                &Pubkey::default(),
                &Account {
                    owner: solana_config_api::id(),
                    data,
                    ..Account::default()
                }
            )
            .unwrap(),
            (pubkey, info)
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
