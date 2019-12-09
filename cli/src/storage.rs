use crate::cli::{
    check_account_for_fee, check_unique_pubkeys, log_instruction_custom_error, CliCommand,
    CliCommandInfo, CliConfig, CliError, ProcessResult,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{input_parsers::*, input_validators::*};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signature::Keypair;
use solana_sdk::{
    account_utils::State, message::Message, pubkey::Pubkey, signature::KeypairUtil,
    system_instruction::SystemError, transaction::Transaction,
};
use solana_storage_program::storage_instruction::{self, StorageAccountType};

pub trait StorageSubCommands {
    fn storage_subcommands(self) -> Self;
}

impl StorageSubCommands for App<'_, '_> {
    fn storage_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("create-archiver-storage-account")
                .about("Create an archiver storage account")
                .arg(
                    Arg::with_name("storage_account_owner")
                        .index(1)
                        .value_name("STORAGE ACCOUNT OWNER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair),
                )
                .arg(
                    Arg::with_name("storage_account")
                        .index(2)
                        .value_name("STORAGE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair_or_ask_keyword),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-validator-storage-account")
                .about("Create a validator storage account")
                .arg(
                    Arg::with_name("storage_account_owner")
                        .index(1)
                        .value_name("STORAGE ACCOUNT OWNER PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair),
                )
                .arg(
                    Arg::with_name("storage_account")
                        .index(2)
                        .value_name("STORAGE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair_or_ask_keyword),
                ),
        )
        .subcommand(
            SubCommand::with_name("claim-storage-reward")
                .about("Redeem storage reward credits")
                .arg(
                    Arg::with_name("node_account_pubkey")
                        .index(1)
                        .value_name("NODE PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The node account to credit the rewards to"),
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Storage account address to redeem credits for"),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-storage-account")
                .about("Show the contents of a storage account")
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(1)
                        .value_name("STORAGE ACCOUNT PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Storage account pubkey"),
                ),
        )
    }
}

pub fn parse_storage_create_archiver_account(
    matches: &ArgMatches<'_>,
) -> Result<CliCommandInfo, CliError> {
    let account_owner = pubkey_of(matches, "storage_account_owner").unwrap();
    let storage_account = keypair_of(matches, "storage_account").unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::CreateStorageAccount {
            account_owner,
            storage_account: storage_account.into(),
            account_type: StorageAccountType::Archiver,
        },
        require_keypair: true,
    })
}

pub fn parse_storage_create_validator_account(
    matches: &ArgMatches<'_>,
) -> Result<CliCommandInfo, CliError> {
    let account_owner = pubkey_of(matches, "storage_account_owner").unwrap();
    let storage_account = keypair_of(matches, "storage_account").unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::CreateStorageAccount {
            account_owner,
            storage_account: storage_account.into(),
            account_type: StorageAccountType::Validator,
        },
        require_keypair: true,
    })
}

pub fn parse_storage_claim_reward(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let node_account_pubkey = pubkey_of(matches, "node_account_pubkey").unwrap();
    let storage_account_pubkey = pubkey_of(matches, "storage_account_pubkey").unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::ClaimStorageReward {
            node_account_pubkey,
            storage_account_pubkey,
        },
        require_keypair: true,
    })
}

pub fn parse_storage_get_account_command(
    matches: &ArgMatches<'_>,
) -> Result<CliCommandInfo, CliError> {
    let storage_account_pubkey = pubkey_of(matches, "storage_account_pubkey").unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::ShowStorageAccount(storage_account_pubkey),
        require_keypair: false,
    })
}

pub fn process_create_storage_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    account_owner: &Pubkey,
    storage_account: &Keypair,
    account_type: StorageAccountType,
) -> ProcessResult {
    let storage_account_pubkey = storage_account.pubkey();
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (
            &storage_account_pubkey,
            "storage_account_pubkey".to_string(),
        ),
    )?;
    use solana_storage_program::storage_contract::STORAGE_ACCOUNT_SPACE;
    let required_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(STORAGE_ACCOUNT_SPACE as usize)?
        .max(1);

    let ixs = storage_instruction::create_storage_account(
        &config.keypair.pubkey(),
        &account_owner,
        &storage_account_pubkey,
        required_balance,
        account_type,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let mut tx = Transaction::new_signed_instructions(
        &[&config.keypair, &storage_account],
        ixs,
        recent_blockhash,
    );
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, &storage_account]);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_claim_storage_reward(
    rpc_client: &RpcClient,
    config: &CliConfig,
    node_account_pubkey: &Pubkey,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let instruction =
        storage_instruction::claim_reward(node_account_pubkey, storage_account_pubkey);
    let signers = [&config.keypair];
    let message = Message::new_with_payer(vec![instruction], Some(&signers[0].pubkey()));

    let mut tx = Transaction::new(&signers, message, recent_blockhash);
    check_account_for_fee(rpc_client, config, &fee_calculator, &tx.message)?;
    let signature_str = rpc_client.send_and_confirm_transaction(&mut tx, &signers)?;
    Ok(signature_str.to_string())
}

pub fn process_show_storage_account(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let account = rpc_client.get_account(storage_account_pubkey)?;

    if account.owner != solana_storage_program::id() {
        return Err(CliError::RpcRequestError(
            format!("{:?} is not a storage account", storage_account_pubkey).to_string(),
        )
        .into());
    }

    use solana_storage_program::storage_contract::StorageContract;
    let storage_contract: StorageContract = account.state().map_err(|err| {
        CliError::RpcRequestError(
            format!("Unable to deserialize storage account: {:?}", err).to_string(),
        )
    })?;
    println!("{:#?}", storage_contract);
    println!("account lamports: {}", account.lamports);
    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::signature::write_keypair;
    use tempfile::NamedTempFile;

    fn make_tmp_file() -> (String, NamedTempFile) {
        let tmp_file = NamedTempFile::new().unwrap();
        (String::from(tmp_file.path().to_str().unwrap()), tmp_file)
    }

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let pubkey = Pubkey::new_rand();
        let pubkey_string = pubkey.to_string();

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let storage_account_keypair = Keypair::new();
        write_keypair(&storage_account_keypair, tmp_file.as_file_mut()).unwrap();

        let test_create_archiver_storage_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-archiver-storage-account",
            &pubkey_string,
            &keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_create_archiver_storage_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStorageAccount {
                    account_owner: pubkey,
                    storage_account: storage_account_keypair.into(),
                    account_type: StorageAccountType::Archiver,
                },
                require_keypair: true
            }
        );

        let (keypair_file, mut tmp_file) = make_tmp_file();
        let storage_account_keypair = Keypair::new();
        write_keypair(&storage_account_keypair, tmp_file.as_file_mut()).unwrap();
        let storage_account_pubkey = storage_account_keypair.pubkey();
        let storage_account_string = storage_account_pubkey.to_string();

        let test_create_validator_storage_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-validator-storage-account",
            &pubkey_string,
            &keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_create_validator_storage_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStorageAccount {
                    account_owner: pubkey,
                    storage_account: storage_account_keypair.into(),
                    account_type: StorageAccountType::Validator,
                },
                require_keypair: true
            }
        );

        let test_claim_storage_reward = test_commands.clone().get_matches_from(vec![
            "test",
            "claim-storage-reward",
            &pubkey_string,
            &storage_account_string,
        ]);
        assert_eq!(
            parse_command(&test_claim_storage_reward).unwrap(),
            CliCommandInfo {
                command: CliCommand::ClaimStorageReward {
                    node_account_pubkey: pubkey,
                    storage_account_pubkey,
                },
                require_keypair: true
            }
        );
    }
}
