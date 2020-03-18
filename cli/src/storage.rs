use crate::cli::{
    check_account_for_fee, check_unique_pubkeys, generate_unique_signers,
    log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult,
    SignerIndex,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{input_parsers::*, input_validators::*, keypair::signer_from_path};
use solana_client::rpc_client::RpcClient;
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{
    account_utils::StateMut, message::Message, pubkey::Pubkey, system_instruction::SystemError,
    transaction::Transaction,
};
use solana_storage_program::storage_instruction::{self, StorageAccountType};
use std::sync::Arc;

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
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_pubkey),
                )
                .arg(
                    Arg::with_name("storage_account")
                        .index(2)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer),
                ),
        )
        .subcommand(
            SubCommand::with_name("create-validator-storage-account")
                .about("Create a validator storage account")
                .arg(
                    Arg::with_name("storage_account_owner")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_pubkey),
                )
                .arg(
                    Arg::with_name("storage_account")
                        .index(2)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer),
                ),
        )
        .subcommand(
            SubCommand::with_name("claim-storage-reward")
                .about("Redeem storage reward credits")
                .arg(
                    Arg::with_name("node_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_pubkey)
                        .help("The node account to credit the rewards to"),
                )
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(2)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_pubkey)
                        .help("Storage account address to redeem credits for"),
                ),
        )
        .subcommand(
            SubCommand::with_name("storage-account")
                .about("Show the contents of a storage account")
                .alias("show-storage-account")
                .arg(
                    Arg::with_name("storage_account_pubkey")
                        .index(1)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_pubkey)
                        .help("Storage account pubkey"),
                ),
        )
    }
}

pub fn parse_storage_create_archiver_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let account_owner =
        pubkey_of_signer(matches, "storage_account_owner", wallet_manager)?.unwrap();
    let (storage_account, storage_account_pubkey) =
        signer_of(matches, "storage_account", wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, storage_account],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::CreateStorageAccount {
            account_owner,
            storage_account: signer_info.index_of(storage_account_pubkey).unwrap(),
            account_type: StorageAccountType::Archiver,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_storage_create_validator_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let account_owner =
        pubkey_of_signer(matches, "storage_account_owner", wallet_manager)?.unwrap();
    let (storage_account, storage_account_pubkey) =
        signer_of(matches, "storage_account", wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, storage_account],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::CreateStorageAccount {
            account_owner,
            storage_account: signer_info.index_of(storage_account_pubkey).unwrap(),
            account_type: StorageAccountType::Validator,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_storage_claim_reward(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let node_account_pubkey =
        pubkey_of_signer(matches, "node_account_pubkey", wallet_manager)?.unwrap();
    let storage_account_pubkey =
        pubkey_of_signer(matches, "storage_account_pubkey", wallet_manager)?.unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::ClaimStorageReward {
            node_account_pubkey,
            storage_account_pubkey,
        },
        signers: vec![signer_from_path(
            matches,
            default_signer_path,
            "keypair",
            wallet_manager,
        )?],
    })
}

pub fn parse_storage_get_account_command(
    matches: &ArgMatches<'_>,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let storage_account_pubkey =
        pubkey_of_signer(matches, "storage_account_pubkey", wallet_manager)?.unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::ShowStorageAccount(storage_account_pubkey),
        signers: vec![],
    })
}

pub fn process_create_storage_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    storage_account: SignerIndex,
    account_owner: &Pubkey,
    account_type: StorageAccountType,
) -> ProcessResult {
    let storage_account = config.signers[storage_account];
    let storage_account_pubkey = storage_account.pubkey();
    check_unique_pubkeys(
        (&config.signers[0].pubkey(), "cli keypair".to_string()),
        (
            &storage_account_pubkey,
            "storage_account_pubkey".to_string(),
        ),
    )?;

    if let Ok(storage_account) = rpc_client.get_account(&storage_account_pubkey) {
        let err_msg = if storage_account.owner == solana_storage_program::id() {
            format!("Storage account {} already exists", storage_account_pubkey)
        } else {
            format!(
                "Account {} already exists and is not a storage account",
                storage_account_pubkey
            )
        };
        return Err(CliError::BadParameter(err_msg).into());
    }

    use solana_storage_program::storage_contract::STORAGE_ACCOUNT_SPACE;
    let required_balance = rpc_client
        .get_minimum_balance_for_rent_exemption(STORAGE_ACCOUNT_SPACE as usize)?
        .max(1);

    let ixs = storage_instruction::create_storage_account(
        &config.signers[0].pubkey(),
        &account_owner,
        &storage_account_pubkey,
        required_balance,
        account_type,
    );
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let message = Message::new(&ixs);
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &config.signers);
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
    let signers = [config.signers[0]];
    let message = Message::new_with_payer(&[instruction], Some(&signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let signature_str = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &signers)?;
    Ok(signature_str)
}

pub fn process_show_storage_account(
    rpc_client: &RpcClient,
    _config: &CliConfig,
    storage_account_pubkey: &Pubkey,
) -> ProcessResult {
    let account = rpc_client.get_account(storage_account_pubkey)?;

    if account.owner != solana_storage_program::id() {
        return Err(CliError::RpcRequestError(format!(
            "{:?} is not a storage account",
            storage_account_pubkey
        ))
        .into());
    }

    use solana_storage_program::storage_contract::StorageContract;
    let storage_contract: StorageContract = account.state().map_err(|err| {
        CliError::RpcRequestError(format!("Unable to deserialize storage account: {}", err))
    })?;
    println!("{:#?}", storage_contract);
    println!("Account Lamports: {}", account.lamports);
    Ok("".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::signature::{read_keypair_file, write_keypair, Keypair, Signer};
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

        let default_keypair = Keypair::new();
        let (default_keypair_file, mut tmp_file) = make_tmp_file();
        write_keypair(&default_keypair, tmp_file.as_file_mut()).unwrap();

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
            parse_command(
                &test_create_archiver_storage_account,
                &default_keypair_file,
                None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStorageAccount {
                    account_owner: pubkey,
                    storage_account: 1,
                    account_type: StorageAccountType::Archiver,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    storage_account_keypair.into()
                ],
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
            parse_command(
                &test_create_validator_storage_account,
                &default_keypair_file,
                None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateStorageAccount {
                    account_owner: pubkey,
                    storage_account: 1,
                    account_type: StorageAccountType::Validator,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    storage_account_keypair.into()
                ],
            }
        );

        let test_claim_storage_reward = test_commands.clone().get_matches_from(vec![
            "test",
            "claim-storage-reward",
            &pubkey_string,
            &storage_account_string,
        ]);
        assert_eq!(
            parse_command(&test_claim_storage_reward, &default_keypair_file, None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ClaimStorageReward {
                    node_account_pubkey: pubkey,
                    storage_account_pubkey,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );
    }
}
