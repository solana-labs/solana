//! A command-line executable for generating the chain's genesis block.

use clap::{crate_description, crate_name, crate_version, value_t_or_exit, App, Arg};
use solana::blocktree::create_new_ledger;
use solana_sdk::account::Account;
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::signature::{read_keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_stake_api::stake_state;
use solana_vote_api::vote_state;
use std::error;

pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 42;

fn main() -> Result<(), Box<dyn error::Error>> {
    let default_bootstrap_leader_lamports = &BOOTSTRAP_LEADER_LAMPORTS.to_string();
    let default_lamports_per_signature =
        &format!("{}", FeeCalculator::default().lamports_per_signature);

    let matches = App::new(crate_name!())
        .about(crate_description!())
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
            Arg::with_name("lamports")
                .short("t")
                .long("lamports")
                .value_name("LAMPORTS")
                .takes_value(true)
                .required(true)
                .help("Number of lamports to create in the mint"),
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
        .arg(
            Arg::with_name("bootstrap_vote_keypair_file")
                .short("s")
                .long("bootstrap-vote-keypair")
                .value_name("BOOTSTRAP VOTE KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's voting keypair"),
        )
        .arg(
            Arg::with_name("bootstrap_stake_keypair_file")
                .short("k")
                .long("bootstrap-stake-keypair")
                .value_name("BOOTSTRAP STAKE KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's staking keypair"),
        )
        .arg(
            Arg::with_name("bootstrap_leader_lamports")
                .long("bootstrap-leader-lamports")
                .value_name("LAMPORTS")
                .takes_value(true)
                .default_value(default_bootstrap_leader_lamports)
                .required(true)
                .help("Number of lamports to assign to the bootstrap leader"),
        )
        .arg(
            Arg::with_name("lamports_per_signature")
                .long("lamports-per-signature")
                .value_name("LAMPORTS")
                .takes_value(true)
                .default_value(default_lamports_per_signature)
                .help("Number of lamports the cluster will charge for signature verification"),
        )
        .get_matches();

    let bootstrap_leader_keypair_file = matches.value_of("bootstrap_leader_keypair_file").unwrap();
    let bootstrap_vote_keypair_file = matches.value_of("bootstrap_vote_keypair_file").unwrap();
    let bootstrap_stake_keypair_file = matches.value_of("bootstrap_stake_keypair_file").unwrap();
    let mint_keypair_file = matches.value_of("mint_keypair_file").unwrap();
    let ledger_path = matches.value_of("ledger_path").unwrap();
    let lamports = value_t_or_exit!(matches, "lamports", u64);
    let bootstrap_leader_stake_lamports =
        value_t_or_exit!(matches, "bootstrap_leader_lamports", u64);

    let bootstrap_leader_keypair = read_keypair(bootstrap_leader_keypair_file)?;
    let bootstrap_vote_keypair = read_keypair(bootstrap_vote_keypair_file)?;
    let bootstrap_stake_keypair = read_keypair(bootstrap_stake_keypair_file)?;
    let mint_keypair = read_keypair(mint_keypair_file)?;

    // TODO: de-duplicate the stake once passive staking
    //  is fully implemented
    //  https://github.com/solana-labs/solana/issues/4213
    let (vote_account, vote_state) = vote_state::create_bootstrap_leader_account(
        &bootstrap_vote_keypair.pubkey(),
        &bootstrap_leader_keypair.pubkey(),
        0,
        bootstrap_leader_stake_lamports,
    );

    let mut genesis_block = GenesisBlock::new(
        &bootstrap_leader_keypair.pubkey(),
        &[
            // the mint
            (
                mint_keypair.pubkey(),
                Account::new(lamports, 0, &system_program::id()),
            ),
            // node needs an account to issue votes from
            (
                bootstrap_leader_keypair.pubkey(),
                Account::new(1, 0, &system_program::id()),
            ),
            // where votes go to
            (bootstrap_vote_keypair.pubkey(), vote_account),
            // passive bootstrap leader stake, duplicates above temporarily
            (
                bootstrap_stake_keypair.pubkey(),
                stake_state::create_delegate_stake_account(
                    &bootstrap_vote_keypair.pubkey(),
                    &vote_state,
                    bootstrap_leader_stake_lamports,
                ),
            ),
        ],
        &[
            ("solana_vote_program".to_string(), solana_vote_api::id()),
            ("solana_stake_program".to_string(), solana_stake_api::id()),
            ("solana_budget_program".to_string(), solana_budget_api::id()),
            (
                "solana_storage_program".to_string(),
                solana_storage_api::id(),
            ),
            ("solana_token_program".to_string(), solana_token_api::id()),
            ("solana_config_program".to_string(), solana_config_api::id()),
            (
                "solana_exchange_program".to_string(),
                solana_exchange_api::id(),
            ),
        ],
    );
    genesis_block.fee_calculator.lamports_per_signature =
        value_t_or_exit!(matches, "lamports_per_signature", u64);

    create_new_ledger(ledger_path, &genesis_block)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use hashbrown::HashSet;

    #[test]
    fn test_program_ids() {
        let ids = [
            (
                "11111111111111111111111111111111",
                solana_sdk::system_program::id(),
            ),
            (
                "NativeLoader1111111111111111111111111111111",
                solana_sdk::native_loader::id(),
            ),
            (
                "BPFLoader1111111111111111111111111111111111",
                solana_sdk::bpf_loader::id(),
            ),
            (
                "Budget1111111111111111111111111111111111111",
                solana_budget_api::id(),
            ),
            (
                "Stake11111111111111111111111111111111111111",
                solana_stake_api::id(),
            ),
            (
                "Storage111111111111111111111111111111111111",
                solana_storage_api::id(),
            ),
            (
                "Token11111111111111111111111111111111111111",
                solana_token_api::id(),
            ),
            (
                "Vote111111111111111111111111111111111111111",
                solana_vote_api::id(),
            ),
            (
                "Stake11111111111111111111111111111111111111",
                solana_stake_api::id(),
            ),
            (
                "Config1111111111111111111111111111111111111",
                solana_config_api::id(),
            ),
            (
                "Exchange11111111111111111111111111111111111",
                solana_exchange_api::id(),
            ),
        ];
        assert!(ids.iter().all(|(name, id)| *name == id.to_string()));
    }

    #[test]
    fn test_program_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = vec![
            solana_sdk::system_program::id(),
            solana_sdk::native_loader::id(),
            solana_sdk::bpf_loader::id(),
            solana_budget_api::id(),
            solana_storage_api::id(),
            solana_token_api::id(),
            solana_vote_api::id(),
            solana_stake_api::id(),
            solana_config_api::id(),
            solana_exchange_api::id(),
        ];
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
    }
}
