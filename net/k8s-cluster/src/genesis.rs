use {
    crate::{boxed_error, initialize_globals, SOLANA_ROOT},
    clap::{App, Arg, ArgMatches},
    base64::{
        engine::general_purpose,
        Engine as _,
    },
    bip39::{Language, Mnemonic, MnemonicType, Seed},
    itertools::Itertools,
    log::*,
    solana_clap_v3_utils::{input_parsers::STDOUT_OUTFILE_TOKEN, keygen},
    solana_entry::poh::compute_hashes_per_tick,
    solana_genesis::genesis_accounts::add_genesis_accounts,
    solana_ledger::{blockstore::create_new_ledger, blockstore_options::LedgerColumnOptions},
    solana_sdk::{
        account::{Account, AccountSharedData},
        bpf_loader_upgradeable::UpgradeableLoaderState,
        clock,
        epoch_schedule::EpochSchedule,
        genesis_config::{ClusterType, GenesisConfig, DEFAULT_GENESIS_FILE},
        native_token::sol_to_lamports,
        poh_config::PohConfig,
        pubkey::Pubkey,
        rent::Rent,
        signature::{
            keypair_from_seed, write_keypair, write_keypair_file, Keypair,
            Signer,
        },
        signer::keypair::read_keypair_file,
        stake::state::StakeStateV2,
        system_program, timing,
    },
    solana_stake_program::stake_state,
    solana_vote_program::vote_state::{self, VoteState},
    std::{
        error::Error,
        fs::File,
        io::{
            Read,
            BufRead,
            BufReader,
        },
        path::PathBuf,
        process,
        time::Duration,
        collections::HashMap,
    },
};

pub const DEFAULT_WORD_COUNT: usize = 12;

enum SPLGenesisArgType {
    BpfProgram(String, String, String),
    UpgradeableProgram(String, String, String, String),
}

#[derive(Clone, Debug)]
pub struct SetupConfig<'a> {
    pub namespace: &'a str,
    pub num_validators: i32,
    pub prebuild_genesis: bool,
}

fn output_keypair(keypair: &Keypair, outfile: &str, source: &str) -> Result<(), Box<dyn Error>> {
    if outfile == STDOUT_OUTFILE_TOKEN {
        let mut stdout = std::io::stdout();
        write_keypair(keypair, &mut stdout)?;
    } else {
        write_keypair_file(keypair, outfile)?;
        println!("Wrote {source} keypair to {outfile}");
    }
    Ok(())
}

fn generate_keypair() -> Result<Keypair, Box<dyn Error>> {
    let (passphrase, _) = keygen::mnemonic::no_passphrase_and_message();
    let mnemonic_type = MnemonicType::for_word_count(DEFAULT_WORD_COUNT).unwrap();
    let mnemonic = Mnemonic::new(mnemonic_type, Language::English);
    let seed = Seed::new(&mnemonic, &passphrase);
    keypair_from_seed(seed.as_bytes())
}

fn parse_spl_genesis_file(spl_file: &PathBuf) -> Result<HashMap<String, Vec<SPLGenesisArgType>>, Box<dyn Error>> {
    if let Ok(file) = File::open(spl_file) {
        let reader = BufReader::new(file);

        // Read the first line of the file
        if let Some(line) = reader.lines().next() {
            let line_contents = line?;

            // Split the line into individual parts using "--" as the delimiter
            let parts: Vec<&str> = line_contents.split("--").collect();

            // Initialize a HashMap to store parsed arguments
            let mut parsed_args = HashMap::new();

            // The first part is usually empty, so start from index 1
            for part in &parts[1..] {
                // Trim leading and trailing whitespaces
                let trimmed_part = part.trim();

                // Split each part into flag and value using the first space
                let split_parts: Vec<&str> = trimmed_part.splitn(2, ' ').collect();

                if split_parts.len() == 2 {
                    let flag = format!("--{}", split_parts[0].trim());
                    // Split the "value" into three parts using spaces
                    let value_parts: Vec<&str> = split_parts[1].split_whitespace().collect();
                    if flag == "--bpf-program" && value_parts.len() == 3 {
                        // Handle "--bpf-program" with 3 arguments
                        let value_tuple = SPLGenesisArgType::BpfProgram(
                            value_parts[0].to_string(),
                            value_parts[1].to_string(),
                            value_parts[2].to_string(),
                        );

                        // Add the flag and value tuple to the parsed_args HashMap
                        parsed_args
                            .entry(flag)
                            .or_insert(Vec::new())
                            .push(value_tuple);
                    } else if flag == "--upgradeable-program" && value_parts.len() == 4 {
                        // Handle "--upgradable-program" with 4 arguments
                        let value_tuple = SPLGenesisArgType::UpgradeableProgram(
                            value_parts[0].to_string(),
                            value_parts[1].to_string(),
                            value_parts[2].to_string(),
                            value_parts[3].to_string(),
                        );

                        // Add the flag and value tuple to the parsed_args HashMap
                        parsed_args
                            .entry(flag)
                            .or_insert(Vec::new())
                            .push(value_tuple);
                    } else {
                        panic!("Invalid argument passed in! flag: {}, value_parts: {}", flag, value_parts.len());
                    }
                }
            }
            return Ok(parsed_args);
        }
    }
    Err(boxed_error!("Can't open spl file even though it exists"))
}

pub struct ValidatorAccountKeypairs {
    pub vote_account: Keypair,
    pub identity: Keypair,
    pub stake_account: Keypair,
}

pub struct Genesis<'a> {
    pub config: SetupConfig<'a>,
    pub config_dir: PathBuf,
    pub validator_keypairs: Vec<ValidatorAccountKeypairs>,
    pub faucet_keypair: Option<Keypair>,
    pub genesis_config: Option<GenesisConfig>,
    pub all_pubkeys: Vec<Pubkey>
}

impl<'a> Genesis<'a> {
    pub fn new(setup_config: SetupConfig<'a>) -> Self {
        initialize_globals();
        let config_dir = SOLANA_ROOT.join("config-k8s");
        if config_dir.exists() {
            std::fs::remove_dir_all(&config_dir).unwrap();
        }
        std::fs::create_dir_all(&config_dir).unwrap();
        Genesis {
            config: setup_config,
            config_dir,
            validator_keypairs: Vec::default(),
            faucet_keypair: None,
            genesis_config: None,
            all_pubkeys: Vec::default()
        }
    }

    pub fn generate_faucet(&mut self) -> Result<(), Box<dyn Error>> {
        let outfile = self.config_dir.join("faucet.json");
        let keypair = match generate_keypair() {
            Ok(keypair) => keypair,
            Err(err) => return Err(err),
        };

        if let Some(outfile) = outfile.to_str() {
            output_keypair(&keypair, outfile, "new")
                .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
        }
        self.faucet_keypair = Some(keypair);
        Ok(())
    }

    pub fn generate_accounts(
        &mut self, 
        validator_type: &str, 
        number_of_accounts: i32
    ) -> Result<(), Box<dyn Error>> {
        let mut filename_prefix = "validator".to_string();
        if validator_type == "bootstrap" {
            filename_prefix = format!("{}-{}", validator_type, filename_prefix);
        } else if validator_type == "validator" {
            filename_prefix = "validator".to_string();
        } else {
            return Err(boxed_error!(format!("Invalid validator type: {}", validator_type)));
        }

        for i in 0..number_of_accounts {
            self.generate_account(validator_type, filename_prefix.as_str(), i)?;
        }

        Ok(())
    }

    fn generate_account(&mut self, validator_type: &str, filename_prefix: &str, i: i32) -> Result<(), Box<dyn Error>> {
        let account_types = vec!["identity", "vote-account", "stake-account"];

        let mut identity: Option<Keypair> = None;
        let mut vote: Option<Keypair> = None;
        let mut stake: Option<Keypair> = None;
        for account in account_types {
            let filename: String;
            if validator_type == "bootstrap" {
                filename = format!("{}/{}.json", filename_prefix, account);
            } else if validator_type == "validator" {
                filename = format!("{}-{}-{}.json", filename_prefix, account, i);
            } else {
                return Err(boxed_error!(format!("Invalid validator type: {}", validator_type)));
            }

            let outfile = self.config_dir.join(filename);
            info!("outfile: {:?}", outfile);

            let keypair = match generate_keypair() {
                Ok(keypair) => keypair,
                Err(err) => return Err(err),
            };
            self.all_pubkeys.push(keypair.pubkey());

            if let Some(outfile) = outfile.to_str() {
                output_keypair(&keypair, outfile, "new")
                    .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
            }

            if account == "identity" {
                identity = Some(keypair);
            } else if account == "vote-account" {
                vote = Some(keypair);
            } else if account == "stake-account" {
                stake = Some(keypair);
            } else {
                return Err(boxed_error!(format!("invalid account type! {}", account)));
            }
        }

        if validator_type == "bootstrap" {
            self.validator_keypairs.push(ValidatorAccountKeypairs {
                vote_account: vote
                    .ok_or_else(|| boxed_error!("vote-account keypair not initialized"))?,
                identity: identity.ok_or_else(|| boxed_error!("identity keypair not initialized"))?,
                stake_account: stake
                    .ok_or_else(|| boxed_error!("stake-account keypair not initialized"))?,
            });
        }

        Ok(())
    }

    pub fn generate(&mut self) -> Result<(), Box<dyn Error>> {
        let ledger_path = self.config_dir.join("bootstrap-validator");
        let rent = Rent::default();

        // vote account
        // lamports set a default 500
        let default_bootstrap_validator_lamports = &sol_to_lamports(500.0)
            .max(VoteState::get_rent_exempt_reserve(&rent))
            .to_string();
        let bootstrap_validator_lamports: u64 =
            default_bootstrap_validator_lamports.parse().unwrap(); //TODO enable command line arg

        // stake account
        let default_bootstrap_validator_stake_lamports = &sol_to_lamports(100.0)
            .max(rent.minimum_balance(StakeStateV2::size_of()))
            .to_string();
        let bootstrap_validator_stake_lamports: u64 =
            default_bootstrap_validator_stake_lamports.parse().unwrap(); // TODO enable command line arg

        let commission: u8 = 100; // Default

        let faucet_lamports: u64 = 500000000000000000;

        let mut poh_config = PohConfig {
            target_tick_duration: Duration::from_micros(timing::duration_as_us(
                &PohConfig::default().target_tick_duration,
            )),
            ..PohConfig::default()
        };

        let cluster_type = ClusterType::Development; //Default

        match cluster_type {
            ClusterType::Development => {
                let hashes_per_tick =
                    compute_hashes_per_tick(poh_config.target_tick_duration, 1_000_000);
                poh_config.hashes_per_tick = Some(hashes_per_tick / 2); // use 50% of peak ability
            }
            ClusterType::Devnet | ClusterType::Testnet | ClusterType::MainnetBeta => {
                poh_config.hashes_per_tick = Some(clock::DEFAULT_HASHES_PER_TICK);
            }
        }

        let max_genesis_archive_unpacked_size = 1073741824; // set in setup.sh

        let slots_per_epoch = match cluster_type {
            ClusterType::Development => clock::DEFAULT_DEV_SLOTS_PER_EPOCH,
            ClusterType::Devnet | ClusterType::Testnet | ClusterType::MainnetBeta => {
                clock::DEFAULT_SLOTS_PER_EPOCH
            }
        };

        let enable_warmup_epochs = true; //  TODO: fix for flag
        let epoch_schedule = EpochSchedule::custom(
            slots_per_epoch,
            slots_per_epoch,
            enable_warmup_epochs, 
        );

        let mut genesis_config = GenesisConfig {
            epoch_schedule,
            ..GenesisConfig::default()
        };

        // Based off of genesis/src/main.rs
        // loop through this. we only excpect to loop through once for bootstrap.
        // but when we add in other validators, we should add these other validator accounts to genesis here
        for account in self.validator_keypairs.iter() {
            genesis_config.add_account(
                account.identity.pubkey(),
                AccountSharedData::new(bootstrap_validator_lamports, 0, &system_program::id()),
            );

            let vote_account = vote_state::create_account_with_authorized(
                &account.identity.pubkey(),
                &account.identity.pubkey(),
                &account.identity.pubkey(),
                commission,
                VoteState::get_rent_exempt_reserve(&rent).max(1),
            );

            genesis_config.add_account(
                account.stake_account.pubkey(),
                stake_state::create_account(
                    &account.identity.pubkey(),
                    &account.vote_account.pubkey(),
                    &vote_account,
                    &rent,
                    bootstrap_validator_stake_lamports,
                ),
            );

            genesis_config.add_account(account.vote_account.pubkey(), vote_account);
        }

        if let Some(faucet_keypair) = &self.faucet_keypair {
            genesis_config.add_account(
                faucet_keypair.pubkey(),
                AccountSharedData::new(faucet_lamports, 0, &system_program::id()),
            );
        }

        solana_stake_program::add_genesis_accounts(&mut genesis_config);
        if genesis_config.cluster_type == ClusterType::Development {
            solana_runtime::genesis_utils::activate_all_features(&mut genesis_config);
        }

        let issued_lamports = genesis_config
            .accounts
            .values()
            .map(|account| account.lamports)
            .sum::<u64>();

        add_genesis_accounts(&mut genesis_config, issued_lamports - faucet_lamports);

        let parse_address = |address: &str, input_type: &str| {
            address.parse::<Pubkey>().unwrap_or_else(|err| {
                eprintln!("Error: invalid {input_type} {address}: {err}");
                process::exit(1);
            })
        };

        let parse_program_data = |program: &str| {
            let mut program_data = vec![];
            File::open(program)
                .and_then(|mut file| file.read_to_end(&mut program_data))
                .unwrap_or_else(|err| {
                    eprintln!("Error: failed to read {program}: {err}");
                    process::exit(1);
                });
            program_data
        };

        // add in spl stuff
        let spl_file = SOLANA_ROOT.join("spl-genesis-args.sh");
        // Check if the file exists before reading it
        if let Ok(_) = std::fs::metadata(&spl_file) {
            let parsed_args = match parse_spl_genesis_file(&spl_file) {
                Ok(args) => args,
                Err(err) => return Err(err),
            };
        
            // Now you have a HashMap where the keys are flags and the values are vectors of values.
            // You can access them as needed.
            if let Some(values) = parsed_args.get("--bpf-program") {
                for value in values {
                    match value {
                        SPLGenesisArgType::BpfProgram(address, loader, program) => {
                            info!(
                                "Flag: --bpf-program, Address: {}, Loader: {}, Program: {}",
                                address, loader, program
                            );
                            let address = parse_address(address, "address");
                            let loader = parse_address(loader, "loader");
                            let program_data = parse_program_data(program);
                            genesis_config.add_account(
                                address,
                                AccountSharedData::from(Account {
                                    lamports: genesis_config.rent.minimum_balance(program_data.len()),
                                    data: program_data,
                                    executable: true,
                                    owner: loader,
                                    rent_epoch: 0,
                                }),
                            );
                        }
                        _ => panic!("Incorrect number of values passed in for --bpf-program")
                    }
                }
            }
            if let Some(values) =  parsed_args.get("upgradeable-program") {
                for value in values {
                    match value {
                        SPLGenesisArgType::UpgradeableProgram(address, loader, program, upgrade_authority) => {
                            info!(
                                "Flag: --upgradeable-program, Address: {}, Loader: {}, Program: {}, upgrade_authority: {}",
                                address, loader, program, upgrade_authority
                            );
                            let address = parse_address(address, "address");
                            let loader = parse_address(loader, "loader");
                            let program_data_elf = parse_program_data(program);
                            let upgrade_authority_address = if upgrade_authority == "none" {
                                Pubkey::default()
                            } else {
                                upgrade_authority.parse::<Pubkey>().unwrap_or_else(|_| {
                                    read_keypair_file(upgrade_authority)
                                        .map(|keypair| keypair.pubkey())
                                        .unwrap_or_else(|err| {
                                            eprintln!(
                                                "Error: invalid upgrade_authority {upgrade_authority}: {err}"
                                            );
                                            process::exit(1);
                                        })
                                })
                            };
                
                            let (programdata_address, _) =
                                Pubkey::find_program_address(&[address.as_ref()], &loader);
                            let mut program_data = bincode::serialize(&UpgradeableLoaderState::ProgramData {
                                slot: 0,
                                upgrade_authority_address: Some(upgrade_authority_address),
                            })
                            .unwrap();
                            program_data.extend_from_slice(&program_data_elf);
                            genesis_config.add_account(
                                programdata_address,
                                AccountSharedData::from(Account {
                                    lamports: genesis_config.rent.minimum_balance(program_data.len()),
                                    data: program_data,
                                    owner: loader,
                                    executable: false,
                                    rent_epoch: 0,
                                }),
                            );
                
                            let program_data = bincode::serialize(&UpgradeableLoaderState::Program {
                                programdata_address,
                            })
                            .unwrap();
                            genesis_config.add_account(
                                address,
                                AccountSharedData::from(Account {
                                    lamports: genesis_config.rent.minimum_balance(program_data.len()),
                                    data: program_data,
                                    owner: loader,
                                    executable: true,
                                    rent_epoch: 0,
                                }),
                            );
                        }
                        _ => panic!("Incorrect number of values passed in for --upgradeable-program")
                    }
                }
            }
        }

        // should probably create new implementation that writes this directly to a configmap yaml
        // or at least a base64 file
        create_new_ledger(
            &ledger_path,
            &genesis_config,
            max_genesis_archive_unpacked_size,
            LedgerColumnOptions::default(),
        )?;

        self.genesis_config = Some(genesis_config);

        // genesis
        // read in the three bootstrap keys
        Ok(())
    }

    pub fn load_genesis_to_base64_from_file(&self) -> Result<String, Box<dyn Error>> {
        let path = self
            .config_dir
            .join("bootstrap-validator")
            .join(DEFAULT_GENESIS_FILE);
        let mut input_content = Vec::new();
        File::open(path)?.read_to_end(&mut input_content)?;
        Ok(general_purpose::STANDARD.encode(input_content))
    }

    // should be run inside pod
    pub fn verify_genesis_from_file(&self) -> Result<(), Box<dyn Error>> {
        let path = self.config_dir.join("bootstrap-validator");
        let loaded_config = GenesisConfig::load(&path)?;
        info!("hash of loaded genesis: {}", loaded_config.hash());
        match &self.genesis_config {
            Some(config) => assert_eq!(config.hash(), loaded_config.hash()),
            None => return Err(boxed_error!("No genesis config set in Genesis struct")),
        };
        Ok(())
    }

    pub fn ensure_no_dup_pubkeys(&self) {
        // Ensure there are no duplicated pubkeys
        info!("len of pubkeys: {}", self.all_pubkeys.len());
        let mut v = self.all_pubkeys.clone();
        v.sort();
        v.dedup();
        if v.len() != self.all_pubkeys.len() {
            panic!("Error: validator pubkeys have duplicates!");
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            signature::{Keypair, Signer},
            pubkey::Pubkey,
        },
        std::path::PathBuf,
    };
    fn make_tmp_path(name: &str) -> PathBuf {
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();

        let path = [
            out_dir,
            "tmp".to_string(),
            format!("{}-{}", name, keypair.pubkey()),
        ]
        .iter()
        .collect();

        // whack any possible collision
        let _ignored = std::fs::remove_dir_all(&path);
        // whack any possible collision
        let _ignored = std::fs::remove_file(&path);

        path
    }

    #[test]
    fn test_genesis_config() {
        let faucet_keypair = Keypair::new();
        let mut config = GenesisConfig::default();
        config.add_account(
            faucet_keypair.pubkey(),
            AccountSharedData::new(10_000, 0, &Pubkey::default()),
        );
        config.add_account(
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::new(1, 0, &Pubkey::default()),
        );
        config.add_native_instruction_processor("hi".to_string(), solana_sdk::pubkey::new_rand());

        assert_eq!(config.accounts.len(), 2);
        assert!(config
            .accounts
            .iter()
            .any(|(pubkey, account)| *pubkey == faucet_keypair.pubkey()
                && account.lamports == 10_000));

        let path = &make_tmp_path("genesis_config");
        config.write(path).expect("write");
        let loaded_config = GenesisConfig::load(path).expect("load");
        assert_eq!(config.hash(), loaded_config.hash());
        let _ignored = std::fs::remove_file(path);
    }
}

/*
1) Create bootstrap validator keys ->
*/
