/// The `bigtable` subcommand
use clap::{value_t, value_t_or_exit, App, AppSettings, Arg, ArgMatches, SubCommand};
use solana_clap_utils::{
    input_parsers::pubkey_of,
    input_validators::{is_slot, is_valid_pubkey},
};
use solana_cli::display::println_transaction;
use solana_ledger::{blockstore::Blockstore, blockstore_db::AccessType};
use solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::UiTransactionEncoding;
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
) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    solana_ledger::bigtable_upload::upload_confirmed_blocks(
        Arc::new(blockstore),
        bigtable,
        starting_slot,
        ending_slot,
        allow_missing_metadata,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}

async fn first_available_block() -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(true).await?;
    match bigtable.get_first_available_block().await? {
        Some(block) => println!("{}", block),
        None => println!("No blocks available"),
    }

    Ok(())
}

async fn block(slot: Slot) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    let block = bigtable
        .get_confirmed_block(slot, UiTransactionEncoding::Base64)
        .await?;

    println!("Slot: {}", slot);
    println!("Parent Slot: {}", block.parent_slot);
    println!("Blockhash: {}", block.blockhash);
    println!("Previous Blockhash: {}", block.previous_blockhash);
    if block.block_time.is_some() {
        println!("Block Time: {:?}", block.block_time);
    }
    if !block.rewards.is_empty() {
        println!("Rewards: {:?}", block.rewards);
    }
    for (index, transaction_with_meta) in block.transactions.iter().enumerate() {
        println!("Transaction {}:", index);
        println_transaction(
            &transaction_with_meta.transaction.decode().unwrap(),
            &transaction_with_meta.meta,
            "  ",
        );
    }
    Ok(())
}

async fn blocks(starting_slot: Slot, limit: usize) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    let slots = bigtable.get_confirmed_blocks(starting_slot, limit).await?;
    println!("{:?}", slots);
    println!("{} blocks found", slots.len());

    Ok(())
}

async fn confirm(signature: &Signature, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(false)
        .await
        .map_err(|err| format!("Failed to connect to storage: {:?}", err))?;

    let transaction_status = bigtable.get_signature_status(signature).await?;

    if verbose {
        match bigtable
            .get_confirmed_transaction(signature, UiTransactionEncoding::Base64)
            .await
        {
            Ok(Some(confirmed_transaction)) => {
                println!(
                    "\nTransaction executed in slot {}:",
                    confirmed_transaction.slot
                );
                println_transaction(
                    &confirmed_transaction
                        .transaction
                        .transaction
                        .decode()
                        .expect("Successful decode"),
                    &confirmed_transaction.transaction.meta,
                    "  ",
                );
            }
            Ok(None) => println!("Confirmed transaction details not available"),
            Err(err) => println!("Unable to get confirmed transaction details: {}", err),
        }
        println!();
    }
    match transaction_status.status {
        Ok(_) => println!("Confirmed"),
        Err(err) => println!("Transaction failed: {}", err),
    }
    Ok(())
}

pub async fn transaction_history(
    address: &Pubkey,
    mut limit: usize,
    mut before: Option<Signature>,
    until: Option<Signature>,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let bigtable = solana_storage_bigtable::LedgerStorage::new(true).await?;

    while limit > 0 {
        let results = bigtable
            .get_confirmed_signatures_for_address(
                address,
                before.as_ref(),
                until.as_ref(),
                limit.min(1000),
            )
            .await?;

        if results.is_empty() {
            break;
        }
        before = Some(results.last().unwrap().signature);
        assert!(limit >= results.len());
        limit = limit.saturating_sub(results.len());

        for result in results {
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
                .setting(AppSettings::ArgRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("upload")
                        .about("Upload the ledger to BigTable")
                        .arg(
                            Arg::with_name("starting_slot")
                                .long("starting-slot")
                                .validator(is_slot)
                                .value_name("SLOT")
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
                                .value_name("SLOT")
                                .takes_value(true)
                                .index(2)
                                .help("Stop uploading at this slot [default: last available slot]"),
                        )
                        .arg(
                            Arg::with_name("allow_missing_metadata")
                                .long("allow-missing-metadata")
                                .takes_value(false)
                                .help("Don't panic if transaction metadata is missing"),
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
                        )
                        .arg(
                            Arg::with_name("verbose")
                                .short("v")
                                .long("verbose")
                                .takes_value(false)
                                .help("Show additional information"),
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
                            Arg::with_name("verbose")
                                .short("v")
                                .long("verbose")
                                .takes_value(false)
                                .help("Show additional information"),
                        ),
                ),
        )
    }
}

pub fn bigtable_process_command(ledger_path: &Path, matches: &ArgMatches<'_>) {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    let future = match matches.subcommand() {
        ("upload", Some(arg_matches)) => {
            let starting_slot = value_t!(arg_matches, "starting_slot", Slot).unwrap_or(0);
            let ending_slot = value_t!(arg_matches, "ending_slot", Slot).ok();
            let allow_missing_metadata = arg_matches.is_present("allow_missing_metadata");
            let blockstore =
                crate::open_blockstore(&ledger_path, AccessType::TryPrimaryThenSecondary, None);

            runtime.block_on(upload(
                blockstore,
                starting_slot,
                ending_slot,
                allow_missing_metadata,
            ))
        }
        ("first-available-block", Some(_arg_matches)) => runtime.block_on(first_available_block()),
        ("block", Some(arg_matches)) => {
            let slot = value_t_or_exit!(arg_matches, "slot", Slot);
            runtime.block_on(block(slot))
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
            let verbose = arg_matches.is_present("verbose");

            runtime.block_on(confirm(&signature, verbose))
        }
        ("transaction-history", Some(arg_matches)) => {
            let address = pubkey_of(arg_matches, "address").unwrap();
            let limit = value_t_or_exit!(arg_matches, "limit", usize);
            let before = arg_matches
                .value_of("before")
                .map(|signature| signature.parse().expect("Invalid signature"));
            let until = arg_matches
                .value_of("until")
                .map(|signature| signature.parse().expect("Invalid signature"));
            let verbose = arg_matches.is_present("verbose");

            runtime.block_on(transaction_history(&address, limit, before, until, verbose))
        }
        _ => unreachable!(),
    };

    future.unwrap_or_else(|err| {
        eprintln!("{:?}", err);
        exit(1);
    });
}
