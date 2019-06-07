//! A command-line executable for generating the chain's genesis block.
#[macro_use]
extern crate solana_vote_program;
#[macro_use]
extern crate solana_stake_program;
#[macro_use]
extern crate solana_budget_program;
#[macro_use]
extern crate solana_token_program;
#[macro_use]
extern crate solana_config_program;
#[macro_use]
extern crate solana_exchange_program;

use clap::{crate_description, crate_name, crate_version, value_t_or_exit, App, Arg};
use serde_derive::{Deserialize, Serialize};
use solana::blocktree::create_new_ledger;
use solana_sdk::account::Account;
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::poh_config::PohConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_sdk::timing;
use solana_stake_api::stake_state;
use solana_storage_program::genesis_block_util::GenesisBlockUtil;
use solana_vote_api::vote_state;
use std::error;
use std::fs::File;
use std::io;
use std::time::{Duration, Instant};

pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 42;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct PrimordialAccount {
    pub pubkey: Pubkey,
    pub lamports: u64,
}

pub fn append_primordial_accounts(file: &str, genesis_block: &mut GenesisBlock) -> io::Result<()> {
    let accounts_file = File::open(file.to_string())?;

    let primordial_accounts: Vec<PrimordialAccount> = serde_yaml::from_reader(accounts_file)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    primordial_accounts.iter().for_each(|primordial| {
        genesis_block.accounts.push((
            primordial.pubkey,
            Account::new(primordial.lamports, 0, &system_program::id()),
        ))
    });

    Ok(())
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let default_bootstrap_leader_lamports = &BOOTSTRAP_LEADER_LAMPORTS.to_string();
    let default_lamports_per_signature =
        &FeeCalculator::default().lamports_per_signature.to_string();
    let default_target_tick_duration =
        &timing::duration_as_ms(&PohConfig::default().target_tick_duration).to_string();
    let default_ticks_per_slot = &timing::DEFAULT_TICKS_PER_SLOT.to_string();
    let default_slots_per_epoch = &timing::DEFAULT_SLOTS_PER_EPOCH.to_string();

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
            Arg::with_name("bootstrap_storage_keypair_file")
                .long("bootstrap-storage-keypair")
                .value_name("BOOTSTRAP STORAGE KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's storage keypair"),
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
            Arg::with_name("bootstrap_leader_stake_lamports")
                .long("bootstrap-leader-stake-lamports")
                .value_name("LAMPORTS")
                .takes_value(true)
                .default_value(default_bootstrap_leader_lamports)
                .required(true)
                .help("Number of lamports to assign to the bootstrap leader's stake account"),
        )
        .arg(
            Arg::with_name("lamports_per_signature")
                .long("lamports-per-signature")
                .value_name("LAMPORTS")
                .takes_value(true)
                .default_value(default_lamports_per_signature)
                .help("Number of lamports the cluster will charge for signature verification"),
        )
        .arg(
            Arg::with_name("target_tick_duration")
                .long("target-tick-duration")
                .value_name("MILLIS")
                .takes_value(true)
                .default_value(default_target_tick_duration)
                .help("The target tick rate of the cluster in milliseconds"),
        )
        .arg(
            Arg::with_name("hashes_per_tick")
                .long("hashes-per-tick")
                .value_name("NUM_HASHES|\"auto\"|\"sleep\"")
                .takes_value(true)
                .default_value("auto")
                .help(
                    "How many PoH hashes to roll before emitting the next tick. \
                     If \"auto\", determine based on --target-tick-duration \
                     and the hash rate of this computer. If \"sleep\", for development \
                     sleep for --target-tick-duration instead of hashing",
                ),
        )
        .arg(
            Arg::with_name("ticks_per_slot")
                .long("ticks-per-slot")
                .value_name("TICKS")
                .takes_value(true)
                .default_value(default_ticks_per_slot)
                .help("The number of ticks in a slot"),
        )
        .arg(
            Arg::with_name("slots_per_epoch")
                .long("slots-per-epoch")
                .value_name("SLOTS")
                .takes_value(true)
                .default_value(default_slots_per_epoch)
                .help("The number of slots in an epoch"),
        )
        .arg(
            Arg::with_name("primordial_accounts_file")
                .long("primordial-accounts-file")
                .value_name("FILENAME")
                .takes_value(true)
                .help("The location of pubkey for primordial accounts and balance"),
        )
        .get_matches();

    let bootstrap_leader_keypair_file = matches.value_of("bootstrap_leader_keypair_file").unwrap();
    let bootstrap_vote_keypair_file = matches.value_of("bootstrap_vote_keypair_file").unwrap();
    let bootstrap_stake_keypair_file = matches.value_of("bootstrap_stake_keypair_file").unwrap();
    let bootstrap_storage_keypair_file =
        matches.value_of("bootstrap_storage_keypair_file").unwrap();
    let mint_keypair_file = matches.value_of("mint_keypair_file").unwrap();
    let ledger_path = matches.value_of("ledger_path").unwrap();
    let lamports = value_t_or_exit!(matches, "lamports", u64);
    let bootstrap_leader_lamports = value_t_or_exit!(matches, "bootstrap_leader_lamports", u64);
    let bootstrap_leader_stake_lamports =
        value_t_or_exit!(matches, "bootstrap_leader_stake_lamports", u64);

    let bootstrap_leader_keypair = read_keypair(bootstrap_leader_keypair_file)?;
    let bootstrap_vote_keypair = read_keypair(bootstrap_vote_keypair_file)?;
    let bootstrap_stake_keypair = read_keypair(bootstrap_stake_keypair_file)?;
    let bootstrap_storage_keypair = read_keypair(bootstrap_storage_keypair_file)?;
    let mint_keypair = read_keypair(mint_keypair_file)?;

    let (vote_account, vote_state) = vote_state::create_bootstrap_leader_account(
        &bootstrap_vote_keypair.pubkey(),
        &bootstrap_leader_keypair.pubkey(),
        0,
        1,
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
                Account::new(bootstrap_leader_lamports, 0, &system_program::id()),
            ),
            // where votes go to
            (bootstrap_vote_keypair.pubkey(), vote_account),
            // passive bootstrap leader stake
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
            solana_vote_program!(),
            solana_stake_program!(),
            solana_budget_program!(),
            solana_token_program!(),
            solana_config_program!(),
            solana_exchange_program!(),
        ],
    );

    if let Some(file) = matches.value_of("primordial_accounts_file") {
        append_primordial_accounts(file, &mut genesis_block)?;
    }

    genesis_block.add_storage_program(
        &bootstrap_leader_keypair.pubkey(),
        &bootstrap_storage_keypair.pubkey(),
    );

    genesis_block.fee_calculator.lamports_per_signature =
        value_t_or_exit!(matches, "lamports_per_signature", u64);
    genesis_block.ticks_per_slot = value_t_or_exit!(matches, "ticks_per_slot", u64);
    genesis_block.slots_per_epoch = value_t_or_exit!(matches, "slots_per_epoch", u64);
    genesis_block.poh_config.target_tick_duration =
        Duration::from_millis(value_t_or_exit!(matches, "target_tick_duration", u64));

    match matches.value_of("hashes_per_tick").unwrap() {
        "auto" => {
            let mut v = Hash::default();
            println!("Running 1 million hashes...");
            let start = Instant::now();
            for _ in 0..1_000_000 {
                v = hash(&v.as_ref());
            }
            let end = Instant::now();
            let elapsed = end.duration_since(start).as_millis();

            let hashes_per_tick = (genesis_block.poh_config.target_tick_duration.as_millis()
                * 1_000_000
                / elapsed) as u64;
            println!("Hashes per tick: {}", hashes_per_tick);
            genesis_block.poh_config.hashes_per_tick = Some(hashes_per_tick);
        }
        "sleep" => {
            genesis_block.poh_config.hashes_per_tick = None;
        }
        _ => {
            genesis_block.poh_config.hashes_per_tick =
                Some(value_t_or_exit!(matches, "hashes_per_tick", u64));
        }
    }

    create_new_ledger(ledger_path, &genesis_block)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashbrown::HashSet;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::pubkey::Pubkey;
    use std::fs::remove_file;
    use std::io::Write;
    use std::path::Path;

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

    #[test]
    fn test_append_primordial_accounts_to_genesis() {
        let mut genesis_block = GenesisBlock::new(&Pubkey::new_rand(), &[], &[]);

        // Test invalid file returns error
        assert!(append_primordial_accounts("unknownfile", &mut genesis_block).is_err());

        let primordial_accounts = [
            PrimordialAccount {
                pubkey: Pubkey::new_rand(),
                lamports: 2,
            },
            PrimordialAccount {
                pubkey: Pubkey::new_rand(),
                lamports: 1,
            },
            PrimordialAccount {
                pubkey: Pubkey::new_rand(),
                lamports: 3,
            },
        ];

        let serialized = serde_yaml::to_string(&primordial_accounts).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        // Test valid file returns ok
        assert!(append_primordial_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            &mut genesis_block
        )
        .is_ok());

        remove_file(path).unwrap();

        // Test all accounts were added
        assert_eq!(genesis_block.accounts.len(), primordial_accounts.len());

        // Test account data matches
        (0..primordial_accounts.len()).for_each(|i| {
            assert_eq!(genesis_block.accounts[i].0, primordial_accounts[i].pubkey);
            assert_eq!(
                genesis_block.accounts[i].1.lamports,
                primordial_accounts[i].lamports
            );
        });

        // Test more accounts can be appended
        let primordial_accounts1 = [
            PrimordialAccount {
                pubkey: Pubkey::new_rand(),
                lamports: 6,
            },
            PrimordialAccount {
                pubkey: Pubkey::new_rand(),
                lamports: 5,
            },
            PrimordialAccount {
                pubkey: Pubkey::new_rand(),
                lamports: 10,
            },
        ];

        let serialized = serde_yaml::to_string(&primordial_accounts1).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        assert!(append_primordial_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            &mut genesis_block
        )
        .is_ok());

        remove_file(path).unwrap();

        // Test total number of accounts is correct
        assert_eq!(
            genesis_block.accounts.len(),
            primordial_accounts.len() + primordial_accounts1.len()
        );

        // Test old accounts are still there
        (0..primordial_accounts.len()).for_each(|i| {
            assert_eq!(genesis_block.accounts[i].0, primordial_accounts[i].pubkey);
            assert_eq!(
                genesis_block.accounts[i].1.lamports,
                primordial_accounts[i].lamports
            );
        });

        // Test new account data matches
        (0..primordial_accounts1.len()).for_each(|i| {
            assert_eq!(
                genesis_block.accounts[primordial_accounts.len() + i].0,
                primordial_accounts1[i].pubkey
            );
            assert_eq!(
                genesis_block.accounts[primordial_accounts.len() + i]
                    .1
                    .lamports,
                primordial_accounts1[i].lamports
            );
        });
    }
}
