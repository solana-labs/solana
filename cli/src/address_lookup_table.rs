use {
    crate::cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    clap::{App, AppSettings, Arg, ArgMatches, SubCommand},
    solana_address_lookup_table_program::instruction::create_lookup_table,
    solana_clap_utils::{self, input_parsers::*, input_validators::*, keypair::*},
    solana_cli_output::CliAddressLookupTableCreated,
    solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig},
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_sdk::{
        account::from_account, clock::Clock, commitment_config::CommitmentConfig, message::Message,
        sysvar, transaction::Transaction,
    },
    std::sync::Arc,
};

#[derive(Debug, PartialEq, Eq)]
pub enum AddressLookupTableCliCommand {
    CreateLookupTable {
        authority_signer_index: SignerIndex,
        payer_signer_index: SignerIndex,
    },
}

pub trait AddressLookupTableSubCommands {
    fn address_lookup_table_subcommands(self) -> Self;
}

impl AddressLookupTableSubCommands for App<'_, '_> {
    fn address_lookup_table_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("address-lookup-table")
                .about("Address lookup table management")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("create-lookup-table")
                        .about("Create a lookup table")
                        .arg(
                            Arg::with_name("authority")
                                .long("authority")
                                .value_name("AUTHORITY_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help("Lookup table authority [default: the default configured keypair]")
                        )
                        .arg(
                            Arg::with_name("payer")
                                .long("payer")
                                .value_name("PAYER_SIGNER")
                                .takes_value(true)
                                .validator(is_valid_signer)
                                .help("Account that will pay rent fees for the created lookup table [default: the default configured keypair]")
                        )
                )
        )
    }
}

pub fn parse_address_lookup_table_subcommand(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    let (subcommand, sub_matches) = matches.subcommand();

    let response = match (subcommand, sub_matches) {
        ("create-lookup-table", Some(matches)) => {
            let mut bulk_signers = vec![Some(
                default_signer.signer_from_path(matches, wallet_manager)?,
            )];

            let authority_pubkey = if let Ok((authority_signer, Some(authority_pubkey))) =
                signer_of(matches, "authority", wallet_manager)
            {
                bulk_signers.push(authority_signer);
                Some(authority_pubkey)
            } else {
                Some(
                    default_signer
                        .signer_from_path(matches, wallet_manager)?
                        .pubkey(),
                )
            };

            let payer_pubkey = if let Ok((payer_signer, Some(payer_pubkey))) =
                signer_of(matches, "payer", wallet_manager)
            {
                bulk_signers.push(payer_signer);
                Some(payer_pubkey)
            } else {
                Some(
                    default_signer
                        .signer_from_path(matches, wallet_manager)?
                        .pubkey(),
                )
            };

            let signer_info =
                default_signer.generate_unique_signers(bulk_signers, matches, wallet_manager)?;

            CliCommandInfo {
                command: CliCommand::AddressLookupTable(
                    AddressLookupTableCliCommand::CreateLookupTable {
                        authority_signer_index: signer_info.index_of(authority_pubkey).unwrap(),
                        payer_signer_index: signer_info.index_of(payer_pubkey).unwrap(),
                    },
                ),
                signers: signer_info.signers,
            }
        }
        _ => unreachable!(),
    };
    Ok(response)
}

pub fn process_address_lookup_table_subcommand(
    rpc_client: Arc<RpcClient>,
    config: &CliConfig,
    subcommand: &AddressLookupTableCliCommand,
) -> ProcessResult {
    match subcommand {
        AddressLookupTableCliCommand::CreateLookupTable {
            authority_signer_index,
            payer_signer_index,
        } => process_create_lookup_table(
            &rpc_client,
            config,
            *authority_signer_index,
            *payer_signer_index,
        ),
    }
}

fn process_create_lookup_table(
    rpc_client: &RpcClient,
    config: &CliConfig,
    authority_signer_index: usize,
    payer_signer_index: usize,
) -> ProcessResult {
    let authority_signer = config.signers[authority_signer_index];
    let payer_signer = config.signers[payer_signer_index];

    let get_clock_result = rpc_client
        .get_account_with_commitment(&sysvar::clock::id(), CommitmentConfig::finalized())?;
    let clock_account = get_clock_result.value.expect("Clock account doesn't exist");
    let clock: Clock = from_account(&clock_account).ok_or_else(|| {
        CliError::RpcRequestError("Failed to deserialize clock sysvar".to_string())
    })?;

    let authority_address = authority_signer.pubkey();
    let payer_address = payer_signer.pubkey();
    let (create_lookup_table_ix, lookup_table_address) =
        create_lookup_table(authority_address, payer_address, clock.slot);

    let blockhash = rpc_client.get_latest_blockhash()?;
    let mut tx = Transaction::new_unsigned(Message::new(
        &[create_lookup_table_ix],
        Some(&config.signers[0].pubkey()),
    ));

    tx.try_sign(
        &[config.signers[0], authority_signer, payer_signer],
        blockhash,
    )?;
    let result = rpc_client.send_and_confirm_transaction_with_spinner_and_config(
        &tx,
        config.commitment,
        RpcSendTransactionConfig {
            skip_preflight: false,
            preflight_commitment: Some(config.commitment.commitment),
            ..RpcSendTransactionConfig::default()
        },
    );
    match result {
        Err(err) => Err(format!("Create failed: {}", err).into()),
        Ok(signature) => Ok(config
            .output_format
            .formatted_string(&CliAddressLookupTableCreated {
                lookup_table_address: lookup_table_address.to_string(),
                signature: signature.to_string(),
            })),
    }
}
