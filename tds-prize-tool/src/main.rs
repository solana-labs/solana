mod prize;
mod rewards_earned;

use clap::{crate_description, crate_name, crate_version, value_t_or_exit, App, Arg, SubCommand};
use solana_core::blocktree::Blocktree;
use solana_core::blocktree_processor::process_blocktree;
use solana_sdk::genesis_block::GenesisBlock;
use std::path::PathBuf;
use std::process::exit;

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("ledger")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .global(true)
                .help("Use directory for ledger location"),
        )
        .subcommand(SubCommand::with_name("results").about("Print the results"))
        .get_matches();

    let ledger_path = PathBuf::from(value_t_or_exit!(matches, "ledger", String));

    let genesis_block = GenesisBlock::load(&ledger_path).unwrap_or_else(|err| {
        eprintln!(
            "Failed to open ledger genesis_block at {:?}: {}",
            ledger_path, err
        );
        exit(1);
    });

    let blocktree = match Blocktree::open(&ledger_path) {
        Ok(blocktree) => blocktree,
        Err(err) => {
            eprintln!("Failed to open ledger at {:?}: {}", ledger_path, err);
            exit(1);
        }
    };

    println!("Verifying ledger...");
    match process_blocktree(&genesis_block, &blocktree, None, true, None) {
        Ok((bank_forks, _bank_forks_info, _leader_schedule_cache)) => {
            let bank = bank_forks.working_bank();
            let rewards_earned_winners = rewards_earned::compute_winners(&bank);
            println!("{:#?}", rewards_earned_winners);
        }
        Err(err) => {
            eprintln!("Ledger verification failed: {:?}", err);
            exit(1);
        }
    }
}
