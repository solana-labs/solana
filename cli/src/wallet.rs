use {
    crate::{
        cli::{
            log_instruction_custom_error, request_and_confirm_airdrop, CliCommand, CliCommandInfo,
            CliConfig, CliError, ProcessResult,
        },
        compute_unit_price::WithComputeUnitPrice,
        memo::WithMemo,
        nonce::check_nonce_account,
        spend_utils::{resolve_spend_tx_and_check_account_balances, SpendAmount},
    },
    clap::{value_t_or_exit, App, Arg, ArgMatches, SubCommand},
    solana_clap_utils::{
        compute_unit_price::{compute_unit_price_arg, COMPUTE_UNIT_PRICE_ARG},
        fee_payer::*,
        input_parsers::*,
        input_validators::*,
        keypair::{DefaultSigner, SignerIndex},
        memo::*,
        nonce::*,
        offline::*,
    },
    solana_cli_output::{
        display::{build_balance_message, BuildBalanceMessageConfig},
        return_signers_with_config, CliAccount, CliBalance, CliSignatureVerificationStatus,
        CliTransaction, CliTransactionConfirmation, OutputFormat, ReturnSignersConfig,
    },
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::config::RpcTransactionConfig,
    solana_rpc_client_nonce_utils::blockhash_query::BlockhashQuery,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        message::Message,
        offchain_message::OffchainMessage,
        pubkey::Pubkey,
        signature::Signature,
        stake,
        system_instruction::{self, SystemError},
        system_program,
        transaction::{Transaction, VersionedTransaction},
    },
    solana_transaction_status::{
        EncodableWithMeta, EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction,
        TransactionBinaryEncoding, UiTransactionEncoding,
    },
    std::{fmt::Write as FmtWrite, fs::File, io::Write, sync::Arc},
};

pub trait WalletSubCommands {
    fn wallet_subcommands(self) -> Self;
}

impl WalletSubCommands for App<'_, '_> {
    fn wallet_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("account")
                .about("Show the contents of an account")
                .alias("account")
                .arg(
                    pubkey!(Arg::with_name("account_pubkey")
                        .index(1)
                        .value_name("ACCOUNT_ADDRESS")
                        .required(true),
                        "Account key URI. ")
                )
                .arg(
                    Arg::with_name("output_file")
                        .long("output-file")
                        .short("o")
                        .value_name("FILEPATH")
                        .takes_value(true)
                        .help("Write the account data to this file"),
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
        .subcommand(
            SubCommand::with_name("address")
                .about("Get your public key")
                .arg(
                    Arg::with_name("confirm_key")
                        .long("confirm-key")
                        .takes_value(false)
                        .help("Confirm key on device; only relevant if using remote wallet"),
                ),
        )
        .subcommand(
            SubCommand::with_name("airdrop")
                .about("Request SOL from a faucet")
                .arg(
                    Arg::with_name("amount")
                        .index(1)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount)
                        .required(true)
                        .help("The airdrop amount to request, in SOL"),
                )
                .arg(
                    pubkey!(Arg::with_name("to")
                        .index(2)
                        .value_name("RECIPIENT_ADDRESS"),
                        "The account address of airdrop recipient. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("balance")
                .about("Get your balance")
                .arg(
                    pubkey!(Arg::with_name("pubkey")
                        .index(1)
                        .value_name("ACCOUNT_ADDRESS"),
                        "The account address of the balance to check. ")
                )
                .arg(
                    Arg::with_name("lamports")
                        .long("lamports")
                        .takes_value(false)
                        .help("Display balance in lamports instead of SOL"),
                ),
        )
        .subcommand(
            SubCommand::with_name("confirm")
                .about("Confirm transaction by signature")
                .arg(
                    Arg::with_name("signature")
                        .index(1)
                        .value_name("TRANSACTION_SIGNATURE")
                        .takes_value(true)
                        .required(true)
                        .help("The transaction signature to confirm"),
                )
                .after_help(// Formatted specifically for the manually-indented heredoc string
                   "Note: This will show more detailed information for finalized transactions with verbose mode (-v/--verbose).\
                  \n\
                  \nAccount modes:\
                  \n  |srwx|\
                  \n    s: signed\
                  \n    r: readable (always true)\
                  \n    w: writable\
                  \n    x: program account (inner instructions excluded)\
                   "
                ),
        )
        .subcommand(
            SubCommand::with_name("create-address-with-seed")
                .about("Generate a derived account address with a seed")
                .arg(
                    Arg::with_name("seed")
                        .index(1)
                        .value_name("SEED_STRING")
                        .takes_value(true)
                        .required(true)
                        .validator(is_derived_address_seed)
                        .help("The seed.  Must not take more than 32 bytes to encode as utf-8"),
                )
                .arg(
                    Arg::with_name("program_id")
                        .index(2)
                        .value_name("PROGRAM_ID")
                        .takes_value(true)
                        .required(true)
                        .help(
                            "The program_id that the address will ultimately be used for, \n\
                             or one of NONCE, STAKE, and VOTE keywords",
                        ),
                )
                .arg(
                    pubkey!(Arg::with_name("from")
                        .long("from")
                        .value_name("FROM_PUBKEY")
                        .required(false),
                        "From (base) key, [default: cli config keypair]. "),
                ),
        )
        .subcommand(
            SubCommand::with_name("decode-transaction")
                .about("Decode a serialized transaction")
                .arg(
                    Arg::with_name("transaction")
                        .index(1)
                        .value_name("TRANSACTION")
                        .takes_value(true)
                        .required(true)
                        .help("transaction to decode"),
                )
                .arg(
                    Arg::with_name("encoding")
                        .index(2)
                        .value_name("ENCODING")
                        .possible_values(&["base58", "base64"]) // Variants of `TransactionBinaryEncoding` enum
                        .default_value("base58")
                        .takes_value(true)
                        .required(true)
                        .help("transaction encoding"),
                ),
        )
        .subcommand(
            SubCommand::with_name("resolve-signer")
                .about("Checks that a signer is valid, and returns its specific path; useful for signers that may be specified generally, eg. usb://ledger")
                .arg(
                    Arg::with_name("signer")
                        .index(1)
                        .value_name("SIGNER_KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .validator(is_valid_signer)
                        .help("The signer path to resolve")
                )
        )
        .subcommand(
            SubCommand::with_name("transfer")
                .about("Transfer funds between system accounts")
                .alias("pay")
                .arg(
                    pubkey!(Arg::with_name("to")
                        .index(1)
                        .value_name("RECIPIENT_ADDRESS")
                        .required(true),
                        "The account address of recipient. "),
                )
                .arg(
                    Arg::with_name("amount")
                        .index(2)
                        .value_name("AMOUNT")
                        .takes_value(true)
                        .validator(is_amount_or_all)
                        .required(true)
                        .help("The amount to send, in SOL; accepts keyword ALL"),
                )
                .arg(
                    pubkey!(Arg::with_name("from")
                        .long("from")
                        .value_name("FROM_ADDRESS"),
                        "Source account of funds (if different from client local account). "),
                )
                .arg(
                    Arg::with_name("no_wait")
                        .long("no-wait")
                        .takes_value(false)
                        .help("Return signature immediately after submitting the transaction, instead of waiting for confirmations"),
                )
                .arg(
                    Arg::with_name("derived_address_seed")
                        .long("derived-address-seed")
                        .takes_value(true)
                        .value_name("SEED_STRING")
                        .requires("derived_address_program_id")
                        .validator(is_derived_address_seed)
                        .hidden(true)
                )
                .arg(
                    Arg::with_name("derived_address_program_id")
                        .long("derived-address-program-id")
                        .takes_value(true)
                        .value_name("PROGRAM_ID")
                        .requires("derived_address_seed")
                        .hidden(true)
                )
                .arg(
                    Arg::with_name("allow_unfunded_recipient")
                        .long("allow-unfunded-recipient")
                        .takes_value(false)
                        .help("Complete the transfer even if the recipient address is not funded")
                )
                .offline_args()
                .nonce_args(false)
                .arg(memo_arg())
                .arg(fee_payer_arg())
                .arg(compute_unit_price_arg()),
        )
        .subcommand(
            SubCommand::with_name("sign-offchain-message")
                .about("Sign off-chain message")
                .arg(
                    Arg::with_name("message")
                        .index(1)
                        .takes_value(true)
                        .value_name("STRING")
                        .required(true)
                        .help("The message text to be signed")
                )
                .arg(
                    Arg::with_name("version")
                        .long("version")
                        .takes_value(true)
                        .value_name("VERSION")
                        .required(false)
                        .default_value("0")
                        .validator(|p| match p.parse::<u8>() {
                            Err(_) => Err(String::from("Must be unsigned integer")),
                            Ok(_) => { Ok(()) }
                        })
                        .help("The off-chain message version")
                )
        )
        .subcommand(
            SubCommand::with_name("verify-offchain-signature")
                .about("Verify off-chain message signature")
                .arg(
                    Arg::with_name("message")
                        .index(1)
                        .takes_value(true)
                        .value_name("STRING")
                        .required(true)
                        .help("The text of the original message")
                )
                .arg(
                    Arg::with_name("signature")
                        .index(2)
                        .value_name("SIGNATURE")
                        .takes_value(true)
                        .required(true)
                        .help("The message signature to verify")
                )
                .arg(
                    Arg::with_name("version")
                        .long("version")
                        .takes_value(true)
                        .value_name("VERSION")
                        .required(false)
                        .default_value("0")
                        .validator(|p| match p.parse::<u8>() {
                            Err(_) => Err(String::from("Must be unsigned integer")),
                            Ok(_) => { Ok(()) }
                        })
                        .help("The off-chain message version")
                )
                .arg(
                    pubkey!(Arg::with_name("signer")
                        .long("signer")
                        .value_name("PUBKEY")
                        .required(false),
                        "The pubkey of the message signer (if different from config default)")
                )
        )
    }
}

fn resolve_derived_address_program_id(matches: &ArgMatches<'_>, arg_name: &str) -> Option<Pubkey> {
    matches.value_of(arg_name).and_then(|v| {
        let upper = v.to_ascii_uppercase();
        match upper.as_str() {
            "NONCE" | "SYSTEM" => Some(system_program::id()),
            "STAKE" => Some(stake::program::id()),
            "VOTE" => Some(solana_vote_program::id()),
            _ => pubkey_of(matches, arg_name),
        }
    })
}

pub fn parse_account(
    matches: &ArgMatches<'_>,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let account_pubkey = pubkey_of_signer(matches, "account_pubkey", wallet_manager)?.unwrap();
    let output_file = matches.value_of("output_file");
    let use_lamports_unit = matches.is_present("lamports");
    Ok(CliCommandInfo {
        command: CliCommand::ShowAccount {
            pubkey: account_pubkey,
            output_file: output_file.map(ToString::to_string),
            use_lamports_unit,
        },
        signers: vec![],
    })
}

pub fn parse_airdrop(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let pubkey = pubkey_of_signer(matches, "to", wallet_manager)?;
    let signers = if pubkey.is_some() {
        vec![]
    } else {
        vec![default_signer.signer_from_path(matches, wallet_manager)?]
    };
    let lamports = lamports_of_sol(matches, "amount").unwrap();
    Ok(CliCommandInfo {
        command: CliCommand::Airdrop { pubkey, lamports },
        signers,
    })
}

pub fn parse_balance(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let pubkey = pubkey_of_signer(matches, "pubkey", wallet_manager)?;
    let signers = if pubkey.is_some() {
        vec![]
    } else {
        vec![default_signer.signer_from_path(matches, wallet_manager)?]
    };
    Ok(CliCommandInfo {
        command: CliCommand::Balance {
            pubkey,
            use_lamports_unit: matches.is_present("lamports"),
        },
        signers,
    })
}

pub fn parse_decode_transaction(matches: &ArgMatches<'_>) -> Result<CliCommandInfo, CliError> {
    let blob = value_t_or_exit!(matches, "transaction", String);
    let binary_encoding = match matches.value_of("encoding").unwrap() {
        "base58" => TransactionBinaryEncoding::Base58,
        "base64" => TransactionBinaryEncoding::Base64,
        _ => unreachable!(),
    };

    let encoded_transaction = EncodedTransaction::Binary(blob, binary_encoding);
    if let Some(transaction) = encoded_transaction.decode() {
        Ok(CliCommandInfo {
            command: CliCommand::DecodeTransaction(transaction),
            signers: vec![],
        })
    } else {
        Err(CliError::BadParameter(
            "Unable to decode transaction".to_string(),
        ))
    }
}

pub fn parse_create_address_with_seed(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let from_pubkey = pubkey_of_signer(matches, "from", wallet_manager)?;
    let signers = if from_pubkey.is_some() {
        vec![]
    } else {
        vec![default_signer.signer_from_path(matches, wallet_manager)?]
    };

    let program_id = resolve_derived_address_program_id(matches, "program_id").unwrap();

    let seed = matches.value_of("seed").unwrap().to_string();

    Ok(CliCommandInfo {
        command: CliCommand::CreateAddressWithSeed {
            from_pubkey,
            seed,
            program_id,
        },
        signers,
    })
}

pub fn parse_transfer(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let amount = SpendAmount::new_from_matches(matches, "amount");
    let to = pubkey_of_signer(matches, "to", wallet_manager)?.unwrap();
    let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
    let dump_transaction_message = matches.is_present(DUMP_TRANSACTION_MESSAGE.name);
    let no_wait = matches.is_present("no_wait");
    let blockhash_query = BlockhashQuery::new_from_matches(matches);
    let nonce_account = pubkey_of_signer(matches, NONCE_ARG.name, wallet_manager)?;
    let (nonce_authority, nonce_authority_pubkey) =
        signer_of(matches, NONCE_AUTHORITY_ARG.name, wallet_manager)?;
    let memo = matches.value_of(MEMO_ARG.name).map(String::from);
    let (fee_payer, fee_payer_pubkey) = signer_of(matches, FEE_PAYER_ARG.name, wallet_manager)?;
    let (from, from_pubkey) = signer_of(matches, "from", wallet_manager)?;
    let allow_unfunded_recipient = matches.is_present("allow_unfunded_recipient");

    let mut bulk_signers = vec![fee_payer, from];
    if nonce_account.is_some() {
        bulk_signers.push(nonce_authority);
    }

    let signer_info =
        default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;
    let compute_unit_price = value_of(matches, COMPUTE_UNIT_PRICE_ARG.name);

    let derived_address_seed = matches
        .value_of("derived_address_seed")
        .map(|s| s.to_string());
    let derived_address_program_id =
        resolve_derived_address_program_id(matches, "derived_address_program_id");

    Ok(CliCommandInfo {
        command: CliCommand::Transfer {
            amount,
            to,
            sign_only,
            dump_transaction_message,
            allow_unfunded_recipient,
            no_wait,
            blockhash_query,
            nonce_account,
            nonce_authority: signer_info.index_of(nonce_authority_pubkey).unwrap(),
            memo,
            fee_payer: signer_info.index_of(fee_payer_pubkey).unwrap(),
            from: signer_info.index_of(from_pubkey).unwrap(),
            derived_address_seed,
            derived_address_program_id,
            compute_unit_price,
        },
        signers: signer_info.signers,
    })
}

pub fn parse_sign_offchain_message(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let version: u8 = value_of(matches, "version").unwrap();
    let message_text: String = value_of(matches, "message")
        .ok_or_else(|| CliError::BadParameter("MESSAGE".to_string()))?;
    let message = OffchainMessage::new(version, message_text.as_bytes())
        .map_err(|_| CliError::BadParameter("VERSION or MESSAGE".to_string()))?;

    Ok(CliCommandInfo {
        command: CliCommand::SignOffchainMessage { message },
        signers: vec![default_signer.signer_from_path(matches, wallet_manager)?],
    })
}

pub fn parse_verify_offchain_signature(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let version: u8 = value_of(matches, "version").unwrap();
    let message_text: String = value_of(matches, "message")
        .ok_or_else(|| CliError::BadParameter("MESSAGE".to_string()))?;
    let message = OffchainMessage::new(version, message_text.as_bytes())
        .map_err(|_| CliError::BadParameter("VERSION or MESSAGE".to_string()))?;

    let signer_pubkey = pubkey_of_signer(matches, "signer", wallet_manager)?;
    let signers = if signer_pubkey.is_some() {
        vec![]
    } else {
        vec![default_signer.signer_from_path(matches, wallet_manager)?]
    };

    let signature = value_of(matches, "signature")
        .ok_or_else(|| CliError::BadParameter("SIGNATURE".to_string()))?;

    Ok(CliCommandInfo {
        command: CliCommand::VerifyOffchainSignature {
            signer_pubkey,
            signature,
            message,
        },
        signers,
    })
}

pub fn process_show_account(
    rpc_client: &RpcClient,
    config: &CliConfig,
    account_pubkey: &Pubkey,
    output_file: &Option<String>,
    use_lamports_unit: bool,
) -> ProcessResult {
    let account = rpc_client.get_account(account_pubkey)?;
    let data = account.data.clone();
    let cli_account = CliAccount::new(account_pubkey, &account, use_lamports_unit);

    let mut account_string = config.output_format.formatted_string(&cli_account);

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            if let Some(output_file) = output_file {
                let mut f = File::create(output_file)?;
                f.write_all(account_string.as_bytes())?;
                writeln!(&mut account_string)?;
                writeln!(&mut account_string, "Wrote account to {output_file}")?;
            }
        }
        OutputFormat::Display | OutputFormat::DisplayVerbose => {
            if let Some(output_file) = output_file {
                let mut f = File::create(output_file)?;
                f.write_all(&data)?;
                writeln!(&mut account_string)?;
                writeln!(&mut account_string, "Wrote account data to {output_file}")?;
            } else if !data.is_empty() {
                use pretty_hex::*;
                writeln!(&mut account_string, "{:?}", data.hex_dump())?;
            }
        }
        OutputFormat::DisplayQuiet => (),
    }

    Ok(account_string)
}

pub fn process_airdrop(
    rpc_client: &RpcClient,
    config: &CliConfig,
    pubkey: &Option<Pubkey>,
    lamports: u64,
) -> ProcessResult {
    let pubkey = if let Some(pubkey) = pubkey {
        *pubkey
    } else {
        config.pubkey()?
    };
    println!(
        "Requesting airdrop of {}",
        build_balance_message(lamports, false, true),
    );

    let pre_balance = rpc_client.get_balance(&pubkey)?;

    let result = request_and_confirm_airdrop(rpc_client, config, &pubkey, lamports);
    if let Ok(signature) = result {
        let signature_cli_message = log_instruction_custom_error::<SystemError>(result, config)?;
        println!("{signature_cli_message}");

        let current_balance = rpc_client.get_balance(&pubkey)?;

        if current_balance < pre_balance.saturating_add(lamports) {
            println!("Balance unchanged");
            println!("Run `solana confirm -v {signature:?}` for more info");
            Ok("".to_string())
        } else {
            Ok(build_balance_message(current_balance, false, true))
        }
    } else {
        log_instruction_custom_error::<SystemError>(result, config)
    }
}

pub fn process_balance(
    rpc_client: &RpcClient,
    config: &CliConfig,
    pubkey: &Option<Pubkey>,
    use_lamports_unit: bool,
) -> ProcessResult {
    let pubkey = if let Some(pubkey) = pubkey {
        *pubkey
    } else {
        config.pubkey()?
    };
    let balance = rpc_client.get_balance(&pubkey)?;
    let balance_output = CliBalance {
        lamports: balance,
        config: BuildBalanceMessageConfig {
            use_lamports_unit,
            show_unit: true,
            trim_trailing_zeros: true,
        },
    };

    Ok(config.output_format.formatted_string(&balance_output))
}

pub fn process_confirm(
    rpc_client: &RpcClient,
    config: &CliConfig,
    signature: &Signature,
) -> ProcessResult {
    match rpc_client.get_signature_statuses_with_history(&[*signature]) {
        Ok(status) => {
            let cli_transaction = if let Some(transaction_status) = &status.value[0] {
                let mut transaction = None;
                let mut get_transaction_error = None;
                if config.verbose {
                    match rpc_client.get_transaction_with_config(
                        signature,
                        RpcTransactionConfig {
                            encoding: Some(UiTransactionEncoding::Base64),
                            commitment: Some(CommitmentConfig::confirmed()),
                            max_supported_transaction_version: Some(0),
                        },
                    ) {
                        Ok(confirmed_transaction) => {
                            let EncodedConfirmedTransactionWithStatusMeta {
                                block_time,
                                slot,
                                transaction: transaction_with_meta,
                            } = confirmed_transaction;

                            let decoded_transaction =
                                transaction_with_meta.transaction.decode().unwrap();
                            let json_transaction = decoded_transaction.json_encode();

                            transaction = Some(CliTransaction {
                                transaction: json_transaction,
                                meta: transaction_with_meta.meta,
                                block_time,
                                slot: Some(slot),
                                decoded_transaction,
                                prefix: "  ".to_string(),
                                sigverify_status: vec![],
                            });
                        }
                        Err(err) => {
                            get_transaction_error = Some(format!("{err:?}"));
                        }
                    }
                }
                CliTransactionConfirmation {
                    confirmation_status: Some(transaction_status.confirmation_status()),
                    transaction,
                    get_transaction_error,
                    err: transaction_status.err.clone(),
                }
            } else {
                CliTransactionConfirmation {
                    confirmation_status: None,
                    transaction: None,
                    get_transaction_error: None,
                    err: None,
                }
            };
            Ok(config.output_format.formatted_string(&cli_transaction))
        }
        Err(err) => Err(CliError::RpcRequestError(format!("Unable to confirm: {err}")).into()),
    }
}

#[allow(clippy::unnecessary_wraps)]
pub fn process_decode_transaction(
    config: &CliConfig,
    transaction: &VersionedTransaction,
) -> ProcessResult {
    let sigverify_status = CliSignatureVerificationStatus::verify_transaction(transaction);
    let decode_transaction = CliTransaction {
        decoded_transaction: transaction.clone(),
        transaction: transaction.json_encode(),
        meta: None,
        block_time: None,
        slot: None,
        prefix: "".to_string(),
        sigverify_status,
    };
    Ok(config.output_format.formatted_string(&decode_transaction))
}

pub fn process_create_address_with_seed(
    config: &CliConfig,
    from_pubkey: Option<&Pubkey>,
    seed: &str,
    program_id: &Pubkey,
) -> ProcessResult {
    let from_pubkey = if let Some(pubkey) = from_pubkey {
        *pubkey
    } else {
        config.pubkey()?
    };
    let address = Pubkey::create_with_seed(&from_pubkey, seed, program_id)?;
    Ok(address.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn process_transfer(
    rpc_client: &RpcClient,
    config: &CliConfig,
    amount: SpendAmount,
    to: &Pubkey,
    from: SignerIndex,
    sign_only: bool,
    dump_transaction_message: bool,
    allow_unfunded_recipient: bool,
    no_wait: bool,
    blockhash_query: &BlockhashQuery,
    nonce_account: Option<&Pubkey>,
    nonce_authority: SignerIndex,
    memo: Option<&String>,
    fee_payer: SignerIndex,
    derived_address_seed: Option<String>,
    derived_address_program_id: Option<&Pubkey>,
    compute_unit_price: Option<&u64>,
) -> ProcessResult {
    let from = config.signers[from];
    let mut from_pubkey = from.pubkey();

    let recent_blockhash = blockhash_query.get_blockhash(rpc_client, config.commitment)?;

    if !sign_only && !allow_unfunded_recipient {
        let recipient_balance = rpc_client
            .get_balance_with_commitment(to, config.commitment)?
            .value;
        if recipient_balance == 0 {
            return Err(format!(
                "The recipient address ({to}) is not funded. \
                                Add `--allow-unfunded-recipient` to complete the transfer \
                               "
            )
            .into());
        }
    }

    let nonce_authority = config.signers[nonce_authority];
    let fee_payer = config.signers[fee_payer];

    let derived_parts = derived_address_seed.zip(derived_address_program_id);
    let with_seed = if let Some((seed, program_id)) = derived_parts {
        let base_pubkey = from_pubkey;
        from_pubkey = Pubkey::create_with_seed(&base_pubkey, &seed, program_id)?;
        Some((base_pubkey, seed, program_id, from_pubkey))
    } else {
        None
    };

    let build_message = |lamports| {
        let ixs = if let Some((base_pubkey, seed, program_id, from_pubkey)) = with_seed.as_ref() {
            vec![system_instruction::transfer_with_seed(
                from_pubkey,
                base_pubkey,
                seed.clone(),
                program_id,
                to,
                lamports,
            )]
            .with_memo(memo)
            .with_compute_unit_price(compute_unit_price)
        } else {
            vec![system_instruction::transfer(&from_pubkey, to, lamports)]
                .with_memo(memo)
                .with_compute_unit_price(compute_unit_price)
        };

        if let Some(nonce_account) = &nonce_account {
            Message::new_with_nonce(
                ixs,
                Some(&fee_payer.pubkey()),
                nonce_account,
                &nonce_authority.pubkey(),
            )
        } else {
            Message::new(&ixs, Some(&fee_payer.pubkey()))
        }
    };

    let (message, _) = resolve_spend_tx_and_check_account_balances(
        rpc_client,
        sign_only,
        amount,
        &recent_blockhash,
        &from_pubkey,
        &fee_payer.pubkey(),
        build_message,
        config.commitment,
    )?;
    let mut tx = Transaction::new_unsigned(message);

    if sign_only {
        tx.try_partial_sign(&config.signers, recent_blockhash)?;
        return_signers_with_config(
            &tx,
            &config.output_format,
            &ReturnSignersConfig {
                dump_transaction_message,
            },
        )
    } else {
        if let Some(nonce_account) = &nonce_account {
            let nonce_account = solana_rpc_client_nonce_utils::get_account_with_commitment(
                rpc_client,
                nonce_account,
                config.commitment,
            )?;
            check_nonce_account(&nonce_account, &nonce_authority.pubkey(), &recent_blockhash)?;
        }

        tx.try_sign(&config.signers, recent_blockhash)?;
        let result = if no_wait {
            rpc_client.send_transaction(&tx)
        } else {
            rpc_client.send_and_confirm_transaction_with_spinner(&tx)
        };
        log_instruction_custom_error::<SystemError>(result, config)
    }
}

pub fn process_sign_offchain_message(
    config: &CliConfig,
    message: &OffchainMessage,
) -> ProcessResult {
    Ok(message.sign(config.signers[0])?.to_string())
}

pub fn process_verify_offchain_signature(
    config: &CliConfig,
    signer_pubkey: &Option<Pubkey>,
    signature: &Signature,
    message: &OffchainMessage,
) -> ProcessResult {
    let signer = if let Some(pubkey) = signer_pubkey {
        *pubkey
    } else {
        config.signers[0].pubkey()
    };

    if message.verify(&signer, signature)? {
        Ok("Signature is valid".to_string())
    } else {
        Err(CliError::InvalidSignature.into())
    }
}
