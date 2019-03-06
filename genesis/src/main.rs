//! A command-line executable for generating the chain's genesis block.

use clap::{crate_version, value_t_or_exit, App, Arg};
use solana::blocktree::create_new_ledger;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use std::error;

/**
 * Bootstrap leader gets two tokens:
 * - one token to create an instance of the vote_program with
 * - one token for the transaction fee
 * - one second token to keep the node identity public key valid
 */
//pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 3;
// TODO: Until https://github.com/solana-labs/solana/issues/2355 is resolved the bootstrap leader
// needs N tokens as its vote account gets re-created on every node restart, costing it tokens
pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 1_000_000;

fn main() -> Result<(), Box<dyn error::Error>> {
    let matches = App::new("solana-genesis")
        .version(crate_version!())
        .arg(
            Arg::with_name("bootstrap_leader_keypair_file")
                .short("b")
                .long("bootstrap-leader-keypair")
                .value_name("BOOTSTRAP LEADER KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's keypair"),
        )
        .arg(
            Arg::with_name("ledger_path")
                .short("l")
                .long("ledger")
                .value_name("DIR")
                .takes_value(true)
                .required(true)
                .help("Use directory as persistent ledger location"),
        )
        .arg(
            Arg::with_name("num_tokens")
                .short("t")
                .long("num_tokens")
                .value_name("TOKENS")
                .takes_value(true)
                .required(true)
                .help("Number of tokens to create in the mint"),
        )
        .arg(
            Arg::with_name("mint_keypair_file")
                .short("m")
                .long("mint")
                .value_name("MINT")
                .takes_value(true)
                .required(true)
                .help("Path to file containing keys of the mint"),
        )
        .get_matches();

    let bootstrap_leader_keypair_file = matches.value_of("bootstrap_leader_keypair_file").unwrap();
    let ledger_path = matches.value_of("ledger_path").unwrap();
    let mint_keypair_file = matches.value_of("mint_keypair_file").unwrap();
    let num_tokens = value_t_or_exit!(matches, "num_tokens", u64);

    let bootstrap_leader_keypair = read_keypair(bootstrap_leader_keypair_file)?;
    let mint_keypair = read_keypair(mint_keypair_file)?;

    let bootstrap_leader_vote_account_keypair = Keypair::new();
    let (mut genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(
        num_tokens,
        bootstrap_leader_keypair.pubkey(),
        BOOTSTRAP_LEADER_LAMPORTS,
    );
    genesis_block.mint_id = mint_keypair.pubkey();
    genesis_block.bootstrap_leader_vote_account_id = bootstrap_leader_vote_account_keypair.pubkey();

    create_new_ledger(ledger_path, &genesis_block)?;
    Ok(())
}
