use {
    crate::{
        checks::*,
        cli::{
            common_error_adapter, log_instruction_custom_error_ex, CliCommand, CliCommandInfo,
            CliConfig, CliError, ProcessResult,
        },
        feature::{status_from_account, CliFeatureStatus},
        program::calculate_max_chunk_size,
    },
    clap::{value_t, App, AppSettings, Arg, ArgMatches, SubCommand},
    log::*,
    solana_account_decoder::{UiAccountEncoding, UiDataSliceConfig},
    solana_clap_utils::{
        input_parsers::{pubkey_of, pubkey_of_signer, signer_of},
        input_validators::{is_valid_pubkey, is_valid_signer},
        keypair::{DefaultSigner, SignerIndex},
    },
    solana_cli_output::{CliProgramId, CliProgramV4, CliProgramsV4},
    solana_client::{
        connection_cache::ConnectionCache,
        send_and_confirm_transactions_in_parallel::{
            send_and_confirm_transactions_in_parallel_blocking_v2, SendAndConfirmConfigV2,
        },
        tpu_client::{TpuClient, TpuClientConfig},
    },
    solana_compute_budget::compute_budget::ComputeBudget,
    solana_program_runtime::invoke_context::InvokeContext,
    solana_rbpf::{elf::Executable, verifier::RequisiteVerifier},
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{
        config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
        filter::{Memcmp, RpcFilterType},
        request::MAX_MULTIPLE_ACCOUNTS,
    },
    solana_sdk::{
        account::Account,
        feature_set::{FeatureSet, FEATURE_NAMES},
        instruction::Instruction,
        loader_v4::{
            self, LoaderV4State,
            LoaderV4Status::{self, Retracted},
        },
        message::Message,
        pubkey::Pubkey,
        signature::Signer,
        system_instruction::{self, SystemError, MAX_PERMITTED_DATA_LENGTH},
        transaction::Transaction,
    },
    std::{
        cmp::Ordering,
        fs::File,
        io::{Read, Write},
        mem::size_of,
        num::Saturating,
        ops::Range,
        rc::Rc,
        sync::Arc,
    },
};

#[derive(Debug, PartialEq, Eq)]
pub enum ProgramV4CliCommand {
    Deploy {
        program_address: Option<Pubkey>,
        program_signer_index: Option<SignerIndex>,
        buffer_signer_index: Option<SignerIndex>,
        authority_signer_index: SignerIndex,
        program_location: String,
        upload_range: Range<Option<usize>>,
        use_rpc: bool,
    },
    Undeploy {
        program_address: Pubkey,
        authority_signer_index: SignerIndex,
    },
    TransferAuthority {
        program_address: Pubkey,
        authority_signer_index: SignerIndex,
        new_authority_signer_index: SignerIndex,
    },
    Finalize {
        program_address: Pubkey,
        authority_signer_index: SignerIndex,
        next_version_signer_index: SignerIndex,
    },
    Show {
        account_pubkey: Option<Pubkey>,
        authority: Pubkey,
        all: bool,
    },
    Dump {
        account_pubkey: Option<Pubkey>,
        output_location: String,
    },
}

pub trait ProgramV4SubCommands {
    fn program_v4_subcommands(self) -> Self;
}

impl ProgramV4SubCommands for App<'_, '_> {
    fn program_v4_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("program-v4")
                .about("Program V4 management")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("deploy")
                        .about("Deploy a new or redeploy an existing program")
                        .arg(
                            Arg::with_name("program-location")
                                .index(1)
                                .value_name("PROGRAM_FILEPATH")
                                .takes_value(true)
                                .help("/path/to/program.so"),
                        )
                        .arg(
                            Arg::with_name("start-offset")
                                .long("start-offset")
                                .value_name("START_OFFSET")
                                .takes_value(true)
                                .help("Optionally starts writing at this byte offset"),
                        )
                        .arg(
                            Arg::with_name("end-offset")
                                .long("end-offset")
                                .value_name("END_OFFSET")
                                .takes_value(true)
                                .help("Optionally stops writing after this byte offset"),
                        )
                        .arg(
                            Arg::with_name("program")
                                .long("program")
                                .value_name("PROGRAM_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Program account signer for deploying a new program",
                                ),
                        )
                        .arg(
                            Arg::with_name("program-id")
                                .long("program-id")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .help("Program address for redeploying an existing program"),
                        )
                        .arg(
                            Arg::with_name("buffer")
                                .long("buffer")
                                .value_name("BUFFER_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Optional intermediate buffer account to write data to",
                                ),
                        )
                        .arg(
                            Arg::with_name("authority")
                                .long("authority")
                                .value_name("AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Program authority [default: the default configured keypair]",
                                ),
                        )
                        .arg(Arg::with_name("use_rpc").long("use-rpc").help(
                            "Send transactions to the configured RPC instead of validator TPUs",
                        )),
                )
                .subcommand(
                    SubCommand::with_name("undeploy")
                        .about("Undeploy/close a program")
                        .arg(
                            Arg::with_name("program-id")
                                .long("program-id")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .help("Executable program's address"),
                        )
                        .arg(
                            Arg::with_name("authority")
                                .long("authority")
                                .value_name("AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Program authority [default: the default configured keypair]",
                                ),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("transfer-authority")
                        .about("Transfer the authority of a program to a different address")
                        .arg(
                            Arg::with_name("program-id")
                                .long("program-id")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .help("Executable program's address"),
                        )
                        .arg(
                            Arg::with_name("authority")
                                .long("authority")
                                .value_name("AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Current program authority [default: the default configured keypair]",
                                ),
                        )
                        .arg(
                            Arg::with_name("new-authority")
                                .long("new-authority")
                                .value_name("NEW_AUTHORITY_SIGNER")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_signer)
                                .help(
                                    "New program authority",
                                ),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("finalize")
                        .about("Finalize a program to make it immutable")
                        .arg(
                            Arg::with_name("program-id")
                                .long("program-id")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .help("Executable program's address"),
                        )
                        .arg(
                            Arg::with_name("authority")
                                .long("authority")
                                .value_name("AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Program authority [default: the default configured keypair]",
                                ),
                        )
                        .arg(
                            Arg::with_name("next-version")
                                .long("next-version")
                                .value_name("NEXT_VERSION")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help(
                                    "Reserves the address and links it as the programs next-version, which is a hint that frontends can show to users",
                                ),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("show")
                        .about("Display information about a buffer or program")
                        .arg(
                            Arg::with_name("account")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .help("Address of the program to show"),
                        )
                        .arg(
                            Arg::with_name("all")
                                .long("all")
                                .conflicts_with("account")
                                .conflicts_with("authority")
                                .help("Show accounts for all authorities"),
                        )
                        .arg(pubkey!(
                            Arg::with_name("authority")
                                .long("authority")
                                .value_name("AUTHORITY")
                                .conflicts_with("all"),
                            "Authority [default: the default configured keypair]."
                        )),
                )
                .subcommand(
                    SubCommand::with_name("dump")
                        .about("Write the program data to a file")
                        .arg(
                            Arg::with_name("account")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .help("Address of the buffer or program"),
                        )
                        .arg(
                            Arg::with_name("output_location")
                                .index(2)
                                .value_name("OUTPUT_FILEPATH")
                                .takes_value(true)
                                .required(true)
                                .help("/path/to/program.so"),
                        ),
                ),
        )
    }
}

pub fn parse_program_v4_subcommand(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let (subcommand, sub_matches) = matches.subcommand();
    let response = match (subcommand, sub_matches) {
        ("deploy", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let program_location = matches
                .value_of("program-location")
                .map(|location| location.to_string());

            let program_address = pubkey_of(matches, "program-id");
            let program_pubkey = if let Ok((program_signer, Some(program_pubkey))) =
                signer_of(matches, "program", wallet_manager)
            {
                bulk_signers.push(program_signer);
                Some(program_pubkey)
            } else {
                pubkey_of_signer(matches, "program", wallet_manager)?
            };

            let buffer_pubkey = if let Ok((buffer_signer, Some(buffer_pubkey))) =
                signer_of(matches, "buffer", wallet_manager)
            {
                bulk_signers.push(buffer_signer);
                Some(buffer_pubkey)
            } else {
                pubkey_of_signer(matches, "buffer", wallet_manager)?
            };

            let (authority, authority_pubkey) = signer_of(matches, "authority", wallet_manager)?;
            bulk_signers.push(authority);

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;
            let program_signer_index = signer_info.index_of_or_none(program_pubkey);
            assert!(
                program_address.is_some() != program_signer_index.is_some(),
                "Requires either program signer or program address",
            );

            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Deploy {
                    program_address,
                    program_signer_index,
                    buffer_signer_index: signer_info.index_of_or_none(buffer_pubkey),
                    authority_signer_index: signer_info
                        .index_of(authority_pubkey)
                        .expect("Authority signer is missing"),
                    program_location: program_location.expect("Program location is missing"),
                    upload_range: value_t!(matches, "start-offset", usize).ok()
                        ..value_t!(matches, "end-offset", usize).ok(),
                    use_rpc: matches.is_present("use_rpc"),
                }),
                signers: signer_info.signers,
            }
        }
        ("undeploy", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let (authority, authority_pubkey) = signer_of(matches, "authority", wallet_manager)?;
            bulk_signers.push(authority);

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;

            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Undeploy {
                    program_address: pubkey_of(matches, "program-id")
                        .expect("Program address is missing"),
                    authority_signer_index: signer_info
                        .index_of(authority_pubkey)
                        .expect("Authority signer is missing"),
                }),
                signers: signer_info.signers,
            }
        }
        ("transfer-authority", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let (authority, authority_pubkey) = signer_of(matches, "authority", wallet_manager)?;
            bulk_signers.push(authority);

            let (new_authority, new_authority_pubkey) =
                signer_of(matches, "new-authority", wallet_manager)?;
            bulk_signers.push(new_authority);

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;

            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::TransferAuthority {
                    program_address: pubkey_of(matches, "program-id")
                        .expect("Program address is missing"),
                    authority_signer_index: signer_info
                        .index_of(authority_pubkey)
                        .expect("Authority signer is missing"),
                    new_authority_signer_index: signer_info
                        .index_of(new_authority_pubkey)
                        .expect("Authority signer is missing"),
                }),
                signers: signer_info.signers,
            }
        }
        ("finalize", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let (authority, authority_pubkey) = signer_of(matches, "authority", wallet_manager)?;
            bulk_signers.push(authority);

            if let Ok((next_version, _next_version_pubkey)) =
                signer_of(matches, "next-version", wallet_manager)
            {
                bulk_signers.push(next_version);
            }

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;
            let authority_signer_index = signer_info
                .index_of(authority_pubkey)
                .expect("Authority signer is missing");

            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Finalize {
                    program_address: pubkey_of(matches, "program-id")
                        .expect("Program address is missing"),
                    authority_signer_index,
                    next_version_signer_index: pubkey_of(matches, "next-version")
                        .and_then(|pubkey| signer_info.index_of(Some(pubkey)))
                        .unwrap_or(authority_signer_index),
                }),
                signers: signer_info.signers,
            }
        }
        ("show", Some(matches)) => {
            let authority =
                if let Some(authority) = pubkey_of_signer(matches, "authority", wallet_manager)? {
                    authority
                } else {
                    default_signer
                        .signer_from_path(matches, wallet_manager)?
                        .pubkey()
                };

            CliCommandInfo::without_signers(CliCommand::ProgramV4(ProgramV4CliCommand::Show {
                account_pubkey: pubkey_of(matches, "account"),
                authority,
                all: matches.is_present("all"),
            }))
        }
        ("dump", Some(matches)) => {
            CliCommandInfo::without_signers(CliCommand::ProgramV4(ProgramV4CliCommand::Dump {
                account_pubkey: pubkey_of(matches, "account"),
                output_location: matches.value_of("output_location").unwrap().to_string(),
            }))
        }
        _ => unreachable!(),
    };
    Ok(response)
}

pub fn process_program_v4_subcommand(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_subcommand: &ProgramV4CliCommand,
) -> ProcessResult {
    match program_subcommand {
        ProgramV4CliCommand::Deploy {
            program_address,
            program_signer_index,
            buffer_signer_index,
            authority_signer_index,
            program_location,
            upload_range,
            use_rpc,
        } => {
            let mut program_data = Vec::new();
            let mut file = File::open(program_location)
                .map_err(|err| format!("Unable to open program file: {err}"))?;
            file.read_to_end(&mut program_data)
                .map_err(|err| format!("Unable to read program file: {err}"))?;
            process_deploy_program(
                rpc_client,
                config,
                authority_signer_index,
                &program_address
                    .unwrap_or_else(|| config.signers[program_signer_index.unwrap()].pubkey()),
                &program_data,
                upload_range.clone(),
                program_signer_index
                    .or(*buffer_signer_index)
                    .map(|index| config.signers[index]),
                *use_rpc,
            )
        }
        ProgramV4CliCommand::Undeploy {
            program_address,
            authority_signer_index,
        } => process_undeploy_program(rpc_client, config, authority_signer_index, program_address),
        ProgramV4CliCommand::TransferAuthority {
            program_address,
            authority_signer_index,
            new_authority_signer_index,
        } => process_transfer_authority_of_program(
            rpc_client,
            config,
            authority_signer_index,
            program_address,
            config.signers[*new_authority_signer_index],
        ),
        ProgramV4CliCommand::Finalize {
            program_address,
            authority_signer_index,
            next_version_signer_index,
        } => process_finalize_program(
            rpc_client,
            config,
            authority_signer_index,
            program_address,
            config.signers[*next_version_signer_index],
        ),
        ProgramV4CliCommand::Show {
            account_pubkey,
            authority,
            all,
        } => process_show(rpc_client, config, *account_pubkey, *authority, *all),
        ProgramV4CliCommand::Dump {
            account_pubkey,
            output_location,
        } => process_dump(rpc_client, config, *account_pubkey, output_location),
    }
}

// This function can be used for the following use-cases
// * Deploy a program
//   - buffer_signer argument must contain program signer information
//     (program_address must be same as buffer_signer.pubkey())
// * Redeploy a program using original program account
//   - buffer_signer argument must be None
// * Redeploy a program using a buffer account
//   - buffer_signer argument must contain the temporary buffer account information
//     (program_address must contain program ID and must NOT be same as buffer_signer.pubkey())
pub fn process_deploy_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    auth_signer_index: &SignerIndex,
    program_address: &Pubkey,
    program_data: &[u8],
    upload_range: Range<Option<usize>>,
    buffer_signer: Option<&dyn Signer>,
    use_rpc: bool,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();
    let authority_pubkey = config.signers[*auth_signer_index].pubkey();

    // Download feature set
    let mut feature_set = FeatureSet::default();
    for feature_ids in FEATURE_NAMES
        .keys()
        .cloned()
        .collect::<Vec<Pubkey>>()
        .chunks(MAX_MULTIPLE_ACCOUNTS)
    {
        rpc_client
            .get_multiple_accounts(feature_ids)?
            .into_iter()
            .zip(feature_ids)
            .for_each(|(account, feature_id)| {
                let activation_slot = account.and_then(status_from_account);

                if let Some(CliFeatureStatus::Active(slot)) = activation_slot {
                    feature_set.activate(feature_id, slot);
                }
            });
    }
    let program_runtime_environment =
        solana_bpf_loader_program::syscalls::create_program_runtime_environment_v1(
            &feature_set,
            &ComputeBudget::default(),
            true,
            false,
        )
        .unwrap();

    // Verify the program
    let upload_range =
        upload_range.start.unwrap_or(0)..upload_range.end.unwrap_or(program_data.len());
    const MAX_LEN: usize =
        (MAX_PERMITTED_DATA_LENGTH as usize).saturating_sub(LoaderV4State::program_data_offset());
    assert!(
        program_data.len() <= MAX_LEN,
        "Program length {} exeeds maximum length {}",
        program_data.len(),
        MAX_LEN,
    );
    assert!(
        upload_range.start < upload_range.end,
        "Range {}..{} is empty",
        upload_range.start,
        upload_range.end,
    );
    assert!(
        upload_range.end <= program_data.len(),
        "Range end {} exeeds program length {}",
        upload_range.end,
        program_data.len(),
    );
    let executable =
        Executable::<InvokeContext>::from_elf(program_data, Arc::new(program_runtime_environment))
            .map_err(|err| format!("ELF error: {err}"))?;
    executable
        .verify::<RequisiteVerifier>()
        .map_err(|err| format!("ELF error: {err}"))?;

    // Create and add retract message
    let mut initial_messages = Vec::default();
    let program_account = rpc_client
        .get_account_with_commitment(program_address, config.commitment)?
        .value;
    let program_account_exists = program_account.is_some();
    let mut retract_instruction = None;
    if let Some(program_account) = program_account.as_ref() {
        retract_instruction =
            build_retract_instruction(program_account, program_address, &authority_pubkey)?;
    }

    let lamports_required = rpc_client.get_minimum_balance_for_rent_exemption(
        LoaderV4State::program_data_offset().saturating_add(program_data.len()),
    )?;
    let (buffer_address, buffer_account) = if let Some(buffer_signer) = buffer_signer {
        // Deploy new program or redeploy with a buffer account
        let buffer_address = buffer_signer.pubkey();
        let buffer_account = rpc_client
            .get_account_with_commitment(&buffer_address, config.commitment)?
            .value;
        if buffer_account.is_none() {
            // Create and add create_buffer message
            initial_messages.push(Message::new_with_blockhash(
                &loader_v4::create_buffer(
                    &payer_pubkey,
                    &buffer_address,
                    lamports_required,
                    &authority_pubkey,
                    program_data.len() as u32,
                    &payer_pubkey,
                ),
                Some(&payer_pubkey),
                &blockhash,
            ));
        }
        (buffer_address, buffer_account)
    } else {
        // Redeploy without a buffer account
        (*program_address, program_account)
    };

    if buffer_signer.is_none() || &buffer_address != program_address {
        // Redeploy an existing program
        if !program_account_exists {
            return Err("Program account does not exist".into());
        }
    } else {
        // Deploy new program
        if program_account_exists {
            return Err("Program account does exist already".into());
        }
    }

    // Create and add truncate message
    if let Some(buffer_account) = buffer_account.as_ref() {
        let (truncate_instructions, _lamports_required) = build_truncate_instructions(
            rpc_client.clone(),
            config,
            auth_signer_index,
            buffer_account,
            &buffer_address,
            program_data.len() as u32,
        )?;
        if !truncate_instructions.is_empty() {
            initial_messages.push(Message::new_with_blockhash(
                &truncate_instructions,
                Some(&payer_pubkey),
                &blockhash,
            ));
        }
    }

    // Create and add write messages
    let mut write_messages = vec![];
    let create_msg = |offset: u32, bytes: Vec<u8>| {
        let instruction = loader_v4::write(&buffer_address, &authority_pubkey, offset, bytes);
        Message::new_with_blockhash(&[instruction], Some(&payer_pubkey), &blockhash)
    };
    let chunk_size = calculate_max_chunk_size(&create_msg);
    for (chunk, i) in program_data[upload_range.clone()]
        .chunks(chunk_size)
        .zip(0usize..)
    {
        write_messages.push(create_msg(
            (upload_range.start as u32).saturating_add(i.saturating_mul(chunk_size) as u32),
            chunk.to_vec(),
        ));
    }

    // Create and add deploy messages
    let final_messages = [if &buffer_address != program_address {
        // Redeploy with a buffer account
        let mut instructions = Vec::default();
        if let Some(retract_instruction) = retract_instruction {
            instructions.push(retract_instruction);
        }
        instructions.push(loader_v4::deploy_from_source(
            program_address,
            &authority_pubkey,
            &buffer_address,
        ));
        Message::new_with_blockhash(&instructions, Some(&payer_pubkey), &blockhash)
    } else {
        // Deploy new program or redeploy without a buffer account
        if let Some(retract_instruction) = retract_instruction {
            initial_messages.insert(
                0,
                Message::new_with_blockhash(
                    &[retract_instruction],
                    Some(&payer_pubkey),
                    &blockhash,
                ),
            );
        }
        Message::new_with_blockhash(
            &[loader_v4::deploy(program_address, &authority_pubkey)],
            Some(&payer_pubkey),
            &blockhash,
        )
    }];

    check_payer(
        rpc_client.clone(),
        config,
        lamports_required,
        &initial_messages,
        &write_messages,
        &final_messages,
    )?;

    send_messages(
        rpc_client,
        config,
        auth_signer_index,
        &initial_messages,
        &write_messages,
        &final_messages,
        buffer_signer,
        use_rpc,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
        signature: None,
    };
    Ok(config.output_format.formatted_string(&program_id))
}

fn process_undeploy_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    auth_signer_index: &SignerIndex,
    program_address: &Pubkey,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();
    let authority_pubkey = config.signers[*auth_signer_index].pubkey();

    let Some(program_account) = rpc_client
        .get_account_with_commitment(program_address, config.commitment)?
        .value
    else {
        return Err("Program account does not exist".into());
    };

    let retract_instruction =
        build_retract_instruction(&program_account, program_address, &authority_pubkey)?;

    let mut initial_messages = if let Some(instruction) = retract_instruction {
        vec![Message::new_with_blockhash(
            &[instruction],
            Some(&payer_pubkey),
            &blockhash,
        )]
    } else {
        vec![]
    };

    let truncate_instruction =
        loader_v4::truncate(program_address, &authority_pubkey, 0, &payer_pubkey);

    initial_messages.push(Message::new_with_blockhash(
        &[truncate_instruction],
        Some(&payer_pubkey),
        &blockhash,
    ));

    check_payer(rpc_client.clone(), config, 0, &initial_messages, &[], &[])?;

    send_messages(
        rpc_client,
        config,
        auth_signer_index,
        &initial_messages,
        &[],
        &[],
        None,
        true,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
        signature: None,
    };
    Ok(config.output_format.formatted_string(&program_id))
}

fn process_transfer_authority_of_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    auth_signer_index: &SignerIndex,
    program_address: &Pubkey,
    new_authority: &dyn Signer,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();
    let authority_pubkey = config.signers[*auth_signer_index].pubkey();

    let message = [Message::new_with_blockhash(
        &[loader_v4::transfer_authority(
            program_address,
            &authority_pubkey,
            &new_authority.pubkey(),
        )],
        Some(&payer_pubkey),
        &blockhash,
    )];
    check_payer(rpc_client.clone(), config, 0, &message, &[], &[])?;

    send_messages(
        rpc_client,
        config,
        auth_signer_index,
        &message,
        &[],
        &[],
        None,
        true,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
        signature: None,
    };
    Ok(config.output_format.formatted_string(&program_id))
}

fn process_finalize_program(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    auth_signer_index: &SignerIndex,
    program_address: &Pubkey,
    next_version: &dyn Signer,
) -> ProcessResult {
    let blockhash = rpc_client.get_latest_blockhash()?;
    let payer_pubkey = config.signers[0].pubkey();
    let authority_pubkey = config.signers[*auth_signer_index].pubkey();

    let message = [Message::new_with_blockhash(
        &[loader_v4::finalize(
            program_address,
            &authority_pubkey,
            &next_version.pubkey(),
        )],
        Some(&payer_pubkey),
        &blockhash,
    )];
    check_payer(rpc_client.clone(), config, 0, &message, &[], &[])?;

    send_messages(
        rpc_client,
        config,
        auth_signer_index,
        &message,
        &[],
        &[],
        None,
        true,
    )?;

    let program_id = CliProgramId {
        program_id: program_address.to_string(),
        signature: None,
    };
    Ok(config.output_format.formatted_string(&program_id))
}

fn process_show(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    program_address: Option<Pubkey>,
    authority: Pubkey,
    all: bool,
) -> ProcessResult {
    if let Some(program_address) = program_address {
        if let Some(account) = rpc_client
            .get_account_with_commitment(&program_address, config.commitment)?
            .value
        {
            if loader_v4::check_id(&account.owner) {
                if let Ok(state) = solana_loader_v4_program::get_state(&account.data) {
                    let status = match state.status {
                        LoaderV4Status::Retracted => "retracted",
                        LoaderV4Status::Deployed => "deployed",
                        LoaderV4Status::Finalized => "finalized",
                    };
                    Ok(config.output_format.formatted_string(&CliProgramV4 {
                        program_id: program_address.to_string(),
                        owner: account.owner.to_string(),
                        authority: state.authority_address_or_next_version.to_string(),
                        last_deploy_slot: state.slot,
                        data_len: account
                            .data
                            .len()
                            .saturating_sub(LoaderV4State::program_data_offset()),
                        status: status.to_string(),
                    }))
                } else {
                    Err(format!("{program_address} SBF program state is invalid").into())
                }
            } else {
                Err(format!("{program_address} is not an SBF program").into())
            }
        } else {
            Err(format!("Unable to find the account {program_address}").into())
        }
    } else {
        let authority_pubkey = if all { None } else { Some(authority) };
        let programs = get_programs(rpc_client, config, authority_pubkey)?;
        Ok(config.output_format.formatted_string(&programs))
    }
}

pub fn process_dump(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    account_pubkey: Option<Pubkey>,
    output_location: &str,
) -> ProcessResult {
    if let Some(account_pubkey) = account_pubkey {
        if let Some(account) = rpc_client
            .get_account_with_commitment(&account_pubkey, config.commitment)?
            .value
        {
            if loader_v4::check_id(&account.owner) {
                let mut f = File::create(output_location)?;
                f.write_all(&account.data[LoaderV4State::program_data_offset()..])?;
                Ok(format!("Wrote program to {output_location}"))
            } else {
                Err(format!("{account_pubkey} is not an SBF program").into())
            }
        } else {
            Err(format!("Unable to find the account {account_pubkey}").into())
        }
    } else {
        Err("No account specified".into())
    }
}

fn check_payer(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    balance_needed: u64,
    initial_messages: &[Message],
    write_messages: &[Message],
    other_messages: &[Message],
) -> Result<(), Box<dyn std::error::Error>> {
    let payer_pubkey = config.signers[0].pubkey();
    let mut fee = Saturating(0);
    for message in initial_messages {
        fee += rpc_client.get_fee_for_message(message)?;
    }
    for message in other_messages {
        fee += rpc_client.get_fee_for_message(message)?;
    }
    // Assume all write messages cost the same
    if let Some(message) = write_messages.first() {
        fee += rpc_client
            .get_fee_for_message(message)?
            .saturating_mul(write_messages.len() as u64);
    }
    check_account_for_spend_and_fee_with_commitment(
        &rpc_client,
        &payer_pubkey,
        balance_needed,
        fee.0,
        config.commitment,
    )?;
    Ok(())
}

fn send_messages(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    auth_signer_index: &SignerIndex,
    initial_messages: &[Message],
    write_messages: &[Message],
    final_messages: &[Message],
    program_signer: Option<&dyn Signer>,
    use_rpc: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    for message in initial_messages {
        if message.header.num_required_signatures == 3 {
            // The initial message that creates the account and truncates it to the required size requires
            // 3 signatures (payer, program, and authority).
            if let Some(initial_signer) = program_signer {
                let blockhash = rpc_client.get_latest_blockhash()?;

                let mut initial_transaction = Transaction::new_unsigned(message.clone());
                initial_transaction.try_sign(
                    &[
                        config.signers[0],
                        initial_signer,
                        config.signers[*auth_signer_index],
                    ],
                    blockhash,
                )?;
                let result = rpc_client.send_and_confirm_transaction_with_spinner_and_config(
                    &initial_transaction,
                    config.commitment,
                    config.send_transaction_config,
                );
                log_instruction_custom_error_ex::<SystemError, _>(
                    result,
                    &config.output_format,
                    common_error_adapter,
                )
                .map_err(|err| format!("Account allocation failed: {err}"))?;
            } else {
                return Err("Buffer account not created yet, must provide a key pair".into());
            }
        } else if message.header.num_required_signatures == 2 {
            // All other messages should require 2 signatures (payer, and authority)
            let blockhash = rpc_client.get_latest_blockhash()?;

            let mut initial_transaction = Transaction::new_unsigned(message.clone());
            initial_transaction.try_sign(
                &[config.signers[0], config.signers[*auth_signer_index]],
                blockhash,
            )?;
            let result = rpc_client.send_and_confirm_transaction_with_spinner_and_config(
                &initial_transaction,
                config.commitment,
                config.send_transaction_config,
            );
            log_instruction_custom_error_ex::<SystemError, _>(
                result,
                &config.output_format,
                common_error_adapter,
            )
            .map_err(|err| format!("Failed to send initial message: {err}"))?;
        } else {
            return Err("Initial message requires incorrect number of signatures".into());
        }
    }

    if !write_messages.is_empty() {
        trace!("Writing program data");
        let connection_cache = if config.use_quic {
            ConnectionCache::new_quic("connection_cache_cli_program_v4_quic", 1)
        } else {
            ConnectionCache::with_udp("connection_cache_cli_program_v4_udp", 1)
        };
        let transaction_errors = match connection_cache {
            ConnectionCache::Udp(cache) => TpuClient::new_with_connection_cache(
                rpc_client.clone(),
                &config.websocket_url,
                TpuClientConfig::default(),
                cache,
            )?
            .send_and_confirm_messages_with_spinner(
                write_messages,
                &[config.signers[0], config.signers[*auth_signer_index]],
            ),
            ConnectionCache::Quic(cache) => {
                let tpu_client_fut =
                    solana_client::nonblocking::tpu_client::TpuClient::new_with_connection_cache(
                        rpc_client.get_inner_client().clone(),
                        &config.websocket_url,
                        solana_client::tpu_client::TpuClientConfig::default(),
                        cache,
                    );
                let tpu_client = (!use_rpc).then(|| {
                    rpc_client
                        .runtime()
                        .block_on(tpu_client_fut)
                        .expect("Should return a valid tpu client")
                });

                send_and_confirm_transactions_in_parallel_blocking_v2(
                    rpc_client.clone(),
                    tpu_client,
                    write_messages,
                    &[config.signers[0], config.signers[*auth_signer_index]],
                    SendAndConfirmConfigV2 {
                        resign_txs_count: Some(5),
                        with_spinner: true,
                        rpc_send_transaction_config: config.send_transaction_config,
                    },
                )
            }
        }
        .map_err(|err| format!("Data writes to account failed: {err}"))?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        if !transaction_errors.is_empty() {
            for transaction_error in &transaction_errors {
                error!("{:?}", transaction_error);
            }
            return Err(format!("{} write transactions failed", transaction_errors.len()).into());
        }
    }

    for message in final_messages {
        let blockhash = rpc_client.get_latest_blockhash()?;
        let mut final_tx = Transaction::new_unsigned(message.clone());
        final_tx.try_sign(
            &[config.signers[0], config.signers[*auth_signer_index]],
            blockhash,
        )?;
        rpc_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &final_tx,
                config.commitment,
                config.send_transaction_config,
            )
            .map_err(|e| format!("Deploying program failed: {e}"))?;
    }

    Ok(())
}

fn build_retract_instruction(
    account: &Account,
    buffer_address: &Pubkey,
    authority: &Pubkey,
) -> Result<Option<Instruction>, Box<dyn std::error::Error>> {
    if !loader_v4::check_id(&account.owner) {
        return Err("Buffer account passed is already in use by another program".into());
    }

    if let Ok(LoaderV4State {
        slot: _,
        authority_address_or_next_version,
        status,
    }) = solana_loader_v4_program::get_state(&account.data)
    {
        if authority != authority_address_or_next_version {
            return Err(
                "Program authority does not match with the provided authority address".into(),
            );
        }

        match status {
            Retracted => Ok(None),
            LoaderV4Status::Deployed => Ok(Some(loader_v4::retract(buffer_address, authority))),
            LoaderV4Status::Finalized => Err("Program is immutable".into()),
        }
    } else {
        Err("Program account's state could not be deserialized".into())
    }
}

fn build_truncate_instructions(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    auth_signer_index: &SignerIndex,
    account: &Account,
    buffer_address: &Pubkey,
    program_data_length: u32,
) -> Result<(Vec<Instruction>, u64), Box<dyn std::error::Error>> {
    if !loader_v4::check_id(&account.owner) {
        return Err("Buffer account passed is already in use by another program".into());
    }

    let payer_pubkey = config.signers[0].pubkey();
    let authority_pubkey = config.signers[*auth_signer_index].pubkey();

    let truncate_instruction = if account.data.is_empty() {
        loader_v4::truncate_uninitialized(
            buffer_address,
            &authority_pubkey,
            program_data_length,
            &payer_pubkey,
        )
    } else {
        if let Ok(LoaderV4State {
            slot: _,
            authority_address_or_next_version,
            status,
        }) = solana_loader_v4_program::get_state(&account.data)
        {
            if &authority_pubkey != authority_address_or_next_version {
                return Err(
                    "Program authority does not match with the provided authority address".into(),
                );
            }

            if matches!(status, LoaderV4Status::Finalized) {
                return Err("Program is immutable and it cannot be truncated".into());
            }
        } else {
            return Err("Program account's state could not be deserialized".into());
        }

        loader_v4::truncate(
            buffer_address,
            &authority_pubkey,
            program_data_length,
            &payer_pubkey,
        )
    };

    let expected_account_data_len =
        LoaderV4State::program_data_offset().saturating_add(program_data_length as usize);

    let lamports_required =
        rpc_client.get_minimum_balance_for_rent_exemption(expected_account_data_len)?;

    match account.data.len().cmp(&expected_account_data_len) {
        Ordering::Less => {
            if account.lamports < lamports_required {
                let extra_lamports_required = lamports_required.saturating_sub(account.lamports);
                Ok((
                    vec![
                        system_instruction::transfer(
                            &payer_pubkey,
                            buffer_address,
                            extra_lamports_required,
                        ),
                        truncate_instruction,
                    ],
                    extra_lamports_required,
                ))
            } else {
                Ok((vec![truncate_instruction], 0))
            }
        }
        Ordering::Equal => {
            if account.lamports < lamports_required {
                return Err("Program account has less lamports than required for its size".into());
            }
            Ok((vec![], 0))
        }
        Ordering::Greater => {
            if account.lamports < lamports_required {
                return Err("Program account has less lamports than required for its size".into());
            }
            Ok((vec![truncate_instruction], 0))
        }
    }
}

fn get_accounts_with_filter(
    rpc_client: Arc<RpcClient>,
    _config: &CliConfig,
    filters: Vec<RpcFilterType>,
    length: usize,
) -> Result<Vec<(Pubkey, Account)>, Box<dyn std::error::Error>> {
    let results = rpc_client.get_program_accounts_with_config(
        &loader_v4::id(),
        RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                data_slice: Some(UiDataSliceConfig { offset: 0, length }),
                ..RpcAccountInfoConfig::default()
            },
            ..RpcProgramAccountsConfig::default()
        },
    )?;
    Ok(results)
}

fn get_programs(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    authority_pubkey: Option<Pubkey>,
) -> Result<CliProgramsV4, Box<dyn std::error::Error>> {
    let filters = if let Some(authority_pubkey) = authority_pubkey {
        vec![
            (RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                size_of::<u64>(),
                authority_pubkey.as_ref(),
            ))),
        ]
    } else {
        vec![]
    };

    let results = get_accounts_with_filter(
        rpc_client,
        config,
        filters,
        LoaderV4State::program_data_offset(),
    )?;

    let mut programs = vec![];
    for (program, account) in results.iter() {
        if let Ok(state) = solana_loader_v4_program::get_state(&account.data) {
            let status = match state.status {
                LoaderV4Status::Retracted => "retracted",
                LoaderV4Status::Deployed => "deployed",
                LoaderV4Status::Finalized => "finalized",
            };
            programs.push(CliProgramV4 {
                program_id: program.to_string(),
                owner: account.owner.to_string(),
                authority: state.authority_address_or_next_version.to_string(),
                last_deploy_slot: state.slot,
                status: status.to_string(),
                data_len: account
                    .data
                    .len()
                    .saturating_sub(LoaderV4State::program_data_offset()),
            });
        } else {
            return Err(format!("Error parsing Program account {program}").into());
        }
    }
    Ok(CliProgramsV4 { programs })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{clap_app::get_clap_app, cli::parse_command},
        serde_json::json,
        solana_rpc_client_api::{
            request::RpcRequest,
            response::{Response, RpcResponseContext},
        },
        solana_sdk::signature::{
            keypair_from_seed, read_keypair_file, write_keypair_file, Keypair,
        },
        std::collections::HashMap,
    };

    fn program_authority() -> solana_sdk::signature::Keypair {
        keypair_from_seed(&[3u8; 32]).unwrap()
    }

    fn rpc_client_no_existing_program() -> RpcClient {
        RpcClient::new_mock("succeeds".to_string())
    }

    fn rpc_client_with_program_data(data: &str, loader_is_owner: bool) -> RpcClient {
        let owner = if loader_is_owner {
            "LoaderV411111111111111111111111111111111111"
        } else {
            "Vote111111111111111111111111111111111111111"
        };
        let account_info_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!({
                "data": [data, "base64"],
                "lamports": 42,
                "owner": owner,
                "executable": true,
                "rentEpoch": 1,
            }),
        });
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, account_info_response);
        RpcClient::new_mock_with_mocks("".to_string(), mocks)
    }

    fn rpc_client_wrong_account_owner() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QAAAAAAAAAA",
            false,
        )
    }

    fn rpc_client_wrong_authority() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            true,
        )
    }

    fn rpc_client_with_program_retracted() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QAAAAAAAAAA",
            true,
        )
    }

    fn rpc_client_with_program_deployed() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QEAAAAAAAAA",
            true,
        )
    }

    fn rpc_client_with_program_finalized() -> RpcClient {
        rpc_client_with_program_data(
            "AAAAAAAAAADtSSjGKNHCxurpAziQWZVhKVknOlxj+TY2wUYUrIc30QIAAAAAAAAA",
            true,
        )
    }

    #[test]
    fn test_deploy() {
        let mut config = CliConfig::default();
        let mut program_data = Vec::new();
        let mut file = File::open("tests/fixtures/noop.so").unwrap();
        file.read_to_end(&mut program_data).unwrap();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();

        config.signers.push(&payer);
        config.signers.push(&authority_signer);

        assert!(process_deploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &1,
            &program_signer.pubkey(),
            &program_data,
            None..None,
            Some(&program_signer),
            true,
        )
        .is_ok());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &1,
            &program_signer.pubkey(),
            &program_data,
            None..None,
            Some(&program_signer),
            true,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &1,
            &program_signer.pubkey(),
            &program_data,
            None..None,
            Some(&program_signer),
            true,
        )
        .is_err());
    }

    #[test]
    fn test_redeploy() {
        let mut config = CliConfig::default();
        let mut program_data = Vec::new();
        let mut file = File::open("tests/fixtures/noop.so").unwrap();
        file.read_to_end(&mut program_data).unwrap();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_address = Pubkey::new_unique();
        let authority_signer = program_authority();

        config.signers.push(&payer);
        config.signers.push(&authority_signer);

        // Redeploying a non-existent program should fail
        assert!(process_deploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            None,
            true,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_retracted()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            None,
            true,
        )
        .is_ok());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            None,
            true,
        )
        .is_ok());

        assert!(process_deploy_program(
            Arc::new(rpc_client_with_program_finalized()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            None,
            true,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            None,
            true,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_authority()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            None,
            true,
        )
        .is_err());
    }

    #[test]
    fn test_redeploy_from_source() {
        let mut config = CliConfig::default();
        let mut program_data = Vec::new();
        let mut file = File::open("tests/fixtures/noop.so").unwrap();
        file.read_to_end(&mut program_data).unwrap();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let buffer_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let program_address = Pubkey::new_unique();
        let authority_signer = program_authority();

        config.signers.push(&payer);
        config.signers.push(&authority_signer);

        // Redeploying a non-existent program should fail
        assert!(process_deploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            Some(&buffer_signer),
            true,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            Some(&buffer_signer),
            true,
        )
        .is_err());

        assert!(process_deploy_program(
            Arc::new(rpc_client_wrong_authority()),
            &config,
            &1,
            &program_address,
            &program_data,
            None..None,
            Some(&buffer_signer),
            true,
        )
        .is_err());
    }

    #[test]
    fn test_undeploy() {
        let mut config = CliConfig::default();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();

        config.signers.push(&payer);
        config.signers.push(&authority_signer);

        assert!(process_undeploy_program(
            Arc::new(rpc_client_no_existing_program()),
            &config,
            &1,
            &program_signer.pubkey(),
        )
        .is_err());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_with_program_retracted()),
            &config,
            &1,
            &program_signer.pubkey(),
        )
        .is_ok());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &1,
            &program_signer.pubkey(),
        )
        .is_ok());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_with_program_finalized()),
            &config,
            &1,
            &program_signer.pubkey(),
        )
        .is_err());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_wrong_account_owner()),
            &config,
            &1,
            &program_signer.pubkey(),
        )
        .is_err());

        assert!(process_undeploy_program(
            Arc::new(rpc_client_wrong_authority()),
            &config,
            &1,
            &program_signer.pubkey(),
        )
        .is_err());
    }

    #[test]
    fn test_transfer_authority() {
        let mut config = CliConfig::default();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();
        let new_authority_signer = program_authority();

        config.signers.push(&payer);
        config.signers.push(&authority_signer);
        config.signers.push(&new_authority_signer);

        assert!(process_transfer_authority_of_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &1,
            &program_signer.pubkey(),
            &new_authority_signer,
        )
        .is_ok());
    }

    #[test]
    fn test_finalize() {
        let mut config = CliConfig::default();

        let payer = keypair_from_seed(&[1u8; 32]).unwrap();
        let program_signer = keypair_from_seed(&[2u8; 32]).unwrap();
        let authority_signer = program_authority();
        let next_version_signer = keypair_from_seed(&[4u8; 32]).unwrap();

        config.signers.push(&payer);
        config.signers.push(&authority_signer);
        config.signers.push(&next_version_signer);

        assert!(process_finalize_program(
            Arc::new(rpc_client_with_program_deployed()),
            &config,
            &1,
            &program_signer.pubkey(),
            &next_version_signer,
        )
        .is_ok());
    }

    fn make_tmp_path(name: &str) -> String {
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();

        let path = format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey());

        // whack any possible collision
        let _ignored = std::fs::remove_dir_all(&path);
        // whack any possible collision
        let _ignored = std::fs::remove_file(&path);

        path
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_deploy() {
        let test_commands = get_clap_app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner::new("", &keypair_file);

        let program_keypair = Keypair::new();
        let program_keypair_file = make_tmp_path("program_keypair_file");
        write_keypair_file(&program_keypair, &program_keypair_file).unwrap();

        let buffer_keypair = Keypair::new();
        let buffer_keypair_file = make_tmp_path("buffer_keypair_file");
        write_keypair_file(&buffer_keypair, &buffer_keypair_file).unwrap();

        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "deploy",
            "/Users/test/program.so",
            "--program",
            &program_keypair_file,
            "--authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Deploy {
                    program_address: None,
                    program_signer_index: Some(1),
                    buffer_signer_index: None,
                    authority_signer_index: 2,
                    program_location: "/Users/test/program.so".to_string(),
                    upload_range: None..None,
                    use_rpc: false,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&program_keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap())
                ],
            }
        );

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "deploy",
            "/Users/test/program.so",
            "--program-id",
            &program_keypair_file,
            "--authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Deploy {
                    program_address: Some(program_keypair.pubkey()),
                    program_signer_index: None,
                    buffer_signer_index: None,
                    authority_signer_index: 1,
                    program_location: "/Users/test/program.so".to_string(),
                    upload_range: None..None,
                    use_rpc: false,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap())
                ],
            }
        );

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "deploy",
            "/Users/test/program.so",
            "--program-id",
            &program_keypair_file,
            "--buffer",
            &buffer_keypair_file,
            "--authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Deploy {
                    program_address: Some(program_keypair.pubkey()),
                    program_signer_index: None,
                    buffer_signer_index: Some(1),
                    authority_signer_index: 2,
                    program_location: "/Users/test/program.so".to_string(),
                    upload_range: None..None,
                    use_rpc: false,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&buffer_keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap())
                ],
            }
        );

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "deploy",
            "/Users/test/program.so",
            "--start-offset",
            "16",
            "--end-offset",
            "32",
            "--program-id",
            &program_keypair_file,
            "--buffer",
            &buffer_keypair_file,
            "--authority",
            &authority_keypair_file,
            "--use-rpc",
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Deploy {
                    program_address: Some(program_keypair.pubkey()),
                    program_signer_index: None,
                    buffer_signer_index: Some(1),
                    authority_signer_index: 2,
                    program_location: "/Users/test/program.so".to_string(),
                    upload_range: Some(16)..Some(32),
                    use_rpc: true,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&buffer_keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap())
                ],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_undeploy() {
        let test_commands = get_clap_app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner::new("", &keypair_file);

        let program_keypair = Keypair::new();
        let program_keypair_file = make_tmp_path("program_keypair_file");
        write_keypair_file(&program_keypair, &program_keypair_file).unwrap();

        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "undeploy",
            "--program-id",
            &program_keypair_file,
            "--authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Undeploy {
                    program_address: program_keypair.pubkey(),
                    authority_signer_index: 1,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap())
                ],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_transfer_authority() {
        let test_commands = get_clap_app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner::new("", &keypair_file);

        let program_keypair = Keypair::new();
        let program_keypair_file = make_tmp_path("program_keypair_file");
        write_keypair_file(&program_keypair, &program_keypair_file).unwrap();

        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();

        let new_authority_keypair = Keypair::new();
        let new_authority_keypair_file = make_tmp_path("new_authority_keypair_file");
        write_keypair_file(&new_authority_keypair, &new_authority_keypair_file).unwrap();

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "transfer-authority",
            "--program-id",
            &program_keypair_file,
            "--authority",
            &authority_keypair_file,
            "--new-authority",
            &new_authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::TransferAuthority {
                    program_address: program_keypair.pubkey(),
                    authority_signer_index: 1,
                    new_authority_signer_index: 2,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap()),
                    Box::new(read_keypair_file(&new_authority_keypair_file).unwrap()),
                ],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_finalize() {
        let test_commands = get_clap_app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner::new("", &keypair_file);

        let program_keypair = Keypair::new();
        let program_keypair_file = make_tmp_path("program_keypair_file");
        write_keypair_file(&program_keypair, &program_keypair_file).unwrap();

        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();

        let next_version_keypair = Keypair::new();
        let next_version_keypair_file = make_tmp_path("next_version_keypair_file");
        write_keypair_file(&next_version_keypair, &next_version_keypair_file).unwrap();

        let test_command = test_commands.clone().get_matches_from(vec![
            "test",
            "program-v4",
            "finalize",
            "--program-id",
            &program_keypair_file,
            "--authority",
            &authority_keypair_file,
            "--next-version",
            &next_version_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_command, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ProgramV4(ProgramV4CliCommand::Finalize {
                    program_address: program_keypair.pubkey(),
                    authority_signer_index: 1,
                    next_version_signer_index: 2,
                }),
                signers: vec![
                    Box::new(read_keypair_file(&keypair_file).unwrap()),
                    Box::new(read_keypair_file(&authority_keypair_file).unwrap()),
                    Box::new(read_keypair_file(&next_version_keypair_file).unwrap()),
                ],
            }
        );
    }
}
