use bincode::serialize;
use clap::{
    crate_description, crate_name, crate_version, App, AppSettings, Arg, ArgMatches, SubCommand,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use solana_validator_info_api::validator_info_instruction;
use std::collections::HashSet;
use std::error;
use std::process::exit;
use vcard::properties::*;
use vcard::values::{text, uid_value, url as vcard_url};
use vcard::{Set, VCard};

pub const MAX_SHORT_FIELD_LENGTH: usize = 70;
pub const MAX_LONG_FIELD_LENGTH: usize = 240;
pub const JSON_RPC_URL: &str = "https://api.testnet.solana.com/";

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

// Return an error if a pubkey cannot be parsed.
fn is_pubkey(string: String) -> Result<(), String> {
    match string.parse::<Pubkey>() {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{:?}", err)),
    }
}

// Return an error if url field is too long or cannot be parsed.
fn check_url(string: String) -> Result<(), String> {
    is_url(string.clone())?;
    let parsed_string = text::Text::from_string(string).unwrap().into_string();
    if serialize(&parsed_string).unwrap().len() > MAX_SHORT_FIELD_LENGTH {
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
    let parsed_string = text::Text::from_string(string).unwrap().into_string();
    if serialize(&parsed_string).unwrap().len() > MAX_SHORT_FIELD_LENGTH {
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
    let parsed_string = text::Text::from_string(string).unwrap().into_string();
    if serialize(&parsed_string).unwrap().len() > MAX_LONG_FIELD_LENGTH {
        Err(format!(
            "validator details longer than {:?}-byte limit",
            MAX_LONG_FIELD_LENGTH
        ))
    } else {
        Ok(())
    }
}

// Return a parsed value from matches at `name`
fn value_of<T>(matches: &ArgMatches<'_>, name: &str) -> Option<T>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches
        .value_of(name)
        .map(|value| value.parse::<T>().unwrap())
}

fn parse_args(matches: &ArgMatches<'_>) -> Result<VCard, Box<dyn error::Error>> {
    let name = matches.value_of("name").unwrap();
    let mut vcard = VCard::from_formatted_name_str(name).unwrap();
    if let Some(url) = matches.value_of("website") {
        let mut urls = HashSet::new();
        urls.insert(URL::from_url(vcard_url::URL::from_str(url).unwrap()));
        vcard.urls = Some(Set::from_hash_set(urls).unwrap());
    }
    if let Some(details) = matches.value_of("details") {
        let mut notes = HashSet::new();
        notes.insert(Note::from_text(text::Text::from_str(details).unwrap()));
        vcard.notes = Some(Set::from_hash_set(notes).unwrap());
    }
    if let Some(keybase) = matches.value_of("keybase") {
        vcard.uid = Some(UID::from_uid_value(uid_value::UIDValue::Text(
            text::Text::from_str(keybase).unwrap(),
        )));
    }
    Ok(vcard)
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
            Arg::with_name("keypair")
                .short("k")
                .long("keypair")
                .value_name("PATH")
                .takes_value(true)
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
                        .short("i")
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
                        .help("Validator description, max characters: ###"),
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
                    Arg::with_name("info_pubkey")
                        .short("p")
                        .long("info-pubkey")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey)
                        .help("Location of Validator Info"),
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
                        .short("i")
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
                        .help("Validator description, max characters: ###"),
                ),
        )
        .get_matches();

    let json_rpc_url = matches.value_of("json_rpc_url").unwrap();

    let mut path = dirs::home_dir().expect("home directory");
    let id_path = if matches.is_present("keypair") {
        matches.value_of("keypair").unwrap()
    } else {
        path.extend(&[".config", "solana", "id.json"]);
        if !path.exists() {
            println!("No keypair file found. Run solana-keygen to create one.");
            exit(1);
        }
        path.to_str().unwrap()
    };
    let validator_keypair = read_keypair(id_path)?;
    let info_account_pubkey: Pubkey;

    let instructions = match matches.subcommand() {
        ("new", Some(matches)) => {
            let vcard = parse_args(&matches)?;
            info_account_pubkey = Pubkey::new_rand();
            println!("Registering Validator:");
            println!("{}", vcard);
            validator_info_instruction::create_account(
                &validator_keypair.pubkey(),
                &info_account_pubkey,
                &vcard,
            )
        }
        ("update", Some(matches)) => {
            let vcard = parse_args(&matches)?;
            info_account_pubkey = value_of(matches, "info_pubkey").unwrap();
            println!("Updating Validator info at {:?} to:", info_account_pubkey);
            println!("{}", vcard);
            vec![validator_info_instruction::write_validator_info(
                &validator_keypair.pubkey(),
                &info_account_pubkey,
                &vcard,
            )]
        }
        _ => unreachable!(),
    };

    let rpc_client = RpcClient::new(json_rpc_url.to_string());
    let (recent_blockhash, _fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx =
        Transaction::new_signed_instructions(&[&validator_keypair], instructions, recent_blockhash);
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &[&validator_keypair])?;

    println!("Success! Validator info at: {:?}", info_account_pubkey);
    println!("{}", signature_str);

    Ok(())
}
