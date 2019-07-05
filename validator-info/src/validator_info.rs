use clap::{
    crate_description, crate_name, crate_version, App, AppSettings, Arg, ArgMatches, SubCommand,
};
use serde_derive::Serialize;
use solana_client::rpc_client::RpcClient;
use solana_config_api::{config_instruction, ConfigState};
use solana_sdk::message::Message;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use std::error;
use std::process::exit;

pub const MAX_SHORT_FIELD_LENGTH: usize = 70;
pub const MAX_LONG_FIELD_LENGTH: usize = 256;
pub const JSON_RPC_URL: &str = "https://api.testnet.solana.com/";

// Config account key: Va1idator1nfo111111111111111111111111111111
pub const REGISTER_CONFIG_KEY: [u8; 32] = [
    7, 81, 151, 1, 116, 72, 242, 172, 93, 194, 60, 158, 188, 122, 199, 140, 10, 39, 37, 122, 198,
    20, 69, 141, 224, 164, 241, 111, 128, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    REGISTER_CONFIG_KEY,
    "Va1idator1nfo111111111111111111111111111111"
);

#[derive(Debug, Serialize)]
struct ValidatorInfo {
    #[serde(with = "key_value_vec")]
    info: Vec<String>,
}

mod key_value_vec {
    use serde::ser::{self, SerializeTuple, Serializer};

    pub fn serialize<S: Serializer>(elements: &[String], serializer: S) -> Result<S::Ok, S::Error> {
        // Pass a non-zero value to serialize_tuple() so that serde_json will
        // generate an open bracket.
        let mut seq = serializer.serialize_tuple(1)?;

        for element in elements {
            let len = element.len();
            if len > std::u8::MAX as usize {
                return Err(ser::Error::custom("length larger than u8"));
            }
            seq.serialize_element(&(len as u8))?;
            for byte in element.as_bytes() {
                seq.serialize_element(&byte)?;
            }
        }
        seq.end()
    }
}

impl ConfigState for ValidatorInfo {
    fn max_space() -> u64 {
        512
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

// Return an error if a validator name is longer than the max length.
fn check_name_length(string: String) -> Result<(), String> {
    if string.len() > MAX_SHORT_FIELD_LENGTH {
        Err(format!(
            "validator name longer than {:?}-byte limit",
            MAX_SHORT_FIELD_LENGTH
        ))
    } else {
        Ok(())
    }
}

// Return an error if invalid keybase id is submitted.
fn is_keybase(string: String) -> Result<(), String> {
    let mut invalid = string.len() != 16;
    for c in string.chars() {
        if !c.is_digit(16) {
            invalid = true;
        }
    }
    if invalid {
        Err("keybase id is invalid".to_string())
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

fn parse_args(matches: &ArgMatches<'_>) -> Result<Vec<String>, Box<dyn error::Error>> {
    let mut strings = vec!["name".to_string()];
    strings.push(matches.value_of("name").unwrap().to_string());
    if let Some(url) = matches.value_of("website") {
        strings.push("website".to_string());
        strings.push(url.to_string());
    }
    if let Some(details) = matches.value_of("details") {
        strings.push("details".to_string());
        strings.push(details.to_string());
    }
    if let Some(keybase) = matches.value_of("keybase") {
        strings.push("keybase".to_string());
        strings.push(keybase.to_string());
    }
    Ok(strings)
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
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
                .short("v")
                .long("validator-keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("/path/to/id.json"),
        )
        .arg(
            Arg::with_name("info_keypair")
                .short("i")
                .long("info-keypair")
                .value_name("PATH")
                .takes_value(true)
                .required(true)
                .help("/path/to/id.json"),
        )
        .subcommand(
            SubCommand::with_name("new")
                .about("Register a new Validator on Solana")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("name")
                        .short("n")
                        .long("name")
                        .value_name("STRING")
                        .takes_value(true)
                        .required(true)
                        .validator(check_name_length)
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
                    Arg::with_name("keybase")
                        .short("k")
                        .long("keybase-id")
                        .value_name("STRING")
                        .takes_value(true)
                        .validator(is_keybase)
                        .help("Validator Keybase id"),
                )
                .arg(
                    Arg::with_name("details")
                        .short("d")
                        .long("details")
                        .value_name("STRING")
                        .takes_value(true)
                        .validator(check_details_length)
                        .help(&format!(
                            "Validator description, max characters: {}",
                            MAX_LONG_FIELD_LENGTH
                        )),
                ),
        )
        .subcommand(
            SubCommand::with_name("update")
                .about("Update Validator registration info")
                .setting(AppSettings::DisableVersion)
                .arg(
                    Arg::with_name("name")
                        .short("n")
                        .long("name")
                        .value_name("STRING")
                        .takes_value(true)
                        .required(true)
                        .validator(check_name_length)
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
                    Arg::with_name("keybase")
                        .short("k")
                        .long("keybase-id")
                        .value_name("STRING")
                        .takes_value(true)
                        .validator(is_keybase)
                        .help("Validator Keybase id"),
                )
                .arg(
                    Arg::with_name("details")
                        .short("d")
                        .long("details")
                        .value_name("STRING")
                        .takes_value(true)
                        .validator(check_details_length)
                        .help(&format!(
                            "Validator description, max characters: {}",
                            MAX_LONG_FIELD_LENGTH
                        )),
                ),
        )
        .get_matches();

    let json_rpc_url = matches.value_of("json_rpc_url").unwrap();

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

    let mut path = dirs::home_dir().expect("home directory");
    let id_path = if matches.is_present("info_keypair") {
        matches.value_of("info_keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "validator-info.json"]);
        if !path.exists() {
            println!("No info keypair file found. Run solana-keygen to create one.");
            exit(1);
        }
        path.to_str().unwrap()
    };
    let info_keypair = read_keypair(id_path)?;

    let keys = vec![(id(), false), (validator_keypair.pubkey(), true)];

    let (message, signers): (Message, [&Keypair; 2]) = match matches.subcommand() {
        ("new", Some(matches)) => {
            let validator_info = ValidatorInfo {
                info: parse_args(&matches)?,
            };
            println!("Registering Validator {:?}", validator_keypair.pubkey());
            let instructions = vec![
                config_instruction::create_account::<ValidatorInfo>(
                    &validator_keypair.pubkey(),
                    &info_keypair.pubkey(),
                    1,
                    keys.clone(),
                ),
                config_instruction::store(&info_keypair.pubkey(), keys, &validator_info),
            ];
            let signers = [&validator_keypair, &info_keypair];
            let message = Message::new(instructions);
            (message, signers)
        }
        ("update", Some(matches)) => {
            let validator_info = ValidatorInfo {
                info: parse_args(&matches)?,
            };
            println!(
                "Updating Validator {:?} info at: {:?}",
                validator_keypair.pubkey(),
                info_keypair.pubkey()
            );
            let instructions = vec![config_instruction::store(
                &info_keypair.pubkey(),
                keys,
                &validator_info,
            )];
            let message = Message::new_with_payer(instructions, Some(&validator_keypair.pubkey()));
            let signers = [&validator_keypair, &info_keypair];
            (message, signers)
        }
        _ => unreachable!(),
    };

    let rpc_client = RpcClient::new(json_rpc_url.to_string());
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut transaction = Transaction::new(&signers, message, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut transaction, &signers)?;

    println!(
        "Success! Validator info stored at: {:?}",
        info_keypair.pubkey()
    );
    println!("{}", signature_str);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{serialize, serialized_size};

    #[test]
    fn test_key_validator_info_serialize() {
        let info = ValidatorInfo {
            info: vec!["name".to_string(), "Alice".to_string()],
        };

        assert_eq!(serialized_size(&info).unwrap(), 11);
        assert_eq!(
            serialize(&info).unwrap(),
            [4, 110, 97, 109, 101, 5, 65, 108, 105, 99, 101]
        );
    }
}
