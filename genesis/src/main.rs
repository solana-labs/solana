//! A command-line executable for generating the chain's genesis config.

mod genesis_accounts;

use crate::genesis_accounts::create_genesis_accounts;
use clap::{crate_description, crate_name, value_t_or_exit, App, Arg};
use solana_genesis::Base64Account;
use solana_ledger::blocktree::create_new_ledger;
use solana_ledger::poh::compute_hashes_per_tick;
use solana_sdk::{
    account::Account,
    clock,
    epoch_schedule::EpochSchedule,
    fee_calculator::FeeCalculator,
    genesis_config::{GenesisConfig, OperatingMode},
    native_token::sol_to_lamports,
    poh_config::PohConfig,
    pubkey::{read_pubkey_file, Pubkey},
    rent::Rent,
    signature::{read_keypair_file, Keypair, KeypairUtil},
    system_program, timing,
};
use solana_stake_api::stake_state;
use solana_storage_api::storage_contract;
use solana_vote_api::vote_state;
use std::{collections::HashMap, error, fs::File, io, path::PathBuf, str::FromStr, time::Duration};

pub enum AccountFileFormat {
    Pubkey,
    Keypair,
}

fn pubkey_from_file(key_file: &str) -> Result<Pubkey, Box<dyn error::Error>> {
    read_pubkey_file(key_file)
        .or_else(|_| read_keypair_file(key_file).map(|keypair| keypair.pubkey()))
}

fn pubkey_from_str(key_str: &str) -> Result<Pubkey, Box<dyn error::Error>> {
    Pubkey::from_str(key_str).or_else(|_| {
        let bytes: Vec<u8> = serde_json::from_str(key_str)?;
        let keypair = Keypair::from_bytes(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        Ok(keypair.pubkey())
    })
}

pub fn add_genesis_accounts(file: &str, genesis_config: &mut GenesisConfig) -> io::Result<()> {
    let accounts_file = File::open(file.to_string())?;

    let genesis_accounts: HashMap<String, Base64Account> =
        serde_yaml::from_reader(accounts_file)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    for (key, account_details) in genesis_accounts {
        let pubkey = pubkey_from_str(key.as_str()).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Invalid pubkey/keypair {}: {:?}", key, err),
            )
        })?;

        let owner_program_id = Pubkey::from_str(account_details.owner.as_str()).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Invalid owner: {}: {:?}", account_details.owner, err),
            )
        })?;

        let mut account = Account::new(account_details.balance, 0, &owner_program_id);
        if account_details.data != "~" {
            account.data = base64::decode(account_details.data.as_str()).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Invalid account data: {}: {:?}", account_details.data, err),
                )
            })?;
        }
        account.executable = account_details.executable;

        genesis_config.add_account(pubkey, account);
    }

    Ok(())
}

#[allow(clippy::cognitive_complexity)]
fn main() -> Result<(), Box<dyn error::Error>> {
    let default_bootstrap_leader_lamports = &sol_to_lamports(500.0).to_string();
    let default_bootstrap_leader_stake_lamports = &sol_to_lamports(0.5).to_string();
    let default_lamports = &sol_to_lamports(500_000_000.0).to_string();
    let default_target_lamports_per_signature = &FeeCalculator::default()
        .target_lamports_per_signature
        .to_string();
    let default_target_signatures_per_slot = &FeeCalculator::default()
        .target_signatures_per_slot
        .to_string();
    let (
        default_lamports_per_byte_year,
        default_rent_exemption_threshold,
        default_rent_burn_percentage,
    ) = {
        let rent = Rent::default();
        (
            &rent.lamports_per_byte_year.to_string(),
            &rent.exemption_threshold.to_string(),
            &rent.burn_percent.to_string(),
        )
    };
    let default_target_tick_duration =
        &timing::duration_as_ms(&PohConfig::default().target_tick_duration).to_string();
    let default_ticks_per_slot = &clock::DEFAULT_TICKS_PER_SLOT.to_string();
    let default_slots_per_epoch = &EpochSchedule::default().slots_per_epoch.to_string();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("bootstrap_leader_pubkey_file")
                .short("b")
                .long("bootstrap-leader-pubkey")
                .value_name("BOOTSTRAP LEADER PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's pubkey"),
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
                .default_value(default_lamports)
                .required(true)
                .help("Number of lamports to create in the mint"),
        )
        .arg(
            Arg::with_name("mint_pubkey_file")
                .short("m")
                .long("mint")
                .value_name("MINT")
                .takes_value(true)
                .required(true)
                .help("Path to file containing keys of the mint"),
        )
        .arg(
            Arg::with_name("bootstrap_vote_pubkey_file")
                .short("s")
                .long("bootstrap-vote-pubkey")
                .value_name("BOOTSTRAP VOTE PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's voting pubkey"),
        )
        .arg(
            Arg::with_name("bootstrap_stake_pubkey_file")
                .short("k")
                .long("bootstrap-stake-pubkey")
                .value_name("BOOTSTRAP STAKE PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's staking pubkey"),
        )
        .arg(
            Arg::with_name("bootstrap_storage_pubkey_file")
                .long("bootstrap-storage-pubkey")
                .value_name("BOOTSTRAP STORAGE PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Path to file containing the bootstrap leader's storage pubkey"),
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
                .default_value(default_bootstrap_leader_stake_lamports)
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
            Arg::with_name("lamports_per_byte_year")
                .long("lamports-per-byte-year")
                .value_name("LAMPORTS")
                .takes_value(true)
                .default_value(default_lamports_per_byte_year)
                .help(
                    "The cost in lamports that the cluster will charge per byte per year \
                     for accounts with data.",
                ),
        )
        .arg(
            Arg::with_name("rent_exemption_threshold")
                .long("rent-exemption-threshold")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_rent_exemption_threshold)
                .help(
                    "amount of time (in years) the balance has to include rent for \
                     to qualify as rent exempted account.",
                ),
        )
        .arg(
            Arg::with_name("rent_burn_percentage")
                .long("rent-burn-percentage")
                .value_name("NUMBER")
                .takes_value(true)
                .default_value(default_rent_burn_percentage)
                .help("amount of rent to burn, as a fraction of std::u8::MAX."),
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
                .multiple(true)
                .help("The location of pubkey for primordial accounts and balance"),
        )
        .arg(
            Arg::with_name("development")
                .long("dev")
                .help("Configure the cluster for development mode where all features are available at epoch 0"),
        )
        .get_matches();

    let bootstrap_leader_pubkey_file = matches.value_of("bootstrap_leader_pubkey_file").unwrap();
    let bootstrap_vote_pubkey_file = matches.value_of("bootstrap_vote_pubkey_file").unwrap();
    let bootstrap_stake_pubkey_file = matches.value_of("bootstrap_stake_pubkey_file").unwrap();
    let bootstrap_storage_pubkey_file = matches.value_of("bootstrap_storage_pubkey_file").unwrap();
    let mint_pubkey_file = matches.value_of("mint_pubkey_file").unwrap();
    let ledger_path = PathBuf::from(matches.value_of("ledger_path").unwrap());
    let lamports = value_t_or_exit!(matches, "lamports", u64);
    let bootstrap_leader_lamports = value_t_or_exit!(matches, "bootstrap_leader_lamports", u64);
    let bootstrap_leader_stake_lamports =
        value_t_or_exit!(matches, "bootstrap_leader_stake_lamports", u64);

    let bootstrap_leader_pubkey = pubkey_from_file(bootstrap_leader_pubkey_file)?;
    let bootstrap_vote_pubkey = pubkey_from_file(bootstrap_vote_pubkey_file)?;
    let bootstrap_stake_pubkey = pubkey_from_file(bootstrap_stake_pubkey_file)?;
    let bootstrap_storage_pubkey = pubkey_from_file(bootstrap_storage_pubkey_file)?;
    let mint_pubkey = pubkey_from_file(mint_pubkey_file)?;

    let bootstrap_leader_vote_account =
        vote_state::create_account(&bootstrap_vote_pubkey, &bootstrap_leader_pubkey, 0, 1);

    let rent = Rent {
        lamports_per_byte_year: value_t_or_exit!(matches, "lamports_per_byte_year", u64),
        exemption_threshold: value_t_or_exit!(matches, "rent_exemption_threshold", f64),
        burn_percent: value_t_or_exit!(matches, "rent_burn_percentage", u8),
    };

    let bootstrap_leader_stake_account = stake_state::create_account(
        &bootstrap_leader_pubkey,
        &bootstrap_vote_pubkey,
        &bootstrap_leader_vote_account,
        &rent,
        bootstrap_leader_stake_lamports,
    );

    let mut accounts = vec![
        // node needs an account to issue votes from
        (
            bootstrap_leader_pubkey,
            Account::new(bootstrap_leader_lamports, 0, &system_program::id()),
        ),
        // where votes go to
        (bootstrap_vote_pubkey, bootstrap_leader_vote_account),
        // bootstrap leader stake
        (bootstrap_stake_pubkey, bootstrap_leader_stake_account),
        (
            bootstrap_storage_pubkey,
            storage_contract::create_validator_storage_account(bootstrap_leader_pubkey, 1),
        ),
    ];
    accounts.append(&mut create_genesis_accounts(&mint_pubkey, lamports));

    let ticks_per_slot = value_t_or_exit!(matches, "ticks_per_slot", u64);
    let slots_per_epoch = value_t_or_exit!(matches, "slots_per_epoch", u64);
    let epoch_schedule = EpochSchedule::new(slots_per_epoch);

    let fee_calculator = FeeCalculator::new(
        value_t_or_exit!(matches, "target_lamports_per_signature", u64),
        value_t_or_exit!(matches, "target_signatures_per_slot", usize),
    );

    let mut poh_config = PohConfig::default();
    poh_config.target_tick_duration =
        Duration::from_millis(value_t_or_exit!(matches, "target_tick_duration", u64));

    match matches.value_of("hashes_per_tick").unwrap() {
        "auto" => {
            let hashes_per_tick =
                compute_hashes_per_tick(poh_config.target_tick_duration, 1_000_000);
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

    let operating_mode = if matches.is_present("development") {
        OperatingMode::Development
    } else {
        OperatingMode::SoftLaunch
    };
    let native_instruction_processors =
        solana_genesis_programs::get_programs(operating_mode, 0).unwrap();
    let inflation = solana_genesis_programs::get_inflation(operating_mode, 0).unwrap();
    let mut genesis_config = GenesisConfig {
        accounts,
        native_instruction_processors,
        ticks_per_slot,
        epoch_schedule,
        inflation,
        fee_calculator,
        rent,
        poh_config,
        operating_mode,
        ..GenesisConfig::default()
    };

    if let Some(files) = matches.values_of("primordial_accounts_file") {
        for file in files {
            add_genesis_accounts(file, &mut genesis_config)?;
        }
    }

    // add genesis stuff from storage and stake
    solana_storage_api::rewards_pools::add_genesis_accounts(&mut genesis_config);
    solana_stake_api::add_genesis_accounts(&mut genesis_config);

    create_new_ledger(&ledger_path, &genesis_config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_config::GenesisConfig;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::fs::remove_file;
    use std::io::Write;
    use std::path::Path;
    use tempfile;

    #[test]
    fn test_append_primordial_accounts_to_genesis() {
        // Test invalid file returns error
        assert!(add_genesis_accounts("unknownfile", &mut GenesisConfig::default()).is_err());

        let mut genesis_config = GenesisConfig::default();

        let mut genesis_accounts = HashMap::new();
        genesis_accounts.insert(
            Pubkey::new_rand().to_string(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 2 as u64,
                executable: false,
                data: String::from("aGVsbG8="),
            },
        );
        genesis_accounts.insert(
            Pubkey::new_rand().to_string(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 1 as u64,
                executable: true,
                data: String::from("aGVsbG8gd29ybGQ="),
            },
        );
        genesis_accounts.insert(
            Pubkey::new_rand().to_string(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 3 as u64,
                executable: true,
                data: String::from("bWUgaGVsbG8gdG8gd29ybGQ="),
            },
        );

        let serialized = serde_yaml::to_string(&genesis_accounts).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        add_genesis_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            &mut genesis_config,
        )
        .expect("test_append_primordial_accounts_to_genesis.yml");
        // Test valid file returns ok

        remove_file(path).unwrap();

        {
            // Test all accounts were added
            assert_eq!(genesis_config.accounts.len(), genesis_accounts.len());

            // Test account data matches
            (0..genesis_accounts.len()).for_each(|i| {
                assert_eq!(
                    genesis_accounts[&genesis_config.accounts[i].0.to_string()].owner,
                    genesis_config.accounts[i].1.owner.to_string()
                );

                assert_eq!(
                    genesis_accounts[&genesis_config.accounts[i].0.to_string()].balance,
                    genesis_config.accounts[i].1.lamports
                );

                assert_eq!(
                    genesis_accounts[&genesis_config.accounts[i].0.to_string()].executable,
                    genesis_config.accounts[i].1.executable
                );

                assert_eq!(
                    genesis_accounts[&genesis_config.accounts[i].0.to_string()].data,
                    base64::encode(&genesis_config.accounts[i].1.data)
                );
            });
        }

        // Test more accounts can be appended
        let mut genesis_accounts1 = HashMap::new();
        genesis_accounts1.insert(
            Pubkey::new_rand().to_string(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 6 as u64,
                executable: true,
                data: String::from("eW91IGFyZQ=="),
            },
        );
        genesis_accounts1.insert(
            Pubkey::new_rand().to_string(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 5 as u64,
                executable: false,
                data: String::from("bWV0YSBzdHJpbmc="),
            },
        );
        genesis_accounts1.insert(
            Pubkey::new_rand().to_string(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 10 as u64,
                executable: false,
                data: String::from("YmFzZTY0IHN0cmluZw=="),
            },
        );

        let serialized = serde_yaml::to_string(&genesis_accounts1).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        add_genesis_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            &mut genesis_config,
        )
        .expect("test_append_primordial_accounts_to_genesis.yml");

        remove_file(path).unwrap();

        // Test total number of accounts is correct
        assert_eq!(
            genesis_config.accounts.len(),
            genesis_accounts.len() + genesis_accounts1.len()
        );

        // Test old accounts are still there
        (0..genesis_accounts.len()).for_each(|i| {
            assert_eq!(
                genesis_accounts[&genesis_config.accounts[i].0.to_string()].balance,
                genesis_config.accounts[i].1.lamports,
            );
        });

        // Test new account data matches
        (0..genesis_accounts1.len()).for_each(|i| {
            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .owner,
                genesis_config.accounts[genesis_accounts.len() + i]
                    .1
                    .owner
                    .to_string(),
            );

            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .balance,
                genesis_config.accounts[genesis_accounts.len() + i]
                    .1
                    .lamports,
            );

            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .executable,
                genesis_config.accounts[genesis_accounts.len() + i]
                    .1
                    .executable,
            );

            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .data,
                base64::encode(&genesis_config.accounts[genesis_accounts.len() + i].1.data),
            );
        });

        // Test accounts from keypairs can be appended
        let account_keypairs: Vec<_> = (0..3).map(|_| Keypair::new()).collect();
        let mut genesis_accounts2 = HashMap::new();
        genesis_accounts2.insert(
            serde_json::to_string(&account_keypairs[0].to_bytes().to_vec()).unwrap(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 20 as u64,
                executable: true,
                data: String::from("Y2F0IGRvZw=="),
            },
        );
        genesis_accounts2.insert(
            serde_json::to_string(&account_keypairs[1].to_bytes().to_vec()).unwrap(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 15 as u64,
                executable: false,
                data: String::from("bW9ua2V5IGVsZXBoYW50"),
            },
        );
        genesis_accounts2.insert(
            serde_json::to_string(&account_keypairs[2].to_bytes().to_vec()).unwrap(),
            Base64Account {
                owner: Pubkey::new_rand().to_string(),
                balance: 30 as u64,
                executable: true,
                data: String::from("Y29tYSBtb2Nh"),
            },
        );

        let serialized = serde_yaml::to_string(&genesis_accounts2).unwrap();
        let path = Path::new("test_append_primordial_accounts_to_genesis.yml");
        let mut file = File::create(path).unwrap();
        file.write_all(&serialized.into_bytes()).unwrap();

        add_genesis_accounts(
            "test_append_primordial_accounts_to_genesis.yml",
            &mut genesis_config,
        )
        .expect("genesis");

        solana_storage_api::rewards_pools::add_genesis_accounts(&mut genesis_config);

        remove_file(path).unwrap();

        // Test total number of accounts is correct
        assert_eq!(
            genesis_config.accounts.len(),
            genesis_accounts.len() + genesis_accounts1.len() + genesis_accounts2.len()
        );

        // Test old accounts are still there
        (0..genesis_accounts.len()).for_each(|i| {
            assert_eq!(
                genesis_accounts[&genesis_config.accounts[i].0.to_string()].balance,
                genesis_config.accounts[i].1.lamports,
            );
        });

        // Test new account data matches
        (0..genesis_accounts1.len()).for_each(|i| {
            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .owner,
                genesis_config.accounts[genesis_accounts.len() + i]
                    .1
                    .owner
                    .to_string(),
            );

            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .balance,
                genesis_config.accounts[genesis_accounts.len() + i]
                    .1
                    .lamports,
            );

            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .executable,
                genesis_config.accounts[genesis_accounts.len() + i]
                    .1
                    .executable,
            );

            assert_eq!(
                genesis_accounts1[&genesis_config.accounts[genesis_accounts.len() + i]
                    .0
                    .to_string()]
                    .data,
                base64::encode(&genesis_config.accounts[genesis_accounts.len() + i].1.data),
            );
        });

        let offset = genesis_accounts.len() + genesis_accounts1.len();
        // Test account data for keypairs matches
        account_keypairs.iter().for_each(|keypair| {
            let mut i = 0;
            (offset..(offset + account_keypairs.len())).for_each(|n| {
                if keypair.pubkey() == genesis_config.accounts[n].0 {
                    i = n;
                }
            });

            assert_ne!(i, 0);

            assert_eq!(
                genesis_accounts2[&serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap()]
                    .owner,
                genesis_config.accounts[i].1.owner.to_string(),
            );

            assert_eq!(
                genesis_accounts2[&serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap()]
                    .balance,
                genesis_config.accounts[i].1.lamports,
            );

            assert_eq!(
                genesis_accounts2[&serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap()]
                    .executable,
                genesis_config.accounts[i].1.executable,
            );

            assert_eq!(
                genesis_accounts2[&serde_json::to_string(&keypair.to_bytes().to_vec()).unwrap()]
                    .data,
                base64::encode(&genesis_config.accounts[i].1.data),
            );
        });
    }

    #[test]
    fn test_genesis_account_struct_compatibility() {
        let yaml_string_pubkey = "---
98frSc8R8toHoS3tQ1xWSvHCvGEADRM9hAm5qmUKjSDX:
  balance: 4
  owner: Gw6S9CPzR8jHku1QQMdiqcmUKjC2dhJ3gzagWduA6PGw
  data:
  executable: true
88frSc8R8toHoS3tQ1xWSvHCvGEADRM9hAm5qmUKjSDX:
  balance: 3
  owner: Gw7S9CPzR8jHku1QQMdiqcmUKjC2dhJ3gzagWduA6PGw
  data: ~
  executable: true
6s36rsNPDfRSvzwek7Ly3mQu9jUMwgqBhjePZMV6Acp4:
  balance: 2
  owner: DBC5d45LUHTCrq42ZmCdzc8A8ufwTaiYsL9pZY7KU6TR
  data: aGVsbG8=
  executable: false
8Y98svZv5sPHhQiPqZvqA5Z5djQ8hieodscvb61RskMJ:
  balance: 1
  owner: DSknYr8cPucRbx2VyssZ7Yx3iiRqNGD38VqVahkUvgV1
  data: aGVsbG8gd29ybGQ=
  executable: true";

        let tmpfile = tempfile::NamedTempFile::new().unwrap();
        let path = tmpfile.path();
        let mut file = File::create(path).unwrap();
        file.write_all(yaml_string_pubkey.as_bytes()).unwrap();

        let mut genesis_config = GenesisConfig::default();
        add_genesis_accounts(path.to_str().unwrap(), &mut genesis_config).expect("genesis");
        remove_file(path).unwrap();

        assert_eq!(genesis_config.accounts.len(), 4);

        let yaml_string_keypair = "---
\"[17,12,234,59,35,246,168,6,64,36,169,164,219,96,253,79,238,202,164,160,195,89,9,96,179,117,255,239,32,64,124,66,233,130,19,107,172,54,86,32,119,148,4,39,199,40,122,230,249,47,150,168,163,159,83,233,97,18,25,238,103,25,253,108]\":
  balance: 20
  owner: 9ZfsP6Um1KU8d5gNzTsEbSJxanKYp5EPF36qUu4FJqgp
  data: Y2F0IGRvZw==
  executable: true
\"[36,246,244,43,37,214,110,50,134,148,148,8,205,82,233,67,223,245,122,5,149,232,213,125,244,182,26,29,56,224,70,45,42,163,71,62,222,33,229,54,73,136,53,174,128,103,247,235,222,27,219,129,180,77,225,174,220,74,201,123,97,155,159,234]\":
  balance: 15
  owner: F9dmtjJPi8vfLu1EJN4KkyoGdXGmVfSAhxz35Qo9RDCJ
  data: bW9ua2V5IGVsZXBoYW50
  executable: false
\"[103,27,132,107,42,149,72,113,24,138,225,109,209,31,158,6,26,11,8,76,24,128,131,215,156,80,251,114,103,220,111,235,56,22,87,5,209,56,53,12,224,170,10,66,82,42,11,138,51,76,120,27,166,200,237,16,200,31,23,5,57,22,131,221]\":
  balance: 30
  owner: AwAR5mAbNPbvQ4CvMeBxwWE8caigQoMC2chkWAbh2b9V
  data: Y29tYSBtb2Nh
  executable: true";

        let tmpfile = tempfile::NamedTempFile::new().unwrap();
        let path = tmpfile.path();
        let mut file = File::create(path).unwrap();
        file.write_all(yaml_string_keypair.as_bytes()).unwrap();

        let mut genesis_config = GenesisConfig::default();
        add_genesis_accounts(path.to_str().unwrap(), &mut genesis_config).expect("genesis");
        remove_file(path).unwrap();

        assert_eq!(genesis_config.accounts.len(), 3);
    }
}
