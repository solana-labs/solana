use crate::cli::{
    build_balance_message, check_account_for_fee, check_unique_pubkeys,
    log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{input_parsers::*, input_validators::*};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    account_utils::State,
    hash::Hash,
    nonce_instruction::{create_nonce_account, nonce, withdraw, NonceError},
    nonce_program,
    nonce_state::NonceState,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction::SystemError,
    transaction::Transaction,
};

pub trait NonceSubCommands {
    fn nonce_subcommands(self) -> Self;
}

impl NonceSubCommands for App<'_, '_> {
    fn nonce_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("create-nonce-account")
                .about("Create a nonce account")
                .arg(
                    Arg::with_name("nonce_account_keypair")
                        .index(1)
                        .value_name("NONCE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair_or_ask_keyword)
                        .help("Keypair of the nonce account to fund"),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_amount)
                        .help("The amount to load the nonce account with (default unit SOL)"),
                )
                .arg(
                    Arg::with_name("unit")
                        .index(3)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request"),
                ),
        )
        .subcommand(
            SubCommand::with_name("get-nonce")
                .about("Get the current nonce value")
                .arg(
                    Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the nonce account to display"),
                ),
        )
        .subcommand(
            SubCommand::with_name("new-nonce")
                .about("Generate a new nonce, rendering the existing nonce useless")
                .arg(
                    Arg::with_name("nonce_account_keypair")
                        .index(1)
                        .value_name("NONCE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the nonce account"),
                ),
        )
        .subcommand(
            SubCommand::with_name("show-nonce-account")
                .about("Show the contents of a nonce account")
                .arg(
                    Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the nonce account to display"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
        .subcommand(
            SubCommand::with_name("withdraw-from-nonce-account")
                .about("Withdraw lamports from the nonce account")
                .arg(
                    Arg::with_name("nonce_account_keypair")
                        .index(1)
                        .value_name("NONCE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_keypair_or_ask_keyword)
                        .help("Nonce account from to withdraw from"),
                )
                .arg(
                    Arg::with_name("destination_account_pubkey")
                        .index(2)
                        .value_name("DESTINATION ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("The account to which the lamports should be transferred"),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(3)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_amount)
                        .help("The amount to withdraw from the nonce account (default unit SOL)"),
                )
                .arg(
                    Arg::with_name("unit")
                        .index(4)
                        .value_name("UNIT")
                        .takes_value(true)
                        .possible_values(&["SOL", "lamports"])
                        .help("Specify unit to use for request"),
                ),
        )
    }
}

pub fn parse_nonce_create_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let nonce_account = keypair_of(matches, "nonce_account_keypair").unwrap();
    let lamports = amount_of(matches, "amount", "unit").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::CreateNonceAccount {
            nonce_account: nonce_account.into(),
            lamports,
        },
        require_keypair: true,
    })
}

pub fn parse_get_nonce(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let nonce_account_pubkey = pubkey_of(matches, "nonce_account_pubkey").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::GetNonce(nonce_account_pubkey),
        require_keypair: false,
    })
}

pub fn parse_new_nonce(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let nonce_account = keypair_of(matches, "nonce_account_keypair").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::NewNonce(nonce_account.into()),
        require_keypair: true,
    })
}

pub fn parse_show_nonce_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let nonce_account_pubkey = pubkey_of(matches, "nonce_account_pubkey").unwrap();
    let use_lamports_unit = matches.is_present("lamports");

    Ok(CliCommandInfo {
        command: CliCommand::ShowNonceAccount {
            nonce_account_pubkey,
            use_lamports_unit,
        },
        require_keypair: false,
    })
}

pub fn parse_withdraw_from_nonce_account(
    matches: &ArgMatches<'_>,
) -> Result<CliCommandInfo, CliError> {
    let nonce_account = keypair_of(matches, "nonce_account_keypair").unwrap();
    let destination_account_pubkey = pubkey_of(matches, "destination_account_pubkey").unwrap();
    let lamports = amount_of(matches, "amount", "unit").unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::WithdrawFromNonceAccount {
            nonce_account: nonce_account.into(),
            destination_account_pubkey,
            lamports,
        },
        require_keypair: true,
    })
}

pub fn process_create_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Keypair,
    lamports: u64,
) -> ProcessResult {
    let nonce_account_pubkey = nonce_account.pubkey();
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&nonce_account_pubkey, "nonce_account_pubkey".to_string()),
    )?;

    if rpc_client.get_account(&nonce_account_pubkey).is_ok() {
        return Err(CliError::BadParameter(format!(
            "Unable to create nonce account. Nonce account already exists: {}",
            nonce_account_pubkey
        ))
        .into());
    }

    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(NonceState::size())?;
    if lamports < minimum_balance {
        return Err(CliError::BadParameter(format!(
            "need at least {} lamports for nonce account to be rent exempt, provided lamports: {}",
            minimum_balance, lamports
        ))
        .into());
    }

    let ixs = create_nonce_account(&config.keypair.pubkey(), &nonce_account_pubkey, lamports);
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &[&config.keypair, nonce_account],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, nonce_account]);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_get_nonce(rpc_client: &RpcClient, nonce_account_pubkey: &Pubkey) -> ProcessResult {
    let nonce_account = rpc_client.get_account(nonce_account_pubkey)?;
    if nonce_account.owner != nonce_program::id() {
        return Err(CliError::RpcRequestError(
            format!("{:?} is not a nonce account", nonce_account_pubkey).to_string(),
        )
        .into());
    }
    match nonce_account.state() {
        Ok(NonceState::Uninitialized) => Ok("Nonce account is uninitialized".to_string()),
        Ok(NonceState::Initialized(_, hash)) => Ok(format!("{:?}", hash)),
        Err(err) => Err(CliError::RpcRequestError(format!(
            "Account data could not be deserialized to nonce state: {:?}",
            err
        ))
        .into()),
    }
}

pub fn process_new_nonce(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Keypair,
) -> ProcessResult {
    let nonce_account_pubkey = nonce_account.pubkey();
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&nonce_account_pubkey, "nonce_account_pubkey".to_string()),
    )?;

    if rpc_client.get_account(&nonce_account_pubkey).is_err() {
        return Err(CliError::BadParameter(
            "Unable to create new nonce, no nonce account found".to_string(),
        )
        .into());
    }

    let ix = nonce(&nonce_account_pubkey);
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_with_payer(
        vec![ix],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, nonce_account],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, nonce_account]);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_show_nonce_account(
    rpc_client: &RpcClient,
    nonce_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
) -> ProcessResult {
    let nonce_account = rpc_client.get_account(nonce_account_pubkey)?;
    if nonce_account.owner != nonce_program::id() {
        return Err(CliError::RpcRequestError(
            format!("{:?} is not a nonce account", nonce_account_pubkey).to_string(),
        )
        .into());
    }
    let print_account = |hash: Option<Hash>| {
        println!(
            "balance: {}",
            build_balance_message(nonce_account.lamports, use_lamports_unit, true)
        );
        println!(
            "minimum balance required: {}",
            build_balance_message(
                rpc_client.get_minimum_balance_for_rent_exemption(NonceState::size())?,
                use_lamports_unit,
                true
            )
        );
        match hash {
            Some(hash) => println!("nonce: {}", hash),
            None => println!("nonce: uninitialized"),
        }
        Ok("".to_string())
    };
    match nonce_account.state() {
        Ok(NonceState::Uninitialized) => print_account(None),
        Ok(NonceState::Initialized(_, hash)) => print_account(Some(hash)),
        Err(err) => Err(CliError::RpcRequestError(format!(
            "Account data could not be deserialized to nonce state: {:?}",
            err
        ))
        .into()),
    }
}

pub fn process_withdraw_from_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Keypair,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let ix = withdraw(
        &nonce_account.pubkey(),
        destination_account_pubkey,
        lamports,
    );
    let mut tx = Transaction::new_signed_with_payer(
        vec![ix],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, nonce_account],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, nonce_account]);
    log_instruction_custom_error::<NonceError>(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::signature::{read_keypair_file, write_keypair};
    use tempfile::NamedTempFile;

    fn make_tmp_file() -> (String, NamedTempFile) {
        let tmp_file = NamedTempFile::new().unwrap();
        (String::from(tmp_file.path().to_str().unwrap()), tmp_file)
    }

    #[test]
    fn test_parse_command() {
        let test_commands = app("test", "desc", "version");
        let (keypair_file, mut tmp_file) = make_tmp_file();
        let nonce_account_keypair = Keypair::new();
        write_keypair(&nonce_account_keypair, tmp_file.as_file_mut()).unwrap();
        let nonce_account_pubkey = nonce_account_keypair.pubkey();
        let nonce_account_string = nonce_account_pubkey.to_string();

        // Test CreateNonceAccount SubCommand
        let test_create_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-nonce-account",
            &keypair_file,
            "50",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&test_create_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().into(),
                    lamports: 50
                },
                require_keypair: true
            }
        );

        // Test GetNonce Subcommand
        let test_get_nonce = test_commands.clone().get_matches_from(vec![
            "test",
            "get-nonce",
            &nonce_account_string,
        ]);
        assert_eq!(
            parse_command(&test_get_nonce).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetNonce(nonce_account_keypair.pubkey(),),
                require_keypair: false
            }
        );

        // Test NewNonce SubCommand
        let test_new_nonce =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "new-nonce", &keypair_file]);
        assert_eq!(
            parse_command(&test_new_nonce).unwrap(),
            CliCommandInfo {
                command: CliCommand::NewNonce(read_keypair_file(&keypair_file).unwrap().into()),
                require_keypair: true
            }
        );

        // Test ShowNonceAccount Subcommand
        let test_show_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "show-nonce-account",
            &nonce_account_string,
        ]);
        assert_eq!(
            parse_command(&test_show_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::ShowNonceAccount {
                    nonce_account_pubkey: nonce_account_keypair.pubkey(),
                    use_lamports_unit: false,
                },
                require_keypair: false
            }
        );

        // Test WithdrawFromNonceAccount Subcommand
        let test_withdraw_from_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-nonce-account",
            &keypair_file,
            &nonce_account_string,
            "42",
            "lamports",
        ]);
        assert_eq!(
            parse_command(&test_withdraw_from_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().into(),
                    destination_account_pubkey: nonce_account_pubkey,
                    lamports: 42
                },
                require_keypair: true
            }
        );

        let test_withdraw_from_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-nonce-account",
            &keypair_file,
            &nonce_account_string,
            "42",
            "SOL",
        ]);
        assert_eq!(
            parse_command(&test_withdraw_from_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().into(),
                    destination_account_pubkey: nonce_account_pubkey,
                    lamports: 42000000000
                },
                require_keypair: true
            }
        );
    }
}
