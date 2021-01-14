use crate::send_tpu::{get_leader_tpu, send_transaction_tpu};
use crate::{
    checks::*,
    cli::{
        log_instruction_custom_error, CliCommand, CliCommandInfo, CliConfig, CliError,
        ProcessResult,
    },
};
use bincode::serialize;
use bip39::{Language, Mnemonic, MnemonicType, Seed};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use log::*;
use serde_json::{self, json};
use solana_bpf_loader_program::{bpf_verifier, BPFError, ThisInstructionMeter};
use solana_clap_utils::{
    self, commitment::commitment_arg_with_default, input_parsers::*, input_validators::*,
    keypair::*,
};
use solana_cli_output::display::new_spinner_progress_bar;
use solana_client::{
    rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig,
    rpc_request::MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS, rpc_response::RpcLeaderSchedule,
};
use solana_rbpf::vm::{Config, Executable};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{
    account::Account,
    account_utils::StateMut,
    bpf_loader, bpf_loader_deprecated,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    clock::Slot,
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    loader_instruction,
    message::Message,
    native_token::Sol,
    pubkey::Pubkey,
    signature::{keypair_from_seed, read_keypair_file, Keypair, Signer},
    signers::Signers,
    system_instruction::{self, SystemError},
    system_program,
    transaction::Transaction,
};
use solana_transaction_status::TransactionConfirmationStatus;
use std::{
    cmp::min, collections::HashMap, error, fs::File, io::Read, net::UdpSocket, path::PathBuf,
    sync::Arc, thread::sleep, time::Duration,
};

const DATA_CHUNK_SIZE: usize = 229; // Keep program chunks under PACKET_DATA_SIZE

#[derive(Debug, PartialEq)]
pub enum ProgramCliCommand {
    Deploy {
        program_location: Option<String>,
        program_signer_index: Option<SignerIndex>,
        program_pubkey: Option<Pubkey>,
        buffer_signer_index: Option<SignerIndex>,
        buffer_pubkey: Option<Pubkey>,
        upgrade_authority_signer_index: Option<SignerIndex>,
        upgrade_authority_pubkey: Option<Pubkey>,
        is_final: bool,
        max_len: Option<usize>,
        allow_excessive_balance: bool,
    },
    WriteBuffer {
        program_location: String,
        buffer_signer_index: Option<SignerIndex>,
        buffer_pubkey: Option<Pubkey>,
        buffer_authority_signer_index: Option<SignerIndex>,
        is_final: bool,
        max_len: Option<usize>,
    },
    SetBufferAuthority {
        buffer_pubkey: Pubkey,
        buffer_authority_index: Option<SignerIndex>,
        new_buffer_authority: Option<Pubkey>,
    },
    SetUpgradeAuthority {
        program_pubkey: Pubkey,
        upgrade_authority_index: Option<SignerIndex>,
        new_upgrade_authority: Option<Pubkey>,
    },
    GetAuthority {
        account_pubkey: Option<Pubkey>,
    },
}

pub trait ProgramSubCommands {
    fn program_subcommands(self) -> Self;
}

impl ProgramSubCommands for App<'_, '_> {
    fn program_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("program")
                .about("Program management")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("deploy")
                        .about("Deploy a program")
                        .arg(
                            Arg::with_name("program_location")
                                .index(1)
                                .value_name("PROGRAM_FILEPATH")
                                .takes_value(true)
                                .help("/path/to/program.so"),
                        )
                        .arg(
                            Arg::with_name("buffer")
                                .long("buffer")
                                .value_name("BUFFER_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .conflicts_with("program_location")
                                .help("Intermediate buffer account to write data to, which can be used to resume a failed deploy \
                                      [default: random address]")
                        )
                        .arg(
                            pubkey!(Arg::with_name("upgrade_authority")
                                .long("upgrade-authority")
                                .value_name("UPGRADE_AUTHORITY"),
                                "Upgrade authority [default: the default configured keypair]"),
                        )
                        .arg(
                            pubkey!(Arg::with_name("program_id")
                                .long("program-id")
                                .value_name("PROGRAM_ID"),
                                "Executable program's address, must be a signer for initial deploys, can be a pubkey for upgrades \
                                [default: address of keypair at /path/to/program-keypair.json if present, otherwise a random address]"),
                        )
                        .arg(
                            Arg::with_name("final")
                                .long("final")
                                .help("The program will not be upgradeable")
                        )
                        .arg(
                            Arg::with_name("max_len")
                                .long("max-len")
                                .value_name("max_len")
                                .takes_value(true)
                                .required(false)
                                .help("Maximum length of the upgradeable program \
                                      [default: twice the length of the original deployed program]")
                        )
                        .arg(
                            Arg::with_name("allow_excessive_balance")
                                .long("allow-excessive-deploy-account-balance")
                                .takes_value(false)
                                .help("Use the designated program id even if the account already holds a large balance of SOL")
                        )
                        .arg(commitment_arg_with_default("singleGossip")),
                )
                .subcommand(
                    SubCommand::with_name("write-buffer")
                        .about("Writes a program into a buffer account")
                        .arg(
                            Arg::with_name("program_location")
                                .index(1)
                                .value_name("PROGRAM_FILEPATH")
                                .takes_value(true)
                                .required(true)
                                .help("/path/to/program.so"),
                        )
                        .arg(
                            Arg::with_name("buffer")
                                .long("buffer")
                                .value_name("BUFFER_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help("Buffer account to write data into [default: random address]")
                        )
                        .arg(
                            Arg::with_name("buffer_authority")
                                .long("buffer-authority")
                                .value_name("BUFFER_AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help("Buffer authority [default: the default configured keypair]")
                        )
                        .arg(
                            Arg::with_name("final")
                                .long("final")
                                .help("The program will not be upgradeable")
                        )
                        .arg(
                            Arg::with_name("max_len")
                                .long("max-len")
                                .value_name("max_len")
                                .takes_value(true)
                                .required(false)
                                .help("Maximum length of the upgradeable program \
                                      [default: twice the length of the original deployed program]")
                        )
                        .arg(commitment_arg_with_default("singleGossip")),
                )
                .subcommand(
                    SubCommand::with_name("set-buffer-authority")
                        .about("Set a new buffer authority") // TODO deploy with buffer and no file path?
                        .arg(
                            Arg::with_name("buffer")
                                .index(1)
                                .value_name("BUFFER_PUBKEY")
                                .takes_value(true)
                                .required(true)
                                .help("Public key of the buffer")
                        )
                        .arg(
                            Arg::with_name("buffer_authority")
                                .long("buffer-authority")
                                .value_name("BUFFER_AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help("Buffer authority [default: the default configured keypair]")
                        )
                        .arg(
                            pubkey!(Arg::with_name("new_buffer_authority")
                                .long("new-buffer-authority")
                                .value_name("NEW_BUFFER_AUTHORITY")
                                .required_unless("final"),
                                "Address of the new buffer authority"),
                        )
                        .arg(
                            Arg::with_name("final")
                                .long("final")
                                .conflicts_with("new_buffer_authority")
                                .help("The buffer will be immutable")
                        )
                )
                .subcommand(
                    SubCommand::with_name("set-upgrade-authority")
                        .about("Set a new program authority")
                        .arg(
                            Arg::with_name("program_id")
                                .index(1)
                                .value_name("PROGRAM_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .help("Public key of the program to upgrade")
                        )
                        .arg(
                            Arg::with_name("upgrade_authority")
                                .long("upgrade-authority")
                                .value_name("UPGRADE_AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help("Upgrade authority [default: the default configured keypair]")
                        )
                        .arg(
                            pubkey!(Arg::with_name("new_upgrade_authority")
                                .long("new-upgrade-authority")
                                .required_unless("final")
                                .value_name("NEW_UPGRADE_AUTHORITY"),
                                "Address of the new upgrade authority"),
                        )
                        .arg(
                            Arg::with_name("final")
                                .long("final")
                                .conflicts_with("new_upgrade_authority")
                                .help("The program will not be upgradeable")
                        )
                )
                .subcommand(
                    SubCommand::with_name("get-authority")
                        .about("Gets a buffer or program account's authority")
                        .arg(
                            Arg::with_name("account")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .help("Public key of the account to query")
                        )
                )
        )
    }
}

pub fn parse_program_subcommand(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let response = match matches.subcommand() {
        ("deploy", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let program_location = if let Some(location) = matches.value_of("program_location") {
                Some(location.to_string())
            } else {
                None
            };

            let buffer_pubkey = if let Ok((buffer_signer, Some(buffer_pubkey))) =
                signer_of(matches, "buffer", wallet_manager)
            {
                bulk_signers.push(buffer_signer);
                Some(buffer_pubkey)
            } else if let Some(buffer_pubkey) = pubkey_of_signer(matches, "buffer", wallet_manager)?
            {
                Some(buffer_pubkey)
            } else {
                None
            };

            let program_pubkey = if let Ok((program_signer, Some(program_pubkey))) =
                signer_of(matches, "program_id", wallet_manager)
            {
                bulk_signers.push(program_signer);
                Some(program_pubkey)
            } else if let Some(program_pubkey) =
                pubkey_of_signer(matches, "program_id", wallet_manager)?
            {
                Some(program_pubkey)
            } else {
                None
            };

            let upgrade_authority_pubkey =
                if let Ok((upgrade_authority_signer, Some(upgrade_authority_pubkey))) =
                    signer_of(matches, "upgrade_authority", wallet_manager)
                {
                    bulk_signers.push(upgrade_authority_signer);
                    Some(upgrade_authority_pubkey)
                } else if let Some(upgrade_authority_pubkey) =
                    pubkey_of_signer(matches, "upgrade_authority", wallet_manager)?
                {
                    Some(upgrade_authority_pubkey)
                } else {
                    Some(
                        default_signer
                            .signer_from_path(matches, wallet_manager)?
                            .pubkey(),
                    )
                };

            let max_len = value_of(matches, "max_len");

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;

            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location,
                    program_signer_index: signer_info.index_of_or_none(program_pubkey),
                    program_pubkey,
                    buffer_signer_index: signer_info.index_of_or_none(buffer_pubkey),
                    buffer_pubkey,
                    upgrade_authority_signer_index: signer_info
                        .index_of_or_none(upgrade_authority_pubkey),
                    upgrade_authority_pubkey,
                    is_final: matches.is_present("final"),
                    max_len,
                    allow_excessive_balance: matches.is_present("allow_excessive_balance"),
                }),
                signers: signer_info.signers,
            }
        }
        ("write-buffer", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let buffer_pubkey = if let Ok((buffer_signer, Some(buffer_pubkey))) =
                signer_of(matches, "buffer", wallet_manager)
            {
                bulk_signers.push(buffer_signer);
                Some(buffer_pubkey)
            } else if let Some(buffer_pubkey) = pubkey_of_signer(matches, "buffer", wallet_manager)?
            {
                Some(buffer_pubkey)
            } else {
                None
            };

            let buffer_authority_pubkey =
                if let Ok((buffer_authority_signer, Some(buffer_authority_pubkey))) =
                    signer_of(matches, "buffer_authority", wallet_manager)
                {
                    bulk_signers.push(buffer_authority_signer);
                    Some(buffer_authority_pubkey)
                } else {
                    Some(
                        default_signer
                            .signer_from_path(matches, wallet_manager)?
                            .pubkey(),
                    )
                };

            let max_len = value_of(matches, "max_len");

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;

            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: matches.value_of("program_location").unwrap().to_string(),
                    buffer_signer_index: signer_info.index_of_or_none(buffer_pubkey),
                    buffer_pubkey,
                    buffer_authority_signer_index: signer_info
                        .index_of_or_none(buffer_authority_pubkey),
                    is_final: matches.is_present("final"),
                    max_len,
                }),
                signers: signer_info.signers,
            }
        }
        ("set-buffer-authority", Some(matches)) => {
            let buffer_pubkey = pubkey_of(matches, "buffer").unwrap();

            let (buffer_authority_signer, buffer_authority_pubkey) =
                signer_of(matches, "buffer_authority", wallet_manager)?;
            let new_buffer_authority = if matches.is_present("final") {
                None
            } else if let Some(new_buffer_authority) =
                pubkey_of_signer(matches, "new_buffer_authority", wallet_manager)?
            {
                Some(new_buffer_authority)
            } else {
                let (_, new_buffer_authority) =
                    signer_of(matches, "new_buffer_authority", wallet_manager)?;
                new_buffer_authority
            };

            let signer_info = default_signer.generate_unique_signers(
                vec![
                    Some(default_signer.signer_from_path(matches, wallet_manager)?),
                    buffer_authority_signer,
                ],
                matches,
                wallet_manager,
            )?;

            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
                    buffer_pubkey,
                    buffer_authority_index: signer_info.index_of(buffer_authority_pubkey),
                    new_buffer_authority,
                }),
                signers: signer_info.signers,
            }
        }
        ("set-upgrade-authority", Some(matches)) => {
            let (upgrade_authority_signer, upgrade_authority_pubkey) =
                signer_of(matches, "upgrade_authority", wallet_manager)?;
            let program_pubkey = pubkey_of(matches, "program_id").unwrap();
            let new_upgrade_authority = if matches.is_present("final") {
                None
            } else if let Some(new_upgrade_authority) =
                pubkey_of_signer(matches, "new_upgrade_authority", wallet_manager)?
            {
                Some(new_upgrade_authority)
            } else {
                let (_, new_upgrade_authority) =
                    signer_of(matches, "new_upgrade_authority", wallet_manager)?;
                new_upgrade_authority
            };

            let signer_info = default_signer.generate_unique_signers(
                vec![
                    Some(default_signer.signer_from_path(matches, wallet_manager)?),
                    upgrade_authority_signer,
                ],
                matches,
                wallet_manager,
            )?;

            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
                    program_pubkey,
                    upgrade_authority_index: signer_info.index_of(upgrade_authority_pubkey),
                    new_upgrade_authority,
                }),
                signers: signer_info.signers,
            }
        }
        ("get-authority", Some(matches)) => CliCommandInfo {
            command: CliCommand::Program(ProgramCliCommand::GetAuthority {
                account_pubkey: pubkey_of(matches, "account"),
            }),
            signers: vec![],
        },
        _ => unreachable!(),
    };
    Ok(response)
}

pub fn process_program_subcommand(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_subcommand: &ProgramCliCommand,
) -> ProcessResult {
    match program_subcommand {
        ProgramCliCommand::Deploy {
            program_location,
            program_signer_index,
            program_pubkey,
            buffer_signer_index,
            buffer_pubkey,
            upgrade_authority_signer_index,
            upgrade_authority_pubkey,
            is_final,
            max_len,
            allow_excessive_balance,
        } => process_program_deploy(
            &rpc_client,
            config,
            program_location,
            *program_signer_index,
            *program_pubkey,
            *buffer_signer_index,
            *buffer_pubkey,
            *upgrade_authority_signer_index,
            *upgrade_authority_pubkey,
            *is_final,
            *max_len,
            *allow_excessive_balance,
        ),
        ProgramCliCommand::WriteBuffer {
            program_location,
            buffer_signer_index,
            buffer_pubkey,
            buffer_authority_signer_index,
            is_final,
            max_len,
        } => process_write_buffer(
            &rpc_client,
            config,
            program_location,
            *buffer_signer_index,
            *buffer_pubkey,
            *buffer_authority_signer_index,
            *is_final,
            *max_len,
        ),
        ProgramCliCommand::SetBufferAuthority {
            buffer_pubkey,
            buffer_authority_index,
            new_buffer_authority,
        } => process_set_authority(
            &rpc_client,
            config,
            None,
            Some(*buffer_pubkey),
            *buffer_authority_index,
            *new_buffer_authority,
        ),
        ProgramCliCommand::SetUpgradeAuthority {
            program_pubkey,
            upgrade_authority_index,
            new_upgrade_authority,
        } => process_set_authority(
            &rpc_client,
            config,
            Some(*program_pubkey),
            None,
            *upgrade_authority_index,
            *new_upgrade_authority,
        ),
        ProgramCliCommand::GetAuthority { account_pubkey } => {
            process_get_authority(&rpc_client, config, *account_pubkey)
        }
    }
}

fn get_default_program_keypair(program_location: &Option<String>) -> Keypair {
    let program_keypair = {
        if let Some(program_location) = program_location {
            let mut keypair_file = PathBuf::new();
            keypair_file.push(program_location);
            let mut filename = keypair_file.file_stem().unwrap().to_os_string();
            filename.push("-keypair");
            keypair_file.set_file_name(filename);
            keypair_file.set_extension("json");
            if let Ok(keypair) = read_keypair_file(&keypair_file.to_str().unwrap()) {
                keypair
            } else {
                Keypair::new()
            }
        } else {
            Keypair::new()
        }
    };
    program_keypair
}

/// Deploy using upgradeable loader
#[allow(clippy::too_many_arguments)]
fn process_program_deploy(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_location: &Option<String>,
    program_signer_index: Option<SignerIndex>,
    program_pubkey: Option<Pubkey>,
    buffer_signer_index: Option<SignerIndex>,
    buffer_pubkey: Option<Pubkey>,
    upgrade_authority_signer_index: Option<SignerIndex>,
    upgrade_authority_pubkey: Option<Pubkey>,
    is_final: bool,
    max_len: Option<usize>,
    allow_excessive_balance: bool,
) -> ProcessResult {
    let (words, mnemonic, buffer_keypair) = create_ephemeral_keypair()?;
    let (buffer_signer, buffer_pubkey) = if let Some(i) = buffer_signer_index {
        (Some(config.signers[i]), config.signers[i].pubkey())
    } else if let Some(pubkey) = buffer_pubkey {
        (None, pubkey)
    } else {
        (
            Some(&buffer_keypair as &dyn Signer),
            buffer_keypair.pubkey(),
        )
    };

    let default_program_keypair = get_default_program_keypair(&program_location);
    let (program_signer, program_pubkey) = if let Some(i) = program_signer_index {
        (Some(config.signers[i]), config.signers[i].pubkey())
    } else if let Some(program_pubkey) = program_pubkey {
        (None, program_pubkey)
    } else {
        (
            Some(&default_program_keypair as &dyn Signer),
            default_program_keypair.pubkey(),
        )
    };

    let do_deploy = if let Some(account) = rpc_client
        .get_account_with_commitment(&program_pubkey, config.commitment)?
        .value
    {
        if !account.executable {
            // Continue an initial deploy
            true
        } else if let UpgradeableLoaderState::Program {
            programdata_address,
        } = account.state()?
        {
            if let Some(account) = rpc_client
                .get_account_with_commitment(&programdata_address, config.commitment)?
                .value
            {
                if let UpgradeableLoaderState::ProgramData {
                    slot: _,
                    upgrade_authority_address: program_authority_pubkey,
                } = account.state()?
                {
                    if program_authority_pubkey.is_none() {
                        return Err("Program is no longer upgradeable".into());
                    }
                    if program_authority_pubkey != upgrade_authority_pubkey {
                        return Err(format!(
                            "Program's authority {:?} does not match authority provided {:?}",
                            program_authority_pubkey, upgrade_authority_pubkey,
                        )
                        .into());
                    }
                    // Do upgrade
                    false
                } else {
                    return Err("Program account is corrupt".into());
                }
            } else {
                return Err("Program account is corrupt".into());
            }
        } else {
            return Err(
                format!("Program {:?} is not an upgradeable program", program_pubkey).into(),
            );
        }
    } else {
        // do new deploy
        true
    };

    let (program_data, buffer_data_len) = if buffer_signer.is_none() {
        // Check supplied buffer account
        if let Some(account) = rpc_client
            .get_account_with_commitment(&buffer_pubkey, config.commitment)?
            .value
        {
            if let UpgradeableLoaderState::Buffer {
                authority_address: _,
            } = account.state()?
            {
            } else {
                return Err("Buffer account is not initialized".into());
            }
            (vec![], account.data.len())
        } else {
            return Err("Specified buffer not found, was it already consumed?".into());
        }
    } else if let Some(program_location) = program_location {
        let program_data = read_and_verify_elf(&program_location)?;
        let buffer_data_len = if let Some(len) = max_len {
            len
        } else {
            program_data.len() * 2
        };
        (program_data, buffer_data_len)
    } else {
        return Err("Program location required if buffer not supplied".into());
    };
    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::programdata_len(buffer_data_len)?,
    )?;

    let result = if do_deploy {
        do_process_program_write_and_deploy(
            rpc_client,
            config,
            &program_data,
            buffer_data_len,
            minimum_balance,
            &bpf_loader_upgradeable::id(),
            Some(program_signer.unwrap()),
            buffer_signer,
            &buffer_pubkey,
            buffer_signer,
            upgrade_authority_pubkey,
            allow_excessive_balance,
        )
    } else if let Some(upgrade_authority_index) = upgrade_authority_signer_index {
        do_process_program_upgrade(
            rpc_client,
            config,
            &program_data,
            &program_pubkey,
            config.signers[upgrade_authority_index],
            &buffer_pubkey,
            buffer_signer,
        )
    } else {
        return Err("Program upgrade requires an authority".into());
    };
    if result.is_ok() && is_final {
        process_set_authority(
            rpc_client,
            config,
            Some(program_pubkey),
            None,
            upgrade_authority_signer_index,
            None,
        )?;
    }
    if result.is_err() && buffer_signer_index.is_none() {
        report_ephemeral_mnemonic(words, mnemonic);
    }
    result
}

fn process_write_buffer(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_location: &str,
    buffer_signer_index: Option<SignerIndex>,
    buffer_pubkey: Option<Pubkey>,
    buffer_authority_signer_index: Option<SignerIndex>,
    is_final: bool,
    max_len: Option<usize>,
) -> ProcessResult {
    // Create ephemeral keypair to use for Buffer account, if not provided
    let (words, mnemonic, buffer_keypair) = create_ephemeral_keypair()?;
    let (buffer_signer, buffer_pubkey) = if let Some(i) = buffer_signer_index {
        (Some(config.signers[i]), config.signers[i].pubkey())
    } else if let Some(pubkey) = buffer_pubkey {
        (None, pubkey)
    } else {
        (
            Some(&buffer_keypair as &dyn Signer),
            buffer_keypair.pubkey(),
        )
    };
    let buffer_authority = if let Some(i) = buffer_authority_signer_index {
        config.signers[i]
    } else {
        config.signers[0]
    };

    if let Some(account) = rpc_client
        .get_account_with_commitment(&buffer_pubkey, config.commitment)?
        .value
    {
        if let UpgradeableLoaderState::Buffer { authority_address } = account.state()? {
            if authority_address.is_none() {
                return Err("Buffer is immutable".into());
            }
            if authority_address != Some(buffer_authority.pubkey()) {
                return Err(format!(
                    "Buffer's authority {:?} does not match authority provided {:?}",
                    authority_address,
                    buffer_authority.pubkey()
                )
                .into());
            }
        } else {
            return Err("Buffer account is corrupt".into());
        }
    }

    let program_data = read_and_verify_elf(program_location)?;
    let buffer_data_len = if let Some(len) = max_len {
        len
    } else {
        program_data.len() * 2
    };
    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::programdata_len(buffer_data_len)?,
    )?;

    let result = do_process_program_write_and_deploy(
        rpc_client,
        config,
        &program_data,
        program_data.len(),
        minimum_balance,
        &bpf_loader_upgradeable::id(),
        None,
        buffer_signer,
        &buffer_pubkey,
        Some(buffer_authority),
        None,
        true,
    );

    if result.is_ok() && is_final {
        process_set_authority(
            rpc_client,
            config,
            None,
            Some(buffer_pubkey),
            buffer_authority_signer_index,
            None,
        )?;
    }

    if result.is_err() && buffer_signer_index.is_none() && buffer_signer.is_some() {
        report_ephemeral_mnemonic(words, mnemonic);
    }
    result
}

fn process_set_authority(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_pubkey: Option<Pubkey>,
    buffer_pubkey: Option<Pubkey>,
    authority: Option<SignerIndex>,
    new_authority: Option<Pubkey>,
) -> ProcessResult {
    let authority_signer = if let Some(index) = authority {
        config.signers[index]
    } else {
        return Err("Set authority requires the current authority".into());
    };

    trace!("Set a new authority");
    let (blockhash, _, _) = rpc_client
        .get_recent_blockhash_with_commitment(config.commitment)?
        .value;

    let mut tx = if let Some(pubkey) = program_pubkey {
        Transaction::new_unsigned(Message::new(
            &[bpf_loader_upgradeable::set_upgrade_authority(
                &pubkey,
                &authority_signer.pubkey(),
                new_authority.as_ref(),
            )],
            Some(&config.signers[0].pubkey()),
        ))
    } else if let Some(pubkey) = buffer_pubkey {
        Transaction::new_unsigned(Message::new(
            &[bpf_loader_upgradeable::set_buffer_authority(
                &pubkey,
                &authority_signer.pubkey(),
                new_authority.as_ref(),
            )],
            Some(&config.signers[0].pubkey()),
        ))
    } else {
        return Err("Program or Buffer not provided".into());
    };

    tx.try_sign(&[config.signers[0], authority_signer], blockhash)?;
    rpc_client
        .send_and_confirm_transaction_with_spinner_and_config(
            &tx,
            config.commitment,
            RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: Some(config.commitment.commitment),
                ..RpcSendTransactionConfig::default()
            },
        )
        .map_err(|e| format!("Setting authority failed: {}", e))?;

    Ok(option_pubkey_to_string("Authority", new_authority))
}

fn process_get_authority(
    rpc_client: &RpcClient,
    config: &CliConfig,
    account_pubkey: Option<Pubkey>,
) -> ProcessResult {
    if let Some(account_pubkey) = account_pubkey {
        if let Some(account) = rpc_client
            .get_account_with_commitment(&account_pubkey, config.commitment)?
            .value
        {
            if let Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) = account.state()
            {
                if let Some(account) = rpc_client
                    .get_account_with_commitment(&programdata_address, config.commitment)?
                    .value
                {
                    if let Ok(UpgradeableLoaderState::ProgramData {
                        upgrade_authority_address,
                        ..
                    }) = account.state()
                    {
                        Ok(option_pubkey_to_string(
                            "Program upgrade authority",
                            upgrade_authority_address,
                        ))
                    } else {
                        Err("Invalid associated ProgramData account found for the program".into())
                    }
                } else {
                    Err(
                        "Failed to find associated ProgramData account for the provided program"
                            .into(),
                    )
                }
            } else if let Ok(UpgradeableLoaderState::Buffer { authority_address }) = account.state()
            {
                Ok(option_pubkey_to_string(
                    "Buffer authority",
                    authority_address,
                ))
            } else {
                Err("Not a buffer or program account".into())
            }
        } else {
            Err("Unable to find the account".into())
        }
    } else {
        Err("No account specified".into())
    }
}

/// Deploy using non-upgradeable loader
pub fn process_deploy(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_location: &str,
    buffer_signer_index: Option<SignerIndex>,
    use_deprecated_loader: bool,
    allow_excessive_balance: bool,
) -> ProcessResult {
    // Create ephemeral keypair to use for Buffer account, if not provided
    let (words, mnemonic, buffer_keypair) = create_ephemeral_keypair()?;
    let buffer_signer = if let Some(i) = buffer_signer_index {
        config.signers[i]
    } else {
        &buffer_keypair
    };

    let program_data = read_and_verify_elf(program_location)?;
    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(program_data.len())?;
    let loader_id = if use_deprecated_loader {
        bpf_loader_deprecated::id()
    } else {
        bpf_loader::id()
    };

    let result = do_process_program_write_and_deploy(
        rpc_client,
        config,
        &program_data,
        program_data.len(),
        minimum_balance,
        &loader_id,
        Some(buffer_signer),
        Some(buffer_signer),
        &buffer_signer.pubkey(),
        Some(buffer_signer),
        None,
        allow_excessive_balance,
    );
    if result.is_err() && buffer_signer_index.is_none() {
        report_ephemeral_mnemonic(words, mnemonic);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn do_process_program_write_and_deploy(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_data: &[u8],
    buffer_data_len: usize,
    minimum_balance: u64,
    loader_id: &Pubkey,
    program_signer: Option<&dyn Signer>,
    buffer_signer: Option<&dyn Signer>,
    buffer_pubkey: &Pubkey,
    buffer_authority_signer: Option<&dyn Signer>,
    upgrade_authority: Option<Pubkey>,
    allow_excessive_balance: bool,
) -> ProcessResult {
    // Build messages to calculate fees
    let mut messages: Vec<&Message> = Vec::new();

    // Initialize buffer account or complete if already partially initialized
    let (initial_message, write_messages, balance_needed) =
        if let Some(buffer_authority_signer) = buffer_authority_signer {
            let (initial_instructions, balance_needed) = if let Some(account) = rpc_client
                .get_account_with_commitment(buffer_pubkey, config.commitment)?
                .value
            {
                complete_partial_program_init(
                    &loader_id,
                    &config.signers[0].pubkey(),
                    buffer_pubkey,
                    &account,
                    if loader_id == &bpf_loader_upgradeable::id() {
                        UpgradeableLoaderState::buffer_len(buffer_data_len)?
                    } else {
                        buffer_data_len
                    },
                    minimum_balance,
                    allow_excessive_balance,
                )?
            } else if loader_id == &bpf_loader_upgradeable::id() {
                (
                    bpf_loader_upgradeable::create_buffer(
                        &config.signers[0].pubkey(),
                        buffer_pubkey,
                        Some(&buffer_authority_signer.pubkey()),
                        minimum_balance,
                        buffer_data_len,
                    )?,
                    minimum_balance,
                )
            } else {
                (
                    vec![system_instruction::create_account(
                        &config.signers[0].pubkey(),
                        buffer_pubkey,
                        minimum_balance,
                        buffer_data_len as u64,
                        &loader_id,
                    )],
                    minimum_balance,
                )
            };
            let initial_message = if !initial_instructions.is_empty() {
                Some(Message::new(
                    &initial_instructions,
                    Some(&config.signers[0].pubkey()),
                ))
            } else {
                None
            };

            // Create and add write messages

            let mut write_messages = vec![];
            for (chunk, i) in program_data.chunks(DATA_CHUNK_SIZE).zip(0..) {
                let instruction = if loader_id == &bpf_loader_upgradeable::id() {
                    bpf_loader_upgradeable::write(
                        buffer_pubkey,
                        Some(&buffer_authority_signer.pubkey()),
                        (i * DATA_CHUNK_SIZE) as u32,
                        chunk.to_vec(),
                    )
                } else {
                    loader_instruction::write(
                        buffer_pubkey,
                        &loader_id,
                        (i * DATA_CHUNK_SIZE) as u32,
                        chunk.to_vec(),
                    )
                };
                let message = Message::new(&[instruction], Some(&config.signers[0].pubkey()));
                write_messages.push(message);
            }

            (initial_message, Some(write_messages), balance_needed)
        } else {
            (None, None, 0)
        };

    if let Some(ref initial_message) = initial_message {
        messages.push(initial_message);
    }
    if let Some(ref write_messages) = write_messages {
        let mut write_message_refs = vec![];
        for message in write_messages.iter() {
            write_message_refs.push(message);
        }
        messages.append(&mut write_message_refs);
    }

    // Create and add final message

    let final_message = if let Some(program_signer) = program_signer {
        let message = if loader_id == &bpf_loader_upgradeable::id() {
            Message::new(
                &bpf_loader_upgradeable::deploy_with_max_program_len(
                    &config.signers[0].pubkey(),
                    &program_signer.pubkey(),
                    buffer_pubkey,
                    upgrade_authority.as_ref(),
                    rpc_client.get_minimum_balance_for_rent_exemption(
                        UpgradeableLoaderState::program_len()?,
                    )?,
                    buffer_data_len,
                )?,
                Some(&config.signers[0].pubkey()),
            )
        } else {
            Message::new(
                &[loader_instruction::finalize(buffer_pubkey, &loader_id)],
                Some(&config.signers[0].pubkey()),
            )
        };
        Some(message)
    } else {
        None
    };
    if let Some(ref message) = final_message {
        messages.push(message);
    }

    check_payer(rpc_client, config, balance_needed, &messages)?;

    send_deploy_messages(
        rpc_client,
        config,
        &initial_message,
        &write_messages,
        &final_message,
        buffer_signer,
        buffer_authority_signer,
        program_signer,
    )?;

    if let Some(program_signer) = program_signer {
        Ok(json!({
            "ProgramId": format!("{}", program_signer.pubkey()),
        })
        .to_string())
    } else {
        Ok(json!({
            "Buffer": format!("{}", buffer_pubkey),
        })
        .to_string())
    }
}

fn do_process_program_upgrade(
    rpc_client: &RpcClient,
    config: &CliConfig,
    program_data: &[u8],
    program_id: &Pubkey,
    upgrade_authority: &dyn Signer,
    buffer_pubkey: &Pubkey,
    buffer_signer: Option<&dyn Signer>,
) -> ProcessResult {
    let loader_id = bpf_loader_upgradeable::id();
    let data_len = program_data.len();
    let minimum_balance = rpc_client.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::programdata_len(data_len)?,
    )?;

    // Build messages to calculate fees
    let mut messages: Vec<&Message> = Vec::new();

    let (initial_message, write_messages, balance_needed) =
        if let Some(buffer_signer) = buffer_signer {
            // Check Buffer account to see if partial initialization has occurred
            let (initial_instructions, balance_needed) = if let Some(account) = rpc_client
                .get_account_with_commitment(&buffer_signer.pubkey(), config.commitment)?
                .value
            {
                complete_partial_program_init(
                    &loader_id,
                    &config.signers[0].pubkey(),
                    &buffer_signer.pubkey(),
                    &account,
                    UpgradeableLoaderState::buffer_len(data_len)?,
                    minimum_balance,
                    true,
                )?
            } else {
                (
                    bpf_loader_upgradeable::create_buffer(
                        &config.signers[0].pubkey(),
                        buffer_pubkey,
                        Some(&buffer_signer.pubkey()),
                        minimum_balance,
                        data_len,
                    )?,
                    minimum_balance,
                )
            };

            let initial_message = if !initial_instructions.is_empty() {
                Some(Message::new(
                    &initial_instructions,
                    Some(&config.signers[0].pubkey()),
                ))
            } else {
                None
            };

            // Create and add write messages
            let mut write_messages = vec![];
            for (chunk, i) in program_data.chunks(DATA_CHUNK_SIZE).zip(0..) {
                let instruction = bpf_loader_upgradeable::write(
                    &buffer_signer.pubkey(),
                    None,
                    (i * DATA_CHUNK_SIZE) as u32,
                    chunk.to_vec(),
                );
                let message = Message::new(&[instruction], Some(&config.signers[0].pubkey()));
                write_messages.push(message);
            }

            (initial_message, Some(write_messages), balance_needed)
        } else {
            (None, None, 0)
        };

    if let Some(ref message) = initial_message {
        messages.push(message);
    }
    if let Some(ref write_messages) = write_messages {
        let mut write_message_refs = vec![];
        for message in write_messages.iter() {
            write_message_refs.push(message);
        }
        messages.append(&mut write_message_refs);
    }

    // Create and add final message
    let final_message = Message::new(
        &[bpf_loader_upgradeable::upgrade(
            &program_id,
            &buffer_pubkey,
            &upgrade_authority.pubkey(),
            &config.signers[0].pubkey(),
        )],
        Some(&config.signers[0].pubkey()),
    );
    messages.push(&final_message);

    check_payer(rpc_client, config, balance_needed, &messages)?;
    send_deploy_messages(
        rpc_client,
        config,
        &initial_message,
        &write_messages,
        &Some(final_message),
        buffer_signer,
        buffer_signer,
        Some(upgrade_authority),
    )?;

    Ok(json!({
        "programId": format!("{}", program_id),
    })
    .to_string())
}

fn read_and_verify_elf(program_location: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut file = File::open(program_location)
        .map_err(|err| format!("Unable to open program file: {}", err))?;
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data)
        .map_err(|err| format!("Unable to read program file: {}", err))?;

    // Verify the program
    Executable::<BPFError, ThisInstructionMeter>::from_elf(
        &program_data,
        Some(|x| bpf_verifier::check(x, false)),
        Config::default(),
    )
    .map_err(|err| format!("ELF error: {}", err))?;

    Ok(program_data)
}

fn complete_partial_program_init(
    loader_id: &Pubkey,
    payer_pubkey: &Pubkey,
    elf_pubkey: &Pubkey,
    account: &Account,
    account_data_len: usize,
    minimum_balance: u64,
    allow_excessive_balance: bool,
) -> Result<(Vec<Instruction>, u64), Box<dyn std::error::Error>> {
    let mut instructions: Vec<Instruction> = vec![];
    let mut balance_needed = 0;
    if account.executable {
        return Err("Buffer account is already executable".into());
    }
    if account.owner != *loader_id && !system_program::check_id(&account.owner) {
        return Err("Buffer account is already owned by another account".into());
    }

    if account.data.is_empty() && system_program::check_id(&account.owner) {
        instructions.push(system_instruction::allocate(
            elf_pubkey,
            account_data_len as u64,
        ));
        if account.owner != *loader_id {
            instructions.push(system_instruction::assign(elf_pubkey, &loader_id));
        }
    }
    if account.lamports < minimum_balance {
        let balance = minimum_balance - account.lamports;
        instructions.push(system_instruction::transfer(
            payer_pubkey,
            elf_pubkey,
            balance,
        ));
        balance_needed = balance;
    } else if account.lamports > minimum_balance
        && system_program::check_id(&account.owner)
        && !allow_excessive_balance
    {
        return Err(format!(
            "Buffer account has a balance: {:?}; it may already be in use",
            Sol(account.lamports)
        )
        .into());
    }
    Ok((instructions, balance_needed))
}

fn check_payer(
    rpc_client: &RpcClient,
    config: &CliConfig,
    balance_needed: u64,
    messages: &[&Message],
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, fee_calculator, _) = rpc_client
        .get_recent_blockhash_with_commitment(config.commitment)?
        .value;

    // Does the payer have enough?
    check_account_for_spend_multiple_fees_with_commitment(
        rpc_client,
        &config.signers[0].pubkey(),
        balance_needed,
        &fee_calculator,
        messages,
        config.commitment,
    )?;
    Ok(())
}

fn send_deploy_messages(
    rpc_client: &RpcClient,
    config: &CliConfig,
    initial_message: &Option<Message>,
    write_messages: &Option<Vec<Message>>,
    final_message: &Option<Message>,
    initial_signer: Option<&dyn Signer>,
    write_signer: Option<&dyn Signer>,
    final_signer: Option<&dyn Signer>,
) -> Result<(), Box<dyn std::error::Error>> {
    let payer_signer = config.signers[0];
    if let Some(message) = initial_message {
        if let Some(initial_signer) = initial_signer {
            trace!("Preparing the required accounts");
            let (blockhash, _, _) = rpc_client
                .get_recent_blockhash_with_commitment(config.commitment)?
                .value;

            let mut initial_transaction = Transaction::new_unsigned(message.clone());
            // Most of the initial_transaction combinations require both the fee-payer and new program
            // account to sign the transaction. One (transfer) only requires the fee-payer signature.
            // This check is to ensure signing does not fail on a KeypairPubkeyMismatch error from an
            // extraneous signature.
            if message.header.num_required_signatures == 2 {
                initial_transaction.try_sign(&[payer_signer, initial_signer], blockhash)?;
            } else {
                initial_transaction.try_sign(&[payer_signer], blockhash)?;
            }
            let result = rpc_client.send_and_confirm_transaction_with_spinner_and_config(
                &initial_transaction,
                config.commitment,
                config.send_transaction_config,
            );
            log_instruction_custom_error::<SystemError>(result, &config)
                .map_err(|err| format!("Account allocation failed: {}", err))?;
        } else {
            return Err("Buffer account not created yet, must provide a key pair".into());
        }
    }

    if let Some(write_messages) = write_messages {
        if let Some(write_signer) = write_signer {
            trace!("Writing program data");
            let (blockhash, _, last_valid_slot) = rpc_client
                .get_recent_blockhash_with_commitment(config.commitment)?
                .value;
            let mut write_transactions = vec![];
            for message in write_messages.iter() {
                let mut tx = Transaction::new_unsigned(message.clone());
                tx.try_sign(&[payer_signer, write_signer], blockhash)?;
                write_transactions.push(tx);
            }

            send_and_confirm_transactions_with_spinner(
                &rpc_client,
                write_transactions,
                &[payer_signer, write_signer],
                config.commitment,
                last_valid_slot,
            )
            .map_err(|err| format!("Data writes to account failed: {}", err))?;
        }
    }

    if let Some(message) = final_message {
        if let Some(final_signer) = final_signer {
            trace!("Deploying program");
            let (blockhash, _, _) = rpc_client
                .get_recent_blockhash_with_commitment(config.commitment)?
                .value;

            let mut final_tx = Transaction::new_unsigned(message.clone());
            final_tx.try_sign(&[payer_signer, final_signer], blockhash)?;
            rpc_client
                .send_and_confirm_transaction_with_spinner_and_config(
                    &final_tx,
                    config.commitment,
                    RpcSendTransactionConfig {
                        skip_preflight: true,
                        preflight_commitment: Some(config.commitment.commitment),
                        ..RpcSendTransactionConfig::default()
                    },
                )
                .map_err(|e| format!("Deploying program failed: {}", e))?;
        }
    }

    Ok(())
}

fn create_ephemeral_keypair(
) -> Result<(usize, bip39::Mnemonic, Keypair), Box<dyn std::error::Error>> {
    const WORDS: usize = 12;
    let mnemonic = Mnemonic::new(MnemonicType::for_word_count(WORDS)?, Language::English);
    let seed = Seed::new(&mnemonic, "");
    let new_keypair = keypair_from_seed(seed.as_bytes())?;

    Ok((WORDS, mnemonic, new_keypair))
}

fn report_ephemeral_mnemonic(words: usize, mnemonic: bip39::Mnemonic) {
    let phrase: &str = mnemonic.phrase();
    let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
    eprintln!(
        "{}\nTo resume a failed deploy, recover the ephemeral keypair file with",
        divider
    );
    eprintln!(
        "`solana-keygen recover` and the following {}-word seed phrase,",
        words
    );
    eprintln!(
        "then pass it as the [BUFFER_SIGNER] argument to `solana upgrade ...`\n{}\n{}\n{}",
        divider, phrase, divider
    );
}

fn option_pubkey_to_string(tag: &str, option: Option<Pubkey>) -> String {
    match option {
        Some(pubkey) => json!({
            tag: format!("{:?}", pubkey),
        })
        .to_string(),
        None => json!({
            tag: "None",
        })
        .to_string(),
    }
}

fn send_and_confirm_transactions_with_spinner<T: Signers>(
    rpc_client: &RpcClient,
    mut transactions: Vec<Transaction>,
    signer_keys: &T,
    commitment: CommitmentConfig,
    mut last_valid_slot: Slot,
) -> Result<(), Box<dyn error::Error>> {
    let progress_bar = new_spinner_progress_bar();
    let mut send_retries = 5;
    let mut leader_schedule: Option<RpcLeaderSchedule> = None;
    let mut leader_schedule_epoch = 0;
    let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let cluster_nodes = rpc_client.get_cluster_nodes().ok();

    loop {
        let mut status_retries = 15;

        progress_bar.set_message("Finding leader node...");
        let epoch_info = rpc_client.get_epoch_info_with_commitment(commitment)?;
        if epoch_info.epoch > leader_schedule_epoch || leader_schedule.is_none() {
            leader_schedule = rpc_client
                .get_leader_schedule_with_commitment(Some(epoch_info.absolute_slot), commitment)?;
            leader_schedule_epoch = epoch_info.epoch;
        }
        let tpu_address = get_leader_tpu(
            min(epoch_info.slot_index + 1, epoch_info.slots_in_epoch),
            leader_schedule.as_ref(),
            cluster_nodes.as_ref(),
        );

        // Send all transactions
        let mut pending_transactions = HashMap::new();
        let num_transactions = transactions.len();
        for transaction in transactions {
            if let Some(tpu_address) = tpu_address {
                let wire_transaction =
                    serialize(&transaction).expect("serialization should succeed");
                send_transaction_tpu(&send_socket, &tpu_address, &wire_transaction);
            } else {
                let _result = rpc_client
                    .send_transaction_with_config(
                        &transaction,
                        RpcSendTransactionConfig {
                            preflight_commitment: Some(commitment.commitment),
                            ..RpcSendTransactionConfig::default()
                        },
                    )
                    .ok();
            }
            pending_transactions.insert(transaction.signatures[0], transaction);

            progress_bar.set_message(&format!(
                "[{}/{}] Total Transactions sent",
                pending_transactions.len(),
                num_transactions
            ));
        }

        // Collect statuses for all the transactions, drop those that are confirmed
        while status_retries > 0 {
            status_retries -= 1;

            progress_bar.set_message(&format!(
                "[{}/{}] Transactions confirmed",
                num_transactions - pending_transactions.len(),
                num_transactions
            ));

            let mut statuses = vec![];
            let pending_signatures = pending_transactions.keys().cloned().collect::<Vec<_>>();
            for pending_signatures_chunk in
                pending_signatures.chunks(MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS - 1)
            {
                statuses.extend(
                    rpc_client
                        .get_signature_statuses_with_history(pending_signatures_chunk)?
                        .value
                        .into_iter(),
                );
            }
            assert_eq!(statuses.len(), pending_signatures.len());

            for (signature, status) in pending_signatures.into_iter().zip(statuses.into_iter()) {
                if let Some(status) = status {
                    if let Some(confirmation_status) = &status.confirmation_status {
                        if *confirmation_status != TransactionConfirmationStatus::Processed {
                            let _ = pending_transactions.remove(&signature);
                        }
                    } else if status.confirmations.is_none() || status.confirmations.unwrap() > 1 {
                        let _ = pending_transactions.remove(&signature);
                    }
                }
                progress_bar.set_message(&format!(
                    "[{}/{}] Transactions confirmed",
                    num_transactions - pending_transactions.len(),
                    num_transactions
                ));
            }

            if pending_transactions.is_empty() {
                return Ok(());
            }

            let slot = rpc_client.get_slot_with_commitment(commitment)?;
            if slot > last_valid_slot {
                break;
            }

            if cfg!(not(test)) {
                // Retry twice a second
                sleep(Duration::from_millis(500));
            }
        }

        if send_retries == 0 {
            return Err("Transactions failed".into());
        }
        send_retries -= 1;

        // Re-sign any failed transactions with a new blockhash and retry
        let (blockhash, _fee_calculator, new_last_valid_slot) = rpc_client
            .get_recent_blockhash_with_commitment(commitment)?
            .value;
        last_valid_slot = new_last_valid_slot;
        transactions = vec![];
        for (_, mut transaction) in pending_transactions.into_iter() {
            transaction.try_sign(signer_keys, blockhash)?;
            transactions.push(transaction);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{app, parse_command, process_command};
    use serde_json::Value;
    use solana_sdk::signature::write_keypair_file;

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
        let test_commands = app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner {
            path: keypair_file.clone(),
            arg_name: "".to_string(),
        };

        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: None,
                    program_pubkey: None,
                    upgrade_authority_signer_index: Some(0),
                    upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                    is_final: false,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
            "--max-len",
            "42",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: None,
                    program_pubkey: None,
                    upgrade_authority_signer_index: Some(0),
                    upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                    is_final: false,
                    max_len: Some(42),
                    allow_excessive_balance: false,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let buffer_keypair = Keypair::new();
        let buffer_keypair_file = make_tmp_path("buffer_keypair_file");
        write_keypair_file(&buffer_keypair, &buffer_keypair_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "--buffer",
            &buffer_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: None,
                    buffer_signer_index: Some(1),
                    buffer_pubkey: Some(buffer_keypair.pubkey()),
                    program_signer_index: None,
                    program_pubkey: None,
                    upgrade_authority_signer_index: Some(0),
                    upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                    is_final: false,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&buffer_keypair_file).unwrap().into(),
                ],
            }
        );

        let program_pubkey = Pubkey::new_unique();
        let test = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
            "--program-id",
            &program_pubkey.to_string(),
        ]);
        assert_eq!(
            parse_command(&test, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: None,
                    program_pubkey: Some(program_pubkey),
                    upgrade_authority_signer_index: Some(0),
                    upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                    is_final: false,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let program_keypair = Keypair::new();
        let program_keypair_file = make_tmp_path("program_keypair_file");
        write_keypair_file(&program_keypair, &program_keypair_file).unwrap();
        let test = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
            "--program-id",
            &program_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: Some(1),
                    program_pubkey: Some(program_keypair.pubkey()),
                    upgrade_authority_signer_index: Some(0),
                    upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                    is_final: false,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&program_keypair_file).unwrap().into(),
                ],
            }
        );

        let authority_pubkey = Pubkey::new_unique();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
            "--upgrade-authority",
            &authority_pubkey.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: None,
                    program_pubkey: None,
                    upgrade_authority_signer_index: None,
                    upgrade_authority_pubkey: Some(authority_pubkey),
                    is_final: false,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
            "--upgrade-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: None,
                    program_pubkey: None,
                    upgrade_authority_signer_index: Some(1),
                    upgrade_authority_pubkey: Some(authority_keypair.pubkey()),
                    is_final: false,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into(),
                ],
            }
        );

        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "deploy",
            "/Users/test/program.so",
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::Deploy {
                    program_location: Some("/Users/test/program.so".to_string()),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    program_signer_index: None,
                    program_pubkey: None,
                    upgrade_authority_signer_index: Some(0),
                    upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                    is_final: true,
                    max_len: None,
                    allow_excessive_balance: false,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_write_buffer() {
        let test_commands = app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner {
            path: keypair_file.clone(),
            arg_name: "".to_string(),
        };

        // defaults
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    buffer_authority_signer_index: Some(0),
                    is_final: false,
                    max_len: None,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        // specify max len
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
            "--max-len",
            "42",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    buffer_authority_signer_index: Some(0),
                    is_final: false,
                    max_len: Some(42),
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        // specify buffer
        let buffer_keypair = Keypair::new();
        let buffer_keypair_file = make_tmp_path("buffer_keypair_file");
        write_keypair_file(&buffer_keypair, &buffer_keypair_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
            "--buffer",
            &buffer_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: Some(1),
                    buffer_pubkey: Some(buffer_keypair.pubkey()),
                    buffer_authority_signer_index: Some(0),
                    is_final: false,
                    max_len: None,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&buffer_keypair_file).unwrap().into(),
                ],
            }
        );

        // specify authority
        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
            "--buffer-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    buffer_authority_signer_index: Some(1),
                    is_final: false,
                    max_len: None,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into(),
                ],
            }
        );

        // specify both buffer and authority
        let buffer_keypair = Keypair::new();
        let buffer_keypair_file = make_tmp_path("buffer_keypair_file");
        write_keypair_file(&buffer_keypair, &buffer_keypair_file).unwrap();
        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
            "--buffer",
            &buffer_keypair_file,
            "--buffer-authority",
            &authority_keypair_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: Some(1),
                    buffer_pubkey: Some(buffer_keypair.pubkey()),
                    buffer_authority_signer_index: Some(2),
                    is_final: false,
                    max_len: None,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&buffer_keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into(),
                ],
            }
        );

        // specify authority
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    buffer_authority_signer_index: Some(0),
                    is_final: true,
                    max_len: None,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        // specify both buffer and authority and final
        let authority_keypair = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority_keypair, &authority_keypair_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "write-buffer",
            "/Users/test/program.so",
            "--buffer-authority",
            &authority_keypair_file,
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::WriteBuffer {
                    program_location: "/Users/test/program.so".to_string(),
                    buffer_signer_index: None,
                    buffer_pubkey: None,
                    buffer_authority_signer_index: Some(1),
                    is_final: true,
                    max_len: None,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into(),
                ],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_set_upgrade_authority() {
        let test_commands = app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner {
            path: keypair_file.clone(),
            arg_name: "".to_string(),
        };

        let program_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Pubkey::new_unique();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-upgrade-authority",
            &program_pubkey.to_string(),
            "--new-upgrade-authority",
            &new_authority_pubkey.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
                    program_pubkey,
                    upgrade_authority_index: Some(0),
                    new_upgrade_authority: Some(new_authority_pubkey),
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let program_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Keypair::new();
        let new_authority_pubkey_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&new_authority_pubkey, &new_authority_pubkey_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-upgrade-authority",
            &program_pubkey.to_string(),
            "--new-upgrade-authority",
            &new_authority_pubkey_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
                    program_pubkey,
                    upgrade_authority_index: Some(0),
                    new_upgrade_authority: Some(new_authority_pubkey.pubkey()),
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let program_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Keypair::new();
        let new_authority_pubkey_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&new_authority_pubkey, &new_authority_pubkey_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-upgrade-authority",
            &program_pubkey.to_string(),
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
                    program_pubkey,
                    upgrade_authority_index: Some(0),
                    new_upgrade_authority: None,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let program_pubkey = Pubkey::new_unique();
        let authority = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority, &authority_keypair_file).unwrap();
        let new_authority_pubkey = Keypair::new();
        let new_authority_pubkey_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&new_authority_pubkey, &new_authority_pubkey_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-upgrade-authority",
            &program_pubkey.to_string(),
            "--upgrade-authority",
            &authority_keypair_file,
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
                    program_pubkey,
                    upgrade_authority_index: Some(1),
                    new_upgrade_authority: None,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into(),
                ],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_set_buffer_authority() {
        let test_commands = app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner {
            path: keypair_file.clone(),
            arg_name: "".to_string(),
        };

        let buffer_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Pubkey::new_unique();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-buffer-authority",
            &buffer_pubkey.to_string(),
            "--new-buffer-authority",
            &new_authority_pubkey.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
                    buffer_pubkey,
                    buffer_authority_index: Some(0),
                    new_buffer_authority: Some(new_authority_pubkey),
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let buffer_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Keypair::new();
        let new_authority_pubkey_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&new_authority_pubkey, &new_authority_pubkey_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-buffer-authority",
            &buffer_pubkey.to_string(),
            "--new-buffer-authority",
            &new_authority_pubkey_file,
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
                    buffer_pubkey,
                    buffer_authority_index: Some(0),
                    new_buffer_authority: Some(new_authority_pubkey.pubkey()),
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let buffer_pubkey = Pubkey::new_unique();
        let new_authority_pubkey = Keypair::new();
        let new_authority_pubkey_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&new_authority_pubkey, &new_authority_pubkey_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-buffer-authority",
            &buffer_pubkey.to_string(),
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
                    buffer_pubkey,
                    buffer_authority_index: Some(0),
                    new_buffer_authority: None,
                }),
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        let buffer_pubkey = Pubkey::new_unique();
        let authority = Keypair::new();
        let authority_keypair_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&authority, &authority_keypair_file).unwrap();
        let new_authority_pubkey = Keypair::new();
        let new_authority_pubkey_file = make_tmp_path("authority_keypair_file");
        write_keypair_file(&new_authority_pubkey, &new_authority_pubkey_file).unwrap();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "set-buffer-authority",
            &buffer_pubkey.to_string(),
            "--buffer-authority",
            &authority_keypair_file,
            "--final",
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
                    buffer_pubkey,
                    buffer_authority_index: Some(1),
                    new_buffer_authority: None,
                }),
                signers: vec![
                    read_keypair_file(&keypair_file).unwrap().into(),
                    read_keypair_file(&authority_keypair_file).unwrap().into(),
                ],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_get_authority() {
        let test_commands = app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let default_signer = DefaultSigner {
            path: keypair_file,
            arg_name: "".to_string(),
        };

        let buffer_pubkey = Pubkey::new_unique();
        let test_deploy = test_commands.clone().get_matches_from(vec![
            "test",
            "program",
            "get-authority",
            &buffer_pubkey.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_deploy, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Program(ProgramCliCommand::GetAuthority {
                    account_pubkey: Some(buffer_pubkey)
                }),
                signers: vec![],
            }
        );
    }

    #[test]
    fn test_cli_keypair_file() {
        solana_logger::setup();

        let default_keypair = Keypair::new();
        let program_pubkey = Keypair::new();
        let deploy_path = make_tmp_path("deploy");
        let mut program_location = PathBuf::from(deploy_path.clone());
        program_location.push("noop");
        program_location.set_extension("so");
        let mut pathbuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        pathbuf.push("tests");
        pathbuf.push("fixtures");
        pathbuf.push("noop");
        pathbuf.set_extension("so");
        let program_keypair_location = program_location.with_file_name("noop-keypair.json");
        std::fs::create_dir_all(deploy_path).unwrap();
        std::fs::copy(pathbuf, program_location.as_os_str()).unwrap();
        write_keypair_file(&program_pubkey, &program_keypair_location).unwrap();

        let config = CliConfig {
            rpc_client: Some(RpcClient::new_mock("".to_string())),
            command: CliCommand::Program(ProgramCliCommand::Deploy {
                program_location: Some(program_location.to_str().unwrap().to_string()),
                buffer_signer_index: None,
                buffer_pubkey: None,
                program_signer_index: None,
                program_pubkey: None,
                upgrade_authority_signer_index: None,
                upgrade_authority_pubkey: Some(default_keypair.pubkey()),
                is_final: false,
                max_len: None,
                allow_excessive_balance: false,
            }),
            signers: vec![&default_keypair],
            ..CliConfig::default()
        };

        let result = process_command(&config);
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        let program_id = json
            .as_object()
            .unwrap()
            .get("ProgramId")
            .unwrap()
            .as_str()
            .unwrap();

        assert_eq!(
            program_id.parse::<Pubkey>().unwrap(),
            program_pubkey.pubkey()
        );
    }
}
