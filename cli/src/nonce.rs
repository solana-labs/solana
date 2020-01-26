use crate::cli::{
    build_balance_message, check_account_for_fee, check_unique_pubkeys,
    log_instruction_custom_error, required_lamports_from, CliCommand, CliCommandInfo, CliConfig,
    CliError, ProcessResult, SigningAuthority,
};
use crate::offline::BLOCKHASH_ARG;
use clap::{App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{input_parsers::*, input_validators::*, ArgConstant};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    account::Account,
    account_utils::StateMut,
    hash::Hash,
    nonce_state::{Meta, NonceState},
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction::{
        advance_nonce_account, authorize_nonce_account, create_address_with_seed,
        create_nonce_account, create_nonce_account_with_seed, withdraw_nonce_account, NonceError,
        SystemError,
    },
    system_program,
    transaction::Transaction,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CliNonceError {
    InvalidAccountOwner,
    InvalidAccountData,
    InvalidHash,
    InvalidAuthority,
    InvalidState,
}

pub const NONCE_ARG: ArgConstant<'static> = ArgConstant {
    name: "nonce",
    long: "nonce",
    help: "Provide the nonce account to use when creating a nonced \n\
           transaction. Nonced transactions are useful when a transaction \n\
           requires a lengthy signing process. Learn more about nonced \n\
           transactions at https://docs.solana.com/offline-signing/durable-nonce",
};

pub const NONCE_AUTHORITY_ARG: ArgConstant<'static> = ArgConstant {
    name: "nonce_authority",
    long: "nonce-authority",
    help: "Provide the nonce authority keypair to use when signing a nonced transaction",
};

pub trait NonceSubCommands {
    fn nonce_subcommands(self) -> Self;
}

pub fn nonce_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(NONCE_ARG.name)
        .long(NONCE_ARG.long)
        .takes_value(true)
        .value_name("PUBKEY")
        .requires(BLOCKHASH_ARG.name)
        .validator(is_pubkey)
        .help(NONCE_ARG.help)
}

pub fn nonce_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(NONCE_AUTHORITY_ARG.name)
        .long(NONCE_AUTHORITY_ARG.long)
        .takes_value(true)
        .value_name("KEYPAIR or PUBKEY")
        .validator(is_pubkey_or_keypair_or_ask_keyword)
        .help(NONCE_AUTHORITY_ARG.help)
}

impl NonceSubCommands for App<'_, '_> {
    fn nonce_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("authorize-nonce-account")
                .about("Assign account authority to a new entity")
                .arg(
                    Arg::with_name("nonce_account_keypair")
                        .index(1)
                        .value_name("NONCE_ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Address of the nonce account"),
                )
                .arg(
                    Arg::with_name("new_authority")
                        .index(2)
                        .value_name("NEW_AUTHORITY_PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
                        .help("Account to be granted authority of the nonce account"),
                )
                .arg(
                    Arg::with_name("seed")
                        .long("seed")
                        .value_name("SEED STRING")
                        .takes_value(true)
                        .help("Seed for address generation; if specified, the resulting account will be at a derived address of the NONCE_ACCOUNT pubkey")
                )
                .arg(nonce_authority_arg()),
        )
        .subcommand(
            SubCommand::with_name("create-nonce-account")
                .about("Create a nonce account")
                .arg(
                    Arg::with_name("nonce_account_keypair")
                        .index(1)
                        .value_name("NONCE ACCOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_pubkey_or_keypair)
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
                )
                .arg(
                    Arg::with_name(NONCE_AUTHORITY_ARG.name)
                        .long(NONCE_AUTHORITY_ARG.long)
                        .takes_value(true)
                        .value_name("BASE58_PUBKEY")
                        .validator(is_pubkey_or_keypair)
                        .help("Assign noncing authority to another entity"),
                ),
        )
        .subcommand(
            SubCommand::with_name("nonce")
                .about("Get the current nonce value")
                .alias("get-nonce")
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
                )
                .arg(nonce_authority_arg()),
        )
        .subcommand(
            SubCommand::with_name("nonce-account")
                .about("Show the contents of a nonce account")
                .alias("show-nonce-account")
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
                )
                .arg(nonce_authority_arg()),
        )
    }
}

pub fn parse_authorize_nonce_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let nonce_account = pubkey_of(matches, "nonce_account_keypair").unwrap();
    let new_authority = pubkey_of(matches, "new_authority").unwrap();
    let nonce_authority = if matches.is_present(NONCE_AUTHORITY_ARG.name) {
        Some(SigningAuthority::new_from_matches(
            &matches,
            NONCE_AUTHORITY_ARG.name,
            None,
        )?)
    } else {
        None
    };

    Ok(CliCommandInfo {
        command: CliCommand::AuthorizeNonceAccount {
            nonce_account,
            nonce_authority,
            new_authority,
        },
        require_keypair: true,
    })
}

pub fn parse_nonce_create_account(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let nonce_account = keypair_of(matches, "nonce_account_keypair").unwrap();
    let seed = matches.value_of("seed").map(|s| s.to_string());
    let lamports = required_lamports_from(matches, "amount", "unit")?;
    let nonce_authority = pubkey_of(matches, NONCE_AUTHORITY_ARG.name);

    Ok(CliCommandInfo {
        command: CliCommand::CreateNonceAccount {
            nonce_account: nonce_account.into(),
            seed,
            nonce_authority,
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
    let nonce_account = pubkey_of(matches, "nonce_account_keypair").unwrap();
    let nonce_authority = if matches.is_present(NONCE_AUTHORITY_ARG.name) {
        Some(SigningAuthority::new_from_matches(
            &matches,
            NONCE_AUTHORITY_ARG.name,
            None,
        )?)
    } else {
        None
    };

    Ok(CliCommandInfo {
        command: CliCommand::NewNonce {
            nonce_account,
            nonce_authority,
        },
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
    let nonce_account = pubkey_of(matches, "nonce_account_keypair").unwrap();
    let destination_account_pubkey = pubkey_of(matches, "destination_account_pubkey").unwrap();
    let lamports = required_lamports_from(matches, "amount", "unit")?;
    let nonce_authority = if matches.is_present(NONCE_AUTHORITY_ARG.name) {
        Some(SigningAuthority::new_from_matches(
            &matches,
            NONCE_AUTHORITY_ARG.name,
            None,
        )?)
    } else {
        None
    };

    Ok(CliCommandInfo {
        command: CliCommand::WithdrawFromNonceAccount {
            nonce_account,
            nonce_authority,
            destination_account_pubkey,
            lamports,
        },
        require_keypair: true,
    })
}

/// Check if a nonce account is initialized with the given authority and hash
pub fn check_nonce_account(
    nonce_account: &Account,
    nonce_authority: &Pubkey,
    nonce_hash: &Hash,
) -> Result<(), Box<CliError>> {
    if nonce_account.owner != system_program::ID {
        return Err(CliError::InvalidNonce(CliNonceError::InvalidAccountOwner).into());
    }
    let nonce_state: NonceState = nonce_account
        .state()
        .map_err(|_| Box::new(CliError::InvalidNonce(CliNonceError::InvalidAccountData)))?;
    match nonce_state {
        NonceState::Initialized(meta, hash) => {
            if &hash != nonce_hash {
                Err(CliError::InvalidNonce(CliNonceError::InvalidHash).into())
            } else if nonce_authority != &meta.nonce_authority {
                Err(CliError::InvalidNonce(CliNonceError::InvalidAuthority).into())
            } else {
                Ok(())
            }
        }
        NonceState::Uninitialized => {
            Err(CliError::InvalidNonce(CliNonceError::InvalidState).into())
        }
    }
}

pub fn process_authorize_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Pubkey,
    nonce_authority: Option<&SigningAuthority>,
    new_authority: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let nonce_authority = nonce_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);
    let ix = authorize_nonce_account(nonce_account, &nonce_authority.pubkey(), new_authority);
    let mut tx = Transaction::new_signed_with_payer(
        vec![ix],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, nonce_authority],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, nonce_authority]);
    log_instruction_custom_error::<NonceError>(result)
}

pub fn process_create_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Keypair,
    seed: Option<String>,
    nonce_authority: Option<Pubkey>,
    lamports: u64,
) -> ProcessResult {
    let nonce_account_pubkey = nonce_account.pubkey();
    let nonce_account_address = if let Some(seed) = seed.clone() {
        create_address_with_seed(&nonce_account_pubkey, &seed, &system_program::id())?
    } else {
        nonce_account_pubkey
    };

    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&nonce_account_address, "nonce_account".to_string()),
    )?;

    if let Ok(nonce_account) = rpc_client.get_account(&nonce_account_address) {
        let err_msg = if nonce_account.owner == system_program::id()
            && StateMut::<NonceState>::state(&nonce_account).is_ok()
        {
            format!("Nonce account {} already exists", nonce_account_address)
        } else {
            format!(
                "Account {} already exists and is not a nonce account",
                nonce_account_address
            )
        };
        return Err(CliError::BadParameter(err_msg).into());
    }

    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(NonceState::size())?;
    if lamports < minimum_balance {
        return Err(CliError::BadParameter(format!(
            "need at least {} lamports for nonce account to be rent exempt, provided lamports: {}",
            minimum_balance, lamports
        ))
        .into());
    }

    let nonce_authority = nonce_authority.unwrap_or_else(|| config.keypair.pubkey());

    let ixs = if let Some(seed) = seed {
        create_nonce_account_with_seed(
            &config.keypair.pubkey(), // from
            &nonce_account_address,   // to
            &nonce_account_pubkey,    // base
            &seed,                    // seed
            &nonce_authority,
            lamports,
        )
    } else {
        create_nonce_account(
            &config.keypair.pubkey(),
            &nonce_account_pubkey,
            &nonce_authority,
            lamports,
        )
    };

    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let signers = if nonce_account_pubkey != config.keypair.pubkey() {
        vec![&config.keypair, nonce_account] // both must sign if `from` and `to` differ
    } else {
        vec![&config.keypair] // when stake_account == config.keypair and there's a seed, we only need one signature
    };

    let mut tx = Transaction::new_signed_with_payer(
        ixs,
        Some(&config.keypair.pubkey()),
        &signers,
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction(&mut tx, &signers);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_get_nonce(rpc_client: &RpcClient, nonce_account_pubkey: &Pubkey) -> ProcessResult {
    let nonce_account = rpc_client.get_account(nonce_account_pubkey)?;
    if nonce_account.owner != system_program::id() {
        return Err(CliError::RpcRequestError(format!(
            "{:?} is not a nonce account",
            nonce_account_pubkey
        ))
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
    nonce_account: &Pubkey,
    nonce_authority: Option<&SigningAuthority>,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.keypair.pubkey(), "cli keypair".to_string()),
        (&nonce_account, "nonce_account_pubkey".to_string()),
    )?;

    if rpc_client.get_account(&nonce_account).is_err() {
        return Err(CliError::BadParameter(
            "Unable to create new nonce, no nonce account found".to_string(),
        )
        .into());
    }

    let nonce_authority = nonce_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);
    let ix = advance_nonce_account(&nonce_account, &nonce_authority.pubkey());
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let mut tx = Transaction::new_signed_with_payer(
        vec![ix],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, nonce_authority],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, nonce_authority]);
    log_instruction_custom_error::<SystemError>(result)
}

pub fn process_show_nonce_account(
    rpc_client: &RpcClient,
    nonce_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
) -> ProcessResult {
    let nonce_account = rpc_client.get_account(nonce_account_pubkey)?;
    if nonce_account.owner != system_program::id() {
        return Err(CliError::RpcRequestError(format!(
            "{:?} is not a nonce account",
            nonce_account_pubkey
        ))
        .into());
    }
    let print_account = |data: Option<(Meta, Hash)>| {
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
        match data {
            Some((meta, hash)) => {
                println!("nonce: {}", hash);
                println!("authority: {}", meta.nonce_authority);
            }
            None => {
                println!("nonce: uninitialized");
                println!("authority: uninitialized");
            }
        }
        Ok("".to_string())
    };
    match nonce_account.state() {
        Ok(NonceState::Uninitialized) => print_account(None),
        Ok(NonceState::Initialized(meta, hash)) => print_account(Some((meta, hash))),
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
    nonce_account: &Pubkey,
    nonce_authority: Option<&SigningAuthority>,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let nonce_authority = nonce_authority
        .map(|a| a.keypair())
        .unwrap_or(&config.keypair);
    let ix = withdraw_nonce_account(
        nonce_account,
        &nonce_authority.pubkey(),
        destination_account_pubkey,
        lamports,
    );
    let mut tx = Transaction::new_signed_with_payer(
        vec![ix],
        Some(&config.keypair.pubkey()),
        &[&config.keypair, nonce_authority],
        recent_blockhash,
    );
    check_account_for_fee(
        rpc_client,
        &config.keypair.pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result =
        rpc_client.send_and_confirm_transaction(&mut tx, &[&config.keypair, nonce_authority]);
    log_instruction_custom_error::<NonceError>(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::{
        account::Account,
        hash::hash,
        nonce_state::{Meta as NonceMeta, NonceState},
        signature::{read_keypair_file, write_keypair},
        system_program,
    };
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

        let (authority_keypair_file, mut tmp_file2) = make_tmp_file();
        let nonce_authority_keypair = Keypair::new();
        write_keypair(&nonce_authority_keypair, tmp_file2.as_file_mut()).unwrap();

        // Test AuthorizeNonceAccount Subcommand
        let test_authorize_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "authorize-nonce-account",
            &keypair_file,
            &Pubkey::default().to_string(),
        ]);
        assert_eq!(
            parse_command(&test_authorize_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::AuthorizeNonceAccount {
                    nonce_account: nonce_account_pubkey,
                    nonce_authority: None,
                    new_authority: Pubkey::default(),
                },
                require_keypair: true,
            }
        );

        // Test AuthorizeNonceAccount Subcommand with authority
        let test_authorize_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "authorize-nonce-account",
            &keypair_file,
            &Pubkey::default().to_string(),
            "--nonce-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_authorize_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::AuthorizeNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: Some(
                        read_keypair_file(&authority_keypair_file).unwrap().into()
                    ),
                    new_authority: Pubkey::default(),
                },
                require_keypair: true,
            }
        );

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
                    seed: None,
                    nonce_authority: None,
                    lamports: 50,
                },
                require_keypair: true
            }
        );

        // Test CreateNonceAccount SubCommand with authority
        let test_create_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-nonce-account",
            &keypair_file,
            "50",
            "lamports",
            "--nonce-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_create_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().into(),
                    seed: None,
                    nonce_authority: Some(
                        read_keypair_file(&authority_keypair_file).unwrap().pubkey()
                    ),
                    lamports: 50,
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
        let nonce_account = read_keypair_file(&keypair_file).unwrap();
        assert_eq!(
            parse_command(&test_new_nonce).unwrap(),
            CliCommandInfo {
                command: CliCommand::NewNonce {
                    nonce_account: nonce_account.pubkey(),
                    nonce_authority: None,
                },
                require_keypair: true
            }
        );

        // Test NewNonce SubCommand with authority
        let test_new_nonce = test_commands.clone().get_matches_from(vec![
            "test",
            "new-nonce",
            &keypair_file,
            "--nonce-authority",
            &authority_keypair_file,
        ]);
        let nonce_account = read_keypair_file(&keypair_file).unwrap();
        assert_eq!(
            parse_command(&test_new_nonce).unwrap(),
            CliCommandInfo {
                command: CliCommand::NewNonce {
                    nonce_account: nonce_account.pubkey(),
                    nonce_authority: Some(
                        read_keypair_file(&authority_keypair_file).unwrap().into()
                    ),
                },
                require_keypair: true
            }
        );

        // Test ShowNonceAccount Subcommand
        let test_show_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "nonce-account",
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
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: None,
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
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: None,
                    destination_account_pubkey: nonce_account_pubkey,
                    lamports: 42000000000
                },
                require_keypair: true
            }
        );

        // Test WithdrawFromNonceAccount Subcommand with authority
        let test_withdraw_from_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-nonce-account",
            &keypair_file,
            &nonce_account_string,
            "42",
            "lamports",
            "--nonce-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_withdraw_from_nonce_account).unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: Some(
                        read_keypair_file(&authority_keypair_file).unwrap().into()
                    ),
                    destination_account_pubkey: nonce_account_pubkey,
                    lamports: 42
                },
                require_keypair: true
            }
        );
    }

    #[test]
    fn test_check_nonce_account() {
        let blockhash = Hash::default();
        let nonce_pubkey = Pubkey::new_rand();
        let valid = Account::new_data(
            1,
            &NonceState::Initialized(NonceMeta::new(&nonce_pubkey), blockhash),
            &system_program::ID,
        );
        assert!(check_nonce_account(&valid.unwrap(), &nonce_pubkey, &blockhash).is_ok());

        let invalid_owner = Account::new_data(
            1,
            &NonceState::Initialized(NonceMeta::new(&nonce_pubkey), blockhash),
            &Pubkey::new(&[1u8; 32]),
        );
        assert_eq!(
            check_nonce_account(&invalid_owner.unwrap(), &nonce_pubkey, &blockhash),
            Err(Box::new(CliError::InvalidNonce(
                CliNonceError::InvalidAccountOwner
            ))),
        );

        let invalid_data = Account::new_data(1, &"invalid", &system_program::ID);
        assert_eq!(
            check_nonce_account(&invalid_data.unwrap(), &nonce_pubkey, &blockhash),
            Err(Box::new(CliError::InvalidNonce(
                CliNonceError::InvalidAccountData
            ))),
        );

        let invalid_hash = Account::new_data(
            1,
            &NonceState::Initialized(NonceMeta::new(&nonce_pubkey), hash(b"invalid")),
            &system_program::ID,
        );
        assert_eq!(
            check_nonce_account(&invalid_hash.unwrap(), &nonce_pubkey, &blockhash),
            Err(Box::new(CliError::InvalidNonce(CliNonceError::InvalidHash))),
        );

        let invalid_authority = Account::new_data(
            1,
            &NonceState::Initialized(NonceMeta::new(&Pubkey::new_rand()), blockhash),
            &system_program::ID,
        );
        assert_eq!(
            check_nonce_account(&invalid_authority.unwrap(), &nonce_pubkey, &blockhash),
            Err(Box::new(CliError::InvalidNonce(
                CliNonceError::InvalidAuthority
            ))),
        );

        let invalid_state = Account::new_data(1, &NonceState::Uninitialized, &system_program::ID);
        assert_eq!(
            check_nonce_account(&invalid_state.unwrap(), &nonce_pubkey, &blockhash),
            Err(Box::new(CliError::InvalidNonce(
                CliNonceError::InvalidState
            ))),
        );
    }
}
