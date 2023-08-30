use {
    crate::{boxed_error, initialize_globals, SOLANA_ROOT},
    base64::{Engine as _, engine::{self, general_purpose}},
    bip39::{Language, Mnemonic, MnemonicType, Seed},
    clap::ArgMatches,
    log::*,
    solana_accounts_db::hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
    solana_clap_v3_utils::{input_parsers::STDOUT_OUTFILE_TOKEN, keygen, keypair},
    solana_entry::poh::{compute_hashes_per_tick, Poh},
    solana_genesis::genesis_accounts::add_genesis_accounts,
    solana_ledger::{blockstore::create_new_ledger, blockstore_options::LedgerColumnOptions},
    solana_sdk::{
        account::AccountSharedData,
        clock,
        epoch_schedule::EpochSchedule,
        genesis_config::{ClusterType, GenesisConfig, DEFAULT_GENESIS_FILE},
        native_token::sol_to_lamports,
        poh_config::PohConfig,
        pubkey::Pubkey,
        rent::Rent,
        signature::{
            keypair_from_seed, read_keypair_file, write_keypair, write_keypair_file, Keypair,
            Signer,
        },
        stake::state::StakeStateV2,
        system_program, timing,
    },
    solana_stake_program::stake_state,
    solana_vote_program::vote_state::{self, VoteState},
    std::{
        error::Error, 
        fs::File,
        io::{
            Read, Write,
        },
        path::PathBuf, 
        process, 
        time::Duration,
    },
};

pub const DEFAULT_WORD_COUNT: usize = 12;

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
    
}

impl<'a> Genesis<'a> {
    pub fn new(setup_config: SetupConfig<'a>) -> Self {
        initialize_globals();
        Genesis {
            config: setup_config,
            config_dir: SOLANA_ROOT.join("config-k8s"),
            validator_keypairs: Vec::default(),
            faucet_keypair: None,
            genesis_config: None
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

    pub fn generate_accounts(&mut self, validator_type: &str) -> Result<(), Box<dyn Error>> {
        let mut accounts = vec!["identity", "vote-account", "stake-account"];
        let mut filename_prefix = "validator".to_string();
        if validator_type == "bootstrap" {
            filename_prefix = format!("{}-{}", validator_type, filename_prefix);
        }
        let mut identity: Option<Keypair> = None;
        let mut vote: Option<Keypair> = None;
        let mut stake: Option<Keypair> = None;
        for account in accounts {
            let filename = format!("{}/{}.json", filename_prefix, account);
            let outfile = self.config_dir.join(filename);
            info!("outfile: {:?}", outfile);

            let keypair = match generate_keypair() {
                Ok(keypair) => keypair,
                Err(err) => return Err(err),
            };

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

        self.validator_keypairs.push(ValidatorAccountKeypairs {
            vote_account: vote.ok_or_else(|| boxed_error!("vote-account keypair not initialized"))?,
            identity: identity.ok_or_else(|| boxed_error!("identity keypair not initialized"))?,
            stake_account: stake.ok_or_else(|| boxed_error!("stake-account keypair not initialized"))?,
        });

        Ok(())
    }

    pub fn generate(&mut self) -> Result<(), Box<dyn Error>> {
        let ledger_path = self.config_dir.join("bootstrap-validator");
        let rent = Rent::default();

        // vote account
        let default_bootstrap_validator_lamports = &sol_to_lamports(500.0)
            .max(VoteState::get_rent_exempt_reserve(&rent))
            .to_string();
        let bootstrap_validator_lamports: u64 =
            default_bootstrap_validator_lamports.parse().unwrap(); //TODO enable command line arg

        // stake account
        let default_bootstrap_validator_stake_lamports = &sol_to_lamports(0.5)
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

        let epoch_schedule = EpochSchedule::custom(
            slots_per_epoch,
            slots_per_epoch,
            true, //  TODO: fix for flag
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

    // convert binary file to base64
    pub fn setup_config_map(
        &self,
    ) -> Result<(), Box<dyn Error>> {
        self.verify_genesis_from_file()?;
        let path = self.config_dir.join("bootstrap-validator").join(DEFAULT_GENESIS_FILE);
        let mut input_content = Vec::new();
        File::open(path)?.read_to_end(&mut input_content)?;
        let base64_content = general_purpose::STANDARD.encode(input_content);

        let outpath = self.config_dir.join("bootstrap-validator").join("genesis-config-map.yaml");


        Ok(())

    }

    pub fn load_genesis_to_base64_from_file(
        &self,
    ) -> Result<String, Box<dyn Error>> {
        let path = self.config_dir.join("bootstrap-validator").join(DEFAULT_GENESIS_FILE);
        let mut input_content = Vec::new();
        File::open(path)?.read_to_end(&mut input_content)?;
        Ok(general_purpose::STANDARD.encode(input_content))
    }

    pub fn load_genesis_from_config_map(
        &self,
    ) {
        
    }


    // should be run inside pod
    pub fn verify_genesis_from_file(&self) -> Result<(), Box<dyn Error>> {
        let path = self.config_dir.join("bootstrap-validator");
        // let loaded_config = GenesisConfig::load(&path);
        // let loaded_config = match GenesisConfig::load(&path) {
        //     Ok(config) => config,
        //     Err(err) => return Err(boxed_error!(format!("Failed to load genesis config from file! err: {}", err))),
        // };

        let loaded_config = GenesisConfig::load(&path)?;
        info!("hash of loaded genesis: {}", loaded_config.hash());
        match &self.genesis_config {
            Some(config) => assert_eq!(config.hash(), loaded_config.hash()),
            None => return Err(boxed_error!("No genesis config set in Genesis struct")),
        };
        Ok(())
    }
}

mod tests {
    use {
        super::*,
        solana_sdk::signature::{Keypair, Signer},
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
