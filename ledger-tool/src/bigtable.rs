/// The `bigtable` subcommand
use clap::{
    value_t, value_t_or_exit, values_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand,
};
use solana_clap_utils::{
    input_parsers::pubkey_of,
    input_validators::{is_slot, is_valid_pubkey},
};
use solana_cli_output::{
    display::println_transaction, CliBlock, CliTransaction, CliTransactionConfirmation,
    OutputFormat,
};
use solana_ledger::{blockstore::Blockstore, blockstore_db::AccessType};
use solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::{ConfirmedBlock, EncodedTransaction, UiTransactionEncoding};
use std::{
    path::Path,
    process::exit,
    result::Result,
    sync::{atomic::AtomicBool, Arc},
};

async fn upload(
    blockstore: Blockstore,
    starting_slot: Slot,
    ending_slot: Option<Slot>,
    allow_missing_metadata: bool,
    force_reupload: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false, None)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    solana_ledger::bigtable_upload::upload_confirmed_blocks(
        Arc::new(blockstore),
        bigtable,
        starting_slot,
        ending_slot,
        allow_missing_metadata,
        force_reupload,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}

async fn delete_slots(slots: Vec<Slot>, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let read_only = dry_run;
    let bigtable = solana_storage_bigtable::LedgerStorage::new(read_only, None)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    solana_ledger::bigtable_delete::delete_confirmed_blocks(bigtable, slots, dry_run).await
}

async fn first_available_block() -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(true, None).await?;
    match bigtable.get_first_available_block().await? {
        Some(block) => println!("{}", block),
        None => println!("No blocks available"),
    }

    Ok(())
}

async fn block(slot: Slot, output_format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false, None)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    let block = bigtable.get_confirmed_block(slot).await?;

    let cli_block = CliBlock {
        encoded_confirmed_block: block.encode(UiTransactionEncoding::Base64),
        slot,
    };
    println!("{}", output_format.formatted_string(&cli_block));
    Ok(())
}

async fn blocks(starting_slot: Slot, limit: usize) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false, None)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    let slots = bigtable.get_confirmed_blocks(starting_slot, limit).await?;
    println!("{:?}", slots);
    println!("{} blocks found", slots.len());

    Ok(())
}

async fn confirm(
    signature: &Signature,
    verbose: bool,
    output_format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false, None)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    let transaction_status = bigtable.get_signature_status(signature).await?;

    let mut transaction = None;
    let mut get_transaction_error = None;
    if verbose {
        match bigtable.get_confirmed_transaction(signature).await {
            Ok(Some(confirmed_transaction)) => {
                transaction = Some(CliTransaction {
                    transaction: EncodedTransaction::encode(
                        confirmed_transaction.transaction.transaction.clone(),
                        UiTransactionEncoding::Json,
                    ),
                    meta: confirmed_transaction.transaction.meta.map(|m| m.into()),
                    block_time: confirmed_transaction.block_time,
                    slot: Some(confirmed_transaction.slot),
                    decoded_transaction: confirmed_transaction.transaction.transaction,
                    prefix: "  ".to_string(),
                    sigverify_status: vec![],
                });
            }
            Ok(None) => {}
            Err(err) => {
                get_transaction_error = Some(format!("{:?}", err));
            }
        }
    }
    let cli_transaction = CliTransactionConfirmation {
        confirmation_status: Some(transaction_status.confirmation_status()),
        transaction,
        get_transaction_error,
        err: transaction_status.err.clone(),
    };
    println!("{}", output_format.formatted_string(&cli_transaction));
    Ok(())
}

pub async fn transaction_history(
    address: &Pubkey,
    mut limit: usize,
    mut before: Option<Signature>,
    until: Option<Signature>,
    verbose: bool,
    show_transactions: bool,
    query_chunk_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(true, None).await?;

    let mut loaded_block: Option<(Slot, ConfirmedBlock)> = None;
    while limit > 0 {
        let results = bigtable
            .get_confirmed_signatures_for_address(
                address,
                before.as_ref(),
                until.as_ref(),
                limit.min(query_chunk_size),
            )
            .await?;

        if results.is_empty() {
            break;
        }
        before = Some(results.last().unwrap().0.signature);
        assert!(limit >= results.len());
        limit = limit.saturating_sub(results.len());

        for (result, index) in results {
            if verbose {
                println!(
                    "{}, slot={}, memo=\"{}\", status={}",
                    result.signature,
                    result.slot,
                    result.memo.unwrap_or_else(|| "".to_string()),
                    match result.err {
                        None => "Confirmed".to_string(),
                        Some(err) => format!("Failed: {:?}", err),
                    }
                );
            } else {
                println!("{}", result.signature);
            }

            if show_transactions {
                // Instead of using `bigtable.get_confirmed_transaction()`, fetch the entire block
                // and keep it around.  This helps reduce BigTable query traffic and speeds up the
                // results for high-volume addresses
                loop {
                    if let Some((slot, block)) = &loaded_block {
                        if *slot == result.slot {
                            match block.transactions.get(index as usize) {
                                None => {
                                    println!(
                                        "  Transaction info for {} is corrupt",
                                        result.signature
                                    );
                                }
                                Some(transaction_with_meta) => {
                                    println_transaction(
                                        &transaction_with_meta.transaction,
                                        &transaction_with_meta.meta.clone().map(|m| m.into()),
                                        "  ",
                                        None,
                                        None,
                                    );
                                }
                            }
                            break;
                        }
                    }
                    match bigtable.get_confirmed_block(result.slot).await {
                        Err(err) => {
                            println!("  Unable to get confirmed transaction details: {}", err);
                            break;
                        }
                        Ok(block) => {
                            loaded_block = Some((result.slot, block));
                        }
                    }
                }
                println!();
            }
        }
    }
    Ok(())
}

pub trait BigTableSubCommand {
    fn bigtable_subcommand(self) -> Self;
}

impl BigTableSubCommand for App<'_, '_> {
    fn bigtable_subcommand(self) -> Self {
        self.subcommand(
            SubCommand::with_name("bigtable")
                .about("Ledger data on a BigTable instance")
                .setting(AppSettings::InferSubcommands)
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("upload")
                        .about("Upload the ledger to BigTable")
                        .arg(
                            Arg::with_name("starting_slot")
                                .long("starting-slot")
                                .validator(is_slot)
                                .value_name("START_SLOT")
                                .takes_value(true)
                                .index(1)
                                .help(
                                    "Start uploading at this slot [default: first available slot]",
                                ),
                        )
                        .arg(
                            Arg::with_name("ending_slot")
                                .long("ending-slot")
                                .validator(is_slot)
                                .value_name("END_SLOT")
                                .takes_value(true)
                                .index(2)
                                .help("Stop uploading at this slot [default: last available slot]"),
                        )
                        .arg(
                            Arg::with_name("allow_missing_metadata")
                                .long("allow-missing-metadata")
                                .takes_value(false)
                                .help("Don't panic if transaction metadata is missing"),
                        )
                        .arg(
                            Arg::with_name("force_reupload")
                                .long("force")
                                .takes_value(false)
                                .help(
                                    "Force reupload of any blocks already present in BigTable instance\
                                    Note: reupload will *not* delete any data from the tx-by-addr table;\
                                    Use with care.",
                                ),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("delete-slots")
                        .about("Delete ledger information from BigTable")
                        .arg(
                                Arg::with_name("slots")
                                    .index(1)
                                    .value_name("SLOTS")
                                    .takes_value(true)
                                    .multiple(true)
                                    .required(true)
                                    .help("Slots to delete"),
                                )
                            .arg(
                                Arg::with_name("force")
                                    .long("force")
                                    .takes_value(false)
                                    .help(
                                        "Deletions are only performed when the force flag is enabled. \
                                        If force is not enabled, show stats about what ledger data \
                                        will be deleted in a real deletion. "),
                            ),
                        )
                .subcommand(
                    SubCommand::with_name("first-available-block")
                        .about("Get the first available block in the storage"),
                )
                .subcommand(
                    SubCommand::with_name("blocks")
                        .about("Get a list of slots with confirmed blocks for the given range")
                        .arg(
                            Arg::with_name("starting_slot")
                                .long("starting-slot")
                                .validator(is_slot)
                                .value_name("SLOT")
                                .takes_value(true)
                                .index(1)
                                .required(true)
                                .default_value("0")
                                .help("Start listing at this slot"),
                        )
                        .arg(
                            Arg::with_name("limit")
                                .long("limit")
                                .validator(is_slot)
                                .value_name("LIMIT")
                                .takes_value(true)
                                .index(2)
                                .required(true)
                                .default_value("1000")
                                .help("Maximum number of slots to return"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("block")
                        .about("Get a confirmed block")
                        .arg(
                            Arg::with_name("slot")
                                .long("slot")
                                .validator(is_slot)
                                .value_name("SLOT")
                                .takes_value(true)
                                .index(1)
                                .required(true),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("confirm")
                        .about("Confirm transaction by signature")
                        .arg(
                            Arg::with_name("signature")
                                .long("signature")
                                .value_name("TRANSACTION_SIGNATURE")
                                .takes_value(true)
                                .required(true)
                                .index(1)
                                .help("The transaction signature to confirm"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("transaction-history")
                        .about(
                            "Show historical transactions affecting the given address \
                             from newest to oldest",
                        )
                        .arg(
                            Arg::with_name("address")
                                .index(1)
                                .value_name("ADDRESS")
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Account address"),
                        )
                        .arg(
                            Arg::with_name("limit")
                                .long("limit")
                                .takes_value(true)
                                .value_name("LIMIT")
                                .validator(is_slot)
                                .index(2)
                                .default_value("18446744073709551615")
                                .help("Maximum number of transaction signatures to return"),
                        )
                        .arg(
                            Arg::with_name("query_chunk_size")
                                .long("query-chunk-size")
                                .takes_value(true)
                                .value_name("AMOUNT")
                                .validator(is_slot)
                                .default_value("1000")
                                .help(
                                    "Number of transaction signatures to query at once. \
                                       Smaller: more responsive/lower throughput. \
                                       Larger: less responsive/higher throughput",
                                ),
                        )
                        .arg(
                            Arg::with_name("before")
                                .long("before")
                                .value_name("TRANSACTION_SIGNATURE")
                                .takes_value(true)
                                .help("Start with the first signature older than this one"),
                        )
                        .arg(
                            Arg::with_name("until")
                                .long("until")
                                .value_name("TRANSACTION_SIGNATURE")
                                .takes_value(true)
                                .help("End with the last signature newer than this one"),
                        )
                        .arg(
                            Arg::with_name("show_transactions")
                                .long("show-transactions")
                                .takes_value(false)
                                .help("Display the full transactions"),
                        ),
                ),
        )
    }
}

pub fn bigtable_process_command(ledger_path: &Path, matches: &ArgMatches<'_>) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let verbose = matches.is_present("verbose");
    let output_format = OutputFormat::from_matches(matches, "output_format", verbose);

    let future = match matches.subcommand() {
        ("upload", Some(arg_matches)) => {
            let starting_slot = value_t!(arg_matches, "starting_slot", Slot).unwrap_or(0);
            let ending_slot = value_t!(arg_matches, "ending_slot", Slot).ok();
            let allow_missing_metadata = arg_matches.is_present("allow_missing_metadata");
            let force_reupload = arg_matches.is_present("force_reupload");
            let blockstore =
                crate::open_blockstore(ledger_path, AccessType::TryPrimaryThenSecondary, None);

            runtime.block_on(upload(
                blockstore,
                starting_slot,
                ending_slot,
                allow_missing_metadata,
                force_reupload,
            ))
        }
        ("delete-slots", Some(arg_matches)) => {
            let slots = values_t_or_exit!(arg_matches, "slots", Slot);
            let dry_run = !value_t_or_exit!(arg_matches, "force", bool);
            runtime.block_on(delete_slots(slots, dry_run))
        }
        ("first-available-block", Some(_arg_matches)) => runtime.block_on(first_available_block()),
        ("block", Some(arg_matches)) => {
            let slot = value_t_or_exit!(arg_matches, "slot", Slot);
            runtime.block_on(block(slot, output_format))
        }
        ("blocks", Some(arg_matches)) => {
            let starting_slot = value_t_or_exit!(arg_matches, "starting_slot", Slot);
            let limit = value_t_or_exit!(arg_matches, "limit", usize);

            runtime.block_on(blocks(starting_slot, limit))
        }
        ("confirm", Some(arg_matches)) => {
            let signature = arg_matches
                .value_of("signature")
                .unwrap()
                .parse()
                .expect("Invalid signature");

            runtime.block_on(confirm(&signature, verbose, output_format))
        }
        ("transaction-history", Some(arg_matches)) => {
            let address = pubkey_of(arg_matches, "address").unwrap();
            let limit = value_t_or_exit!(arg_matches, "limit", usize);
            let query_chunk_size = value_t_or_exit!(arg_matches, "query_chunk_size", usize);
            let before = arg_matches
                .value_of("before")
                .map(|signature| signature.parse().expect("Invalid signature"));
            let until = arg_matches
                .value_of("until")
                .map(|signature| signature.parse().expect("Invalid signature"));
            let show_transactions = arg_matches.is_present("show_transactions");

            runtime.block_on(transaction_history(
                &address,
                limit,
                before,
                until,
                verbose,
                show_transactions,
                query_chunk_size,
            ))
        }
        _ => unreachable!(),
    };

    future.unwrap_or_else(|err| {
        eprintln!("{:?}", err);
        exit(1);
    });
}
