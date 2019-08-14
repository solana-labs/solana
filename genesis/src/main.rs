//! A command-line executable for generating the chain's genesis block.

use clap::{crate_description, crate_name, crate_version, value_t_or_exit, App, Arg};
use solana::blocktree::create_new_ledger;
use solana_sdk::account::Account;
use solana_sdk::fee_calculator::FeeCalculator;
use solana_sdk::genesis_block::Builder;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::poh_config::PohConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_sdk::timing;
use solana_stake_api::stake_state;
use solana_storage_api::storage_contract;
use solana_vote_api::vote_state;
use std::collections::HashMap;
use std::error;
use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};

pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 42;

pub enum AccountFileFormat {
    Pubkey,
    Keypair,
}

pub fn append_primordial_accounts(
    file: &str,
    file_format: AccountFileFormat,
    mut builder: Builder,
) -> io::Result<(Builder)> {
    let accounts_file = File::open(file.to_string())?;

    let primordial_accounts: HashMap<String, u64> = serde_yaml::from_reader(accounts_file)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    for (account, balance) in primordial_accounts {
        let pubkey = match file_format {
            AccountFileFormat::Pubkey => Pubkey::from_str(account.as_str()).unwrap(),
            AccountFileFormat::Keypair => {
                let bytes: Vec<u8> = serde_json::from_str(account.as_str()).unwrap();
                Keypair::from_bytes(&bytes).unwrap().pubkey()
            }
        };

        builder = builder.account(pubkey, Account::new(balance, 0, &system_program::id()));
    }

    Ok(builder)
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let default_bootstrap_leader_lamports = &BOOTSTRAP_LEADER_LAMPORTS.to_string();
    let default_target_lamports_per_signature = &FeeCalculator::default()
        .target_lamports_per_signature
        .to_string();
    let default_target_signatures_per_slot = &FeeCalculator::default()
        .target_signatures_per_slot
        .to_string();
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
            Arg::with_name("target_lamports_per_signature")
                .long("target-lamports-per-signature")
                .value_name("LAMPORTS")
                .takes_value(true)
                .default_value(default_target_lamports_per_signature)
                .help(
                    "The cost in lamports that the cluster will charge for signature \
                     verification when the cluster is operating at target-signatures-per-slot",
                ),
        )
        .arg(
            Arg::with_name("target_signatures_per_slot")
                .long("target-signatures-per-slot")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_target_signatures_per_slot)
                .help(
                    "Used to estimate the desired processing capacity of the cluster.
                    When the latest slot processes fewer/greater signatures than this \
                    value, the lamports-per-signature fee will decrease/increase for \
                    the next slot. A value of 0 disables signature-based fee adjustments",
                ),
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
        .arg(
            Arg::with_name("primordial_keypairs_file")
                .long("primordial-keypairs-file")
                .value_name("FILENAME")
                .takes_value(true)
                .help("The location of keypairs for primordial accounts and balance"),
        )
        .get_matches();

    let bootstrap_leader_keypair_file = matches.value_of("bootstrap_leader_keypair_file").unwrap();
    let bootstrap_vote_keypair_file = matches.value_of("bootstrap_vote_keypair_file").unwrap();
    let bootstrap_stake_keypair_file = matches.value_of("bootstrap_stake_keypair_file").unwrap();
    let bootstrap_storage_keypair_file =
        matches.value_of("bootstrap_storage_keypair_file").unwrap();
    let mint_keypair_file = matches.value_of("mint_keypair_file").unwrap();
    let ledger_path = PathBuf::from(matches.value_of("ledger_path").unwrap());
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

    let mut builder = Builder::new()
        .accounts(&[
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
                stake_state::create_stake_account(
                    &bootstrap_vote_keypair.pubkey(),
                    &vote_state,
                    bootstrap_leader_stake_lamports,
                ),
            ),
            (
                bootstrap_storage_keypair.pubkey(),
                storage_contract::create_validator_storage_account(
                    bootstrap_leader_keypair.pubkey(),
                    1,
                ),
            ),
        ])
        .native_instruction_processors(&solana_genesis_programs::get())
        .ticks_per_slot(value_t_or_exit!(matches, "ticks_per_slot", u64))
        .slots_per_epoch(value_t_or_exit!(matches, "slots_per_epoch", u64));

    let mut fee_calculator = FeeCalculator::default();
    fee_calculator.target_lamports_per_signature =
        value_t_or_exit!(matches, "target_lamports_per_signature", u64);
    fee_calculator.target_signatures_per_slot =
        value_t_or_exit!(matches, "target_signatures_per_slot", usize);
    builder = builder.fee_calculator(FeeCalculator::new_derived(&fee_calculator, 0));

    let mut poh_config = PohConfig::default();
    poh_config.target_tick_duration =
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

            let hashes_per_tick =
                (poh_config.target_tick_duration.as_millis() * 1_000_000 / elapsed) as u64;
            println!("Hashes per tick: {}", hashes_per_tick);
            poh_config.hashes_per_tick = Some(hashes_per_tick);
        }
        "sleep" => {
            poh_config.hashes_per_tick = None;
        }
        _ => {
            poh_config.hashes_per_tick = Some(value_t_or_exit!(matches, "hashes_per_tick", u64));
        }
    }
    builder = builder.poh_config(poh_config);

    if let Some(file) = matches.value_of("primordial_accounts_file") {
        builder = append_primordial_accounts(file, AccountFileFormat::Pubkey, builder)?;
    }

    if let Some(file) = matches.value_of("primordial_keypairs_file") {
        builder = append_primordial_accounts(file, AccountFileFormat::Keypair, builder)?;
    }

    // add genesis stuff from storage and stake
    builder = solana_storage_api::rewards_pools::genesis(builder);
    builder = solana_stake_api::genesis(builder);

    create_new_ledger(&ledger_path, &builder.build())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_block::Builder;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::fs::remove_file;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn test_append_primordial_accounts_to_genesis() {
        // Test invalid file returns error
        assert!(append_primordial_accounts(
            "unknownfile",
            AccountFileFormat::Pubkey,
            Builder::new()
        )
        .is_err());

        let mut builder = Builder::new();

        let mut primordial_accounts = HashMap::new();
        primordial_accounts.insert(Pubkey::new_rand().to_string(), 2 as u64);
        primordial_accounts.insert(Pubkey::new_rand().to_string(), 1 as u64);
        primordial_accounts.insert(Pubkey::new_rand().to_string(), 3 as u64);

        let serialized = serde_yaml::to_string(&primordial_accounts).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        builder = append_primordial_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            AccountFileFormat::Pubkey,
            builder,
        )
        .expect("test_append_primordial_accounts_to_genesis.yml");
        // Test valid file returns ok

        remove_file(path).unwrap();

        {
            let genesis_block = builder.clone().build();
            // Test all accounts were added
            assert_eq!(genesis_block.accounts.len(), primordial_accounts.len());

            // Test account data matches
            (0..primordial_accounts.len()).for_each(|i| {
                assert_eq!(
                    primordial_accounts[&genesis_block.accounts[i].0.to_string()],
                    genesis_block.accounts[i].1.lamports,
                );
            });
        }

        // Test more accounts can be appended
        let mut primordial_accounts1 = HashMap::new();
        primordial_accounts1.insert(Pubkey::new_rand().to_string(), 6 as u64);
        primordial_accounts1.insert(Pubkey::new_rand().to_string(), 5 as u64);
        primordial_accounts1.insert(Pubkey::new_rand().to_string(), 10 as u64);

        let serialized = serde_yaml::to_string(&primordial_accounts1).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        builder = append_primordial_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            AccountFileFormat::Pubkey,
            builder,
        )
        .expect("test_append_primordial_accounts_to_genesis.yml");

        remove_file(path).unwrap();

        let genesis_block = builder.clone().build();
        // Test total number of accounts is correct
        assert_eq!(
            genesis_block.accounts.len(),
            primordial_accounts.len() + primordial_accounts1.len()
        );

        // Test old accounts are still there
        (0..primordial_accounts.len()).for_each(|i| {
            assert_eq!(
                primordial_accounts[&genesis_block.accounts[i].0.to_string()],
                genesis_block.accounts[i].1.lamports,
            );
        });

        // Test new account data matches
        (0..primordial_accounts1.len()).for_each(|i| {
            assert_eq!(
                primordial_accounts1[&genesis_block.accounts[primordial_accounts.len() + i]
                    .0
                    .to_string()],
                genesis_block.accounts[primordial_accounts.len() + i]
                    .1
                    .lamports,
            );
        });

        // Test accounts from keypairs can be appended
        let account_keypairs: Vec<_> = (0..3).map(|_| Keypair::new()).collect();
        let mut primordial_accounts2 = HashMap::new();
        primordial_accounts2.insert(
            serde_json::to_string(&account_keypairs[0].to_bytes().to_vec()).unwrap(),
            20 as u64,
        );
        primordial_accounts2.insert(
            serde_json::to_string(&account_keypairs[1].to_bytes().to_vec()).unwrap(),
            15 as u64,
        );
        primordial_accounts2.insert(
            serde_json::to_string(&account_keypairs[2].to_bytes().to_vec()).unwrap(),
            30 as u64,
        );

        let serialized = serde_yaml::to_string(&primordial_accounts2).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        builder = append_primordial_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            AccountFileFormat::Keypair,
            builder,
        )
        .expect("builder");

        builder = solana_storage_api::rewards_pools::genesis(builder);

        remove_file(path).unwrap();

        let genesis_block = builder.clone().build();
        // Test total number of accounts is correct
        assert_eq!(
            genesis_block.accounts.len(),
            primordial_accounts.len() + primordial_accounts1.len() + primordial_accounts2.len()
        );

        // Test old accounts are still there
        (0..primordial_accounts.len()).for_each(|i| {
            assert_eq!(
                primordial_accounts[&genesis_block.accounts[i].0.to_string()],
                genesis_block.accounts[i].1.lamports,
            );
        });

        // Test new account data matches
        (0..primordial_accounts1.len()).for_each(|i| {
            assert_eq!(
                primordial_accounts1[&genesis_block.accounts[primordial_accounts.len() + i]
                    .0
                    .to_string()],
                genesis_block.accounts[primordial_accounts.len() + i]
                    .1
                    .lamports,
            );
        });

        let offset = primordial_accounts.len() + primordial_accounts1.len();
        // Test account data for keypairs matches
        account_keypairs.iter().for_each(|keypair| {
            let mut i = 0;
            (offset..(offset + account_keypairs.len())).for_each(|n| {
                if keypair.pubkey() == genesis_block.accounts[n].0 {
                    i = n;
                }
            });

            assert_ne!(i, 0);
            assert_eq!(
                primordial_accounts2[&serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap()],
                genesis_block.accounts[i].1.lamports,
            );
        });
    }
}
