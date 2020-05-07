use crate::{
    cli::{
        check_account_for_fee, check_unique_pubkeys, generate_unique_signers,
        log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError,
        ProcessResult, SignerIndex,
    },
    cli_output::CliNonceAccount,
};
use clap::{App, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{
    input_parsers::*, input_validators::*, offline::BLOCKHASH_ARG, ArgConstant,
};
use solana_client::rpc_client::RpcClient;
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{
    account::Account,
    account_utils::StateMut,
    hash::Hash,
    message::Message,
    nonce::{
        self,
        state::{Data, Versions},
        State,
    },
    pubkey::Pubkey,
    system_instruction::{
        advance_nonce_account, authorize_nonce_account, create_nonce_account,
        create_nonce_account_with_seed, withdraw_nonce_account, NonceError, SystemError,
    },
    system_program,
    transaction::Transaction,
};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum CliNonceError {
    #[error("invalid account owner")]
    InvalidAccountOwner,
    #[error("invalid account data")]
    InvalidAccountData,
    #[error("unexpected account data size")]
    UnexpectedDataSize,
    #[error("query hash does not match stored hash")]
    InvalidHash,
    #[error("query authority does not match account authority")]
    InvalidAuthority,
    #[error("invalid state for requested operation")]
    InvalidStateForOperation,
    #[error("client error: {0}")]
    Client(String),
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
        .validator(is_valid_pubkey)
        .help(NONCE_ARG.help)
}

pub fn nonce_authority_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(NONCE_AUTHORITY_ARG.name)
        .long(NONCE_AUTHORITY_ARG.long)
        .takes_value(true)
        .value_name("KEYPAIR")
        .validator(is_valid_signer)
        .help(NONCE_AUTHORITY_ARG.help)
}

impl NonceSubCommands for App<'_, '_> {
    fn nonce_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("authorize-nonce-account")
                .about("Assign account authority to a new entity")
                .arg(
                    pubkey!(Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Address of the nonce account. "),
                )
                .arg(
                    pubkey!(Arg::with_name("new_authority")
                        .index(2)
                        .value_name("AUTHORITY_PUBKEY")
                        .required(true),
                        "Account to be granted authority of the nonce account. "),
                )
                .arg(nonce_authority_arg()),
        )
        .subcommand(
            SubCommand::with_name("create-nonce-account")
                .about("Create a nonce account")
                .arg(
                    Arg::with_name("nonce_account_keypair")
                        .index(1)
                        .value_name("ACCOUNT_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("Keypair of the nonce account to fund"),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_amount)
                        .help("The amount to load the nonce account with, in SOL"),
                )
                .arg(
                    pubkey!(Arg::with_name(NONCE_AUTHORITY_ARG.name)
                        .long(NONCE_AUTHORITY_ARG.long)
                        .value_name("PUBKEY"),
                        "Assign noncing authority to another entity. "),
                )
                .arg(
                    Arg::with_name("seed")
                        .long("seed")
                        .value_name("STRING")
                        .takes_value(true)
                        .help("Seed for address generation; if specified, the resulting account will be at a derived address of the NONCE_ACCOUNT pubkey")
                ),
        )
        .subcommand(
            SubCommand::with_name("nonce")
                .about("Get the current nonce value")
                .alias("get-nonce")
                .arg(
                    pubkey!(Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Address of the nonce account to display. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("new-nonce")
                .about("Generate a new nonce, rendering the existing nonce useless")
                .arg(
                    pubkey!(Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Address of the nonce account. "),
                )
                .arg(nonce_authority_arg()),
        )
        .subcommand(
            SubCommand::with_name("nonce-account")
                .about("Show the contents of a nonce account")
                .alias("show-nonce-account")
                .arg(
                    pubkey!(Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Address of the nonce account to display. "),
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
                .about("Withdraw SOL from the nonce account")
                .arg(
                    pubkey!(Arg::with_name("nonce_account_pubkey")
                        .index(1)
                        .value_name("NONCE_ACCOUNT_ADDRESS")
                        .required(true),
                        "Nonce account to withdraw from. "),
                )
                .arg(
                    pubkey!(Arg::with_name("destination_account_pubkey")
                        .index(2)
                        .value_name("RECIPIENT_ADDRESS")
                        .required(true),
                        "The account to which the SOL should be transferred. "),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(3)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .validator(is_amount)
                        .help("The amount to withdraw from the nonce account, in SOL"),
                )
                .arg(nonce_authority_arg()),
        )
    }
}

pub fn get_account(
    rpc_client: &RpcClient,
    nonce_pubkey: &Pubkey,
) -> Result<Account, CliNonceError> {
    rpc_client
        .get_account(nonce_pubkey)
        .map_err(|e| CliNonceError::Client(format!("{}", e)))
        .and_then(|a| match account_identity_ok(&a) {
            Ok(()) => Ok(a),
            Err(e) => Err(e),
        })
}

pub fn account_identity_ok(account: &Account) -> Result<(), CliNonceError> {
    if account.owner != system_program::id() {
        Err(CliNonceError::InvalidAccountOwner)
    } else if account.data.is_empty() {
        Err(CliNonceError::UnexpectedDataSize)
    } else {
        Ok(())
    }
}

pub fn state_from_account(account: &Account) -> Result<State, CliNonceError> {
    account_identity_ok(account)?;
    StateMut::<Versions>::state(account)
        .map_err(|_| CliNonceError::InvalidAccountData)
        .map(|v| v.convert_to_current())
}

pub fn data_from_account(account: &Account) -> Result<Data, CliNonceError> {
    account_identity_ok(account)?;
    state_from_account(account).and_then(|ref s| data_from_state(s).map(|d| d.clone()))
}

pub fn data_from_state(state: &State) -> Result<&Data, CliNonceError> {
    match state {
        State::Uninitialized => Err(CliNonceError::InvalidStateForOperation),
        State::Initialized(data) => Ok(data),
    }
}

pub fn parse_authorize_nonce_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let nonce_account = pubkey_of_signer(matches, "nonce_account_pubkey", wallet_manager)?.unwrap();
    let new_authority = pubkey_of_signer(matches, "new_authority", wallet_manager)?.unwrap();
    let (nonce_authority, nonce_authority_pubkey) =
        signer_of(matches, NONCE_AUTHORITY_ARG.name, wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, nonce_authority],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::AuthorizeNonceAccount {
            nonce_account,
            nonce_authority: signer_info.index_of(nonce_authority_pubkey).unwrap(),
            new_authority,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_nonce_create_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let (nonce_account, nonce_account_pubkey) =
        signer_of(matches, "nonce_account_keypair", wallet_manager)?;
    let seed = matches.value_of("seed").map(|s| s.to_string());
    let lamports = lamports_of_sol(matches, "amount").unwrap();
    let nonce_authority = pubkey_of_signer(matches, NONCE_AUTHORITY_ARG.name, wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, nonce_account],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::CreateNonceAccount {
            nonce_account: signer_info.index_of(nonce_account_pubkey).unwrap(),
            seed,
            nonce_authority,
            lamports,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_get_nonce(
    matches: &ArgMatches<'_>,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let nonce_account_pubkey =
        pubkey_of_signer(matches, "nonce_account_pubkey", wallet_manager)?.unwrap();

    Ok(CliCommandInfo {
        command: CliCommand::GetNonce(nonce_account_pubkey),
        signers: vec![],
    })
}

pub fn parse_new_nonce(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let nonce_account = pubkey_of_signer(matches, "nonce_account_pubkey", wallet_manager)?.unwrap();
    let (nonce_authority, nonce_authority_pubkey) =
        signer_of(matches, NONCE_AUTHORITY_ARG.name, wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, nonce_authority],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::NewNonce {
            nonce_account,
            nonce_authority: signer_info.index_of(nonce_authority_pubkey).unwrap(),
        },
        signers: signer_info.signers,
    })
}

pub fn parse_show_nonce_account(
    matches: &ArgMatches<'_>,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let nonce_account_pubkey =
        pubkey_of_signer(matches, "nonce_account_pubkey", wallet_manager)?.unwrap();
    let use_lamports_unit = matches.is_present("lamports");

    Ok(CliCommandInfo {
        command: CliCommand::ShowNonceAccount {
            nonce_account_pubkey,
            use_lamports_unit,
        },
        signers: vec![],
    })
}

pub fn parse_withdraw_from_nonce_account(
    matches: &ArgMatches<'_>,
    default_signer_path: &str,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let nonce_account = pubkey_of_signer(matches, "nonce_account_pubkey", wallet_manager)?.unwrap();
    let destination_account_pubkey =
        pubkey_of_signer(matches, "destination_account_pubkey", wallet_manager)?.unwrap();
    let lamports = lamports_of_sol(matches, "amount").unwrap();
    let (nonce_authority, nonce_authority_pubkey) =
        signer_of(matches, NONCE_AUTHORITY_ARG.name, wallet_manager)?;

    let payer_provided = None;
    let signer_info = generate_unique_signers(
        vec![payer_provided, nonce_authority],
        matches,
        default_signer_path,
        wallet_manager,
    )?;

    Ok(CliCommandInfo {
        command: CliCommand::WithdrawFromNonceAccount {
            nonce_account,
            nonce_authority: signer_info.index_of(nonce_authority_pubkey).unwrap(),
            destination_account_pubkey,
            lamports,
        },
        signers: signer_info.signers,
    })
}

/// Check if a nonce account is initialized with the given authority and hash
pub fn check_nonce_account(
    nonce_account: &Account,
    nonce_authority: &Pubkey,
    nonce_hash: &Hash,
) -> Result<(), CliError> {
    match state_from_account(nonce_account)? {
        State::Initialized(ref data) => {
            if &data.blockhash != nonce_hash {
                Err(CliNonceError::InvalidHash.into())
            } else if nonce_authority != &data.authority {
                Err(CliNonceError::InvalidAuthority.into())
            } else {
                Ok(())
            }
        }
        State::Uninitialized => Err(CliNonceError::InvalidStateForOperation.into()),
    }
}

pub fn process_authorize_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Pubkey,
    nonce_authority: SignerIndex,
    new_authority: &Pubkey,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let nonce_authority = config.signers[nonce_authority];
    let ix = authorize_nonce_account(nonce_account, &nonce_authority.pubkey(), new_authority);
    let message = Message::new_with_payer(&[ix], Some(&config.signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;

    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &config.signers);
    log_instruction_custom_error::<NonceError>(result, &config)
}

pub fn process_create_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: SignerIndex,
    seed: Option<String>,
    nonce_authority: Option<Pubkey>,
    lamports: u64,
) -> ProcessResult {
    let nonce_account_pubkey = config.signers[nonce_account].pubkey();
    let nonce_account_address = if let Some(ref seed) = seed {
        Pubkey::create_with_seed(&nonce_account_pubkey, seed, &system_program::id())?
    } else {
        nonce_account_pubkey
    };

    check_unique_pubkeys(
        (&config.signers[0].pubkey(), "cli keypair".to_string()),
        (&nonce_account_address, "nonce_account".to_string()),
    )?;

    if let Ok(nonce_account) = get_account(rpc_client, &nonce_account_address) {
        let err_msg = if state_from_account(&nonce_account).is_ok() {
            format!("Nonce account {} already exists", nonce_account_address)
        } else {
            format!(
                "Account {} already exists and is not a nonce account",
                nonce_account_address
            )
        };
        return Err(CliError::BadParameter(err_msg).into());
    }

    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(State::size())?;
    if lamports < minimum_balance {
        return Err(CliError::BadParameter(format!(
            "need at least {} lamports for nonce account to be rent exempt, provided lamports: {}",
            minimum_balance, lamports
        ))
        .into());
    }

    let nonce_authority = nonce_authority.unwrap_or_else(|| config.signers[0].pubkey());

    let ixs = if let Some(seed) = seed {
        create_nonce_account_with_seed(
            &config.signers[0].pubkey(), // from
            &nonce_account_address,      // to
            &nonce_account_pubkey,       // base
            &seed,                       // seed
            &nonce_authority,
            lamports,
        )
    } else {
        create_nonce_account(
            &config.signers[0].pubkey(),
            &nonce_account_pubkey,
            &nonce_authority,
            lamports,
        )
    };

    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let message = Message::new_with_payer(&ixs, Some(&config.signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;

    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &config.signers);
    log_instruction_custom_error::<SystemError>(result, &config)
}

pub fn process_get_nonce(rpc_client: &RpcClient, nonce_account_pubkey: &Pubkey) -> ProcessResult {
    match get_account(rpc_client, nonce_account_pubkey).and_then(|ref a| state_from_account(a))? {
        State::Uninitialized => Ok("Nonce account is uninitialized".to_string()),
        State::Initialized(ref data) => Ok(format!("{:?}", data.blockhash)),
    }
}

pub fn process_new_nonce(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Pubkey,
    nonce_authority: SignerIndex,
) -> ProcessResult {
    check_unique_pubkeys(
        (&config.signers[0].pubkey(), "cli keypair".to_string()),
        (&nonce_account, "nonce_account_pubkey".to_string()),
    )?;

    if rpc_client.get_account(&nonce_account).is_err() {
        return Err(CliError::BadParameter(
            "Unable to create new nonce, no nonce account found".to_string(),
        )
        .into());
    }

    let nonce_authority = config.signers[nonce_authority];
    let ix = advance_nonce_account(&nonce_account, &nonce_authority.pubkey());
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;
    let message = Message::new_with_payer(&[ix], Some(&config.signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client
        .send_and_confirm_transaction_with_spinner(&mut tx, &[config.signers[0], nonce_authority]);
    log_instruction_custom_error::<SystemError>(result, &config)
}

pub fn process_show_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account_pubkey: &Pubkey,
    use_lamports_unit: bool,
) -> ProcessResult {
    let nonce_account = get_account(rpc_client, nonce_account_pubkey)?;
    let print_account = |data: Option<&nonce::state::Data>| {
        let mut nonce_account = CliNonceAccount {
            balance: nonce_account.lamports,
            minimum_balance_for_rent_exemption: rpc_client
                .get_minimum_balance_for_rent_exemption(State::size())?,
            use_lamports_unit,
            ..CliNonceAccount::default()
        };
        if let Some(ref data) = data {
            nonce_account.nonce = Some(data.blockhash.to_string());
            nonce_account.lamports_per_signature = Some(data.fee_calculator.lamports_per_signature);
            nonce_account.authority = Some(data.authority.to_string());
        }

        Ok(config.output_format.formatted_string(&nonce_account))
    };
    match state_from_account(&nonce_account)? {
        State::Uninitialized => print_account(None),
        State::Initialized(ref data) => print_account(Some(data)),
    }
}

pub fn process_withdraw_from_nonce_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    nonce_account: &Pubkey,
    nonce_authority: SignerIndex,
    destination_account_pubkey: &Pubkey,
    lamports: u64,
) -> ProcessResult {
    let (recent_blockhash, fee_calculator) = rpc_client.get_recent_blockhash()?;

    let nonce_authority = config.signers[nonce_authority];
    let ix = withdraw_nonce_account(
        nonce_account,
        &nonce_authority.pubkey(),
        destination_account_pubkey,
        lamports,
    );
    let message = Message::new_with_payer(&[ix], Some(&config.signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.try_sign(&config.signers, recent_blockhash)?;
    check_account_for_fee(
        rpc_client,
        &config.signers[0].pubkey(),
        &fee_calculator,
        &tx.message,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner(&mut tx, &config.signers);
    log_instruction_custom_error::<NonceError>(result, &config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command};
    use solana_sdk::{
        account::Account,
        fee_calculator::FeeCalculator,
        hash::hash,
        nonce::{self, State},
        signature::{read_keypair_file, write_keypair, Keypair, Signer},
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
        let default_keypair = Keypair::new();
        let (default_keypair_file, mut tmp_file) = make_tmp_file();
        write_keypair(&default_keypair, tmp_file.as_file_mut()).unwrap();
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
            parse_command(
                &test_authorize_nonce_account,
                &default_keypair_file,
                &mut None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::AuthorizeNonceAccount {
                    nonce_account: nonce_account_pubkey,
                    nonce_authority: 0,
                    new_authority: Pubkey::default(),
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
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
            parse_command(
                &test_authorize_nonce_account,
                &default_keypair_file,
                &mut None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::AuthorizeNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: 1,
                    new_authority: Pubkey::default(),
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into()
                ],
            }
        );

        // Test CreateNonceAccount SubCommand
        let test_create_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-nonce-account",
            &keypair_file,
            "50",
        ]);
        assert_eq!(
            parse_command(&test_create_nonce_account, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateNonceAccount {
                    nonce_account: 1,
                    seed: None,
                    nonce_authority: None,
                    lamports: 50_000_000_000,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&keypair_file).unwrap().into()
                ],
            }
        );

        // Test CreateNonceAccount SubCommand with authority
        let test_create_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "create-nonce-account",
            &keypair_file,
            "50",
            "--nonce-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_create_nonce_account, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateNonceAccount {
                    nonce_account: 1,
                    seed: None,
                    nonce_authority: Some(nonce_authority_keypair.pubkey()),
                    lamports: 50_000_000_000,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&keypair_file).unwrap().into()
                ],
            }
        );

        // Test GetNonce Subcommand
        let test_get_nonce = test_commands.clone().get_matches_from(vec![
            "test",
            "get-nonce",
            &nonce_account_string,
        ]);
        assert_eq!(
            parse_command(&test_get_nonce, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::GetNonce(nonce_account_keypair.pubkey()),
                signers: vec![],
            }
        );

        // Test NewNonce SubCommand
        let test_new_nonce =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "new-nonce", &keypair_file]);
        let nonce_account = read_keypair_file(&keypair_file).unwrap();
        assert_eq!(
            parse_command(&test_new_nonce, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::NewNonce {
                    nonce_account: nonce_account.pubkey(),
                    nonce_authority: 0,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
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
            parse_command(&test_new_nonce, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::NewNonce {
                    nonce_account: nonce_account.pubkey(),
                    nonce_authority: 1,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into()
                ],
            }
        );

        // Test ShowNonceAccount Subcommand
        let test_show_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "nonce-account",
            &nonce_account_string,
        ]);
        assert_eq!(
            parse_command(&test_show_nonce_account, &default_keypair_file, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ShowNonceAccount {
                    nonce_account_pubkey: nonce_account_keypair.pubkey(),
                    use_lamports_unit: false,
                },
                signers: vec![],
            }
        );

        // Test WithdrawFromNonceAccount Subcommand
        let test_withdraw_from_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-nonce-account",
            &keypair_file,
            &nonce_account_string,
            "42",
        ]);
        assert_eq!(
            parse_command(
                &test_withdraw_from_nonce_account,
                &default_keypair_file,
                &mut None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: 0,
                    destination_account_pubkey: nonce_account_pubkey,
                    lamports: 42_000_000_000
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        // Test WithdrawFromNonceAccount Subcommand with authority
        let test_withdraw_from_nonce_account = test_commands.clone().get_matches_from(vec![
            "test",
            "withdraw-from-nonce-account",
            &keypair_file,
            &nonce_account_string,
            "42",
            "--nonce-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(
                &test_withdraw_from_nonce_account,
                &default_keypair_file,
                &mut None
            )
            .unwrap(),
            CliCommandInfo {
                command: CliCommand::WithdrawFromNonceAccount {
                    nonce_account: read_keypair_file(&keypair_file).unwrap().pubkey(),
                    nonce_authority: 1,
                    destination_account_pubkey: nonce_account_pubkey,
                    lamports: 42_000_000_000
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into()
                ],
            }
        );
    }

    #[test]
    fn test_check_nonce_account() {
        let blockhash = Hash::default();
        let nonce_pubkey = Pubkey::new_rand();
        let data = Versions::new_current(State::Initialized(nonce::state::Data {
            authority: nonce_pubkey,
            blockhash,
            fee_calculator: FeeCalculator::default(),
        }));
        let valid = Account::new_data(1, &data, &system_program::ID);
        assert!(check_nonce_account(&valid.unwrap(), &nonce_pubkey, &blockhash).is_ok());

        let invalid_owner = Account::new_data(1, &data, &Pubkey::new(&[1u8; 32]));
        assert_eq!(
            check_nonce_account(&invalid_owner.unwrap(), &nonce_pubkey, &blockhash),
            Err(CliNonceError::InvalidAccountOwner.into()),
        );

        let invalid_data = Account::new_data(1, &"invalid", &system_program::ID);
        assert_eq!(
            check_nonce_account(&invalid_data.unwrap(), &nonce_pubkey, &blockhash),
            Err(CliNonceError::InvalidAccountData.into()),
        );

        let data = Versions::new_current(State::Initialized(nonce::state::Data {
            authority: nonce_pubkey,
            blockhash: hash(b"invalid"),
            fee_calculator: FeeCalculator::default(),
        }));
        let invalid_hash = Account::new_data(1, &data, &system_program::ID);
        assert_eq!(
            check_nonce_account(&invalid_hash.unwrap(), &nonce_pubkey, &blockhash),
            Err(CliNonceError::InvalidHash.into()),
        );

        let data = Versions::new_current(State::Initialized(nonce::state::Data {
            authority: Pubkey::new_rand(),
            blockhash,
            fee_calculator: FeeCalculator::default(),
        }));
        let invalid_authority = Account::new_data(1, &data, &system_program::ID);
        assert_eq!(
            check_nonce_account(&invalid_authority.unwrap(), &nonce_pubkey, &blockhash),
            Err(CliNonceError::InvalidAuthority.into()),
        );

        let data = Versions::new_current(State::Uninitialized);
        let invalid_state = Account::new_data(1, &data, &system_program::ID);
        assert_eq!(
            check_nonce_account(&invalid_state.unwrap(), &nonce_pubkey, &blockhash),
            Err(CliNonceError::InvalidStateForOperation.into()),
        );
    }

    #[test]
    fn test_account_identity_ok() {
        let nonce_account = nonce::create_account(1).into_inner();
        assert_eq!(account_identity_ok(&nonce_account), Ok(()));

        let system_account = Account::new(1, 0, &system_program::id());
        assert_eq!(
            account_identity_ok(&system_account),
            Err(CliNonceError::UnexpectedDataSize),
        );

        let other_program = Pubkey::new(&[1u8; 32]);
        let other_account_no_data = Account::new(1, 0, &other_program);
        assert_eq!(
            account_identity_ok(&other_account_no_data),
            Err(CliNonceError::InvalidAccountOwner),
        );
    }

    #[test]
    fn test_state_from_account() {
        let mut nonce_account = nonce::create_account(1).into_inner();
        assert_eq!(state_from_account(&nonce_account), Ok(State::Uninitialized));

        let data = nonce::state::Data {
            authority: Pubkey::new(&[1u8; 32]),
            blockhash: Hash::new(&[42u8; 32]),
            fee_calculator: FeeCalculator::new(42),
        };
        nonce_account
            .set_state(&Versions::new_current(State::Initialized(data.clone())))
            .unwrap();
        assert_eq!(
            state_from_account(&nonce_account),
            Ok(State::Initialized(data))
        );

        let wrong_data_size_account = Account::new(1, 1, &system_program::id());
        assert_eq!(
            state_from_account(&wrong_data_size_account),
            Err(CliNonceError::InvalidAccountData),
        );
    }

    #[test]
    fn test_data_from_helpers() {
        let mut nonce_account = nonce::create_account(1).into_inner();
        let state = state_from_account(&nonce_account).unwrap();
        assert_eq!(
            data_from_state(&state),
            Err(CliNonceError::InvalidStateForOperation)
        );
        assert_eq!(
            data_from_account(&nonce_account),
            Err(CliNonceError::InvalidStateForOperation)
        );

        let data = nonce::state::Data {
            authority: Pubkey::new(&[1u8; 32]),
            blockhash: Hash::new(&[42u8; 32]),
            fee_calculator: FeeCalculator::new(42),
        };
        nonce_account
            .set_state(&Versions::new_current(State::Initialized(data.clone())))
            .unwrap();
        let state = state_from_account(&nonce_account).unwrap();
        assert_eq!(data_from_state(&state), Ok(&data));
        assert_eq!(data_from_account(&nonce_account), Ok(data));
    }
}
