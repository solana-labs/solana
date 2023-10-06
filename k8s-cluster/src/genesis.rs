//! A command-line executable for generating the chain's genesis config.
#![allow(clippy::arithmetic_side_effects)]

use {
    crate::{boxed_error, initialize_globals, LEDGER_DIR, SOLANA_ROOT, ValidatorType},
    base64::{engine::general_purpose, Engine as _},
    bip39::{Language, Mnemonic, MnemonicType, Seed},
    bzip2::{write::BzEncoder, Compression},
    log::*,
    solana_clap_v3_utils::{input_parsers::STDOUT_OUTFILE_TOKEN, keygen},
    solana_sdk::{
        genesis_config::{GenesisConfig, DEFAULT_GENESIS_FILE},
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        signature::{keypair_from_seed, write_keypair, write_keypair_file, Keypair, Signer},
    },
    std::{error::Error, fs::File, io::Read, path::PathBuf, process::Command},
    tar::Builder,
};

pub const DEFAULT_WORD_COUNT: usize = 12;
pub const DEFAULT_FAUCET_LAMPORTS: u64 = 500000000000000000;
pub const DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE: u64 = 1073741824;
pub const DEFAULT_COMMISSION: u8 = 100;
pub const DEFAULT_INTERNAL_NODE_STAKE_SOL: f64 = 10.0; // 10000000000 lamports
pub const DEFAULT_INTERNAL_NODE_SOL: f64 = 500.0; // 500000000000 lamports
pub const DEFAULT_BOOTSTRAP_NODE_STAKE_SOL: f64 = 10.0;
pub const DEFAULT_BOOTSTRAP_NODE_SOL: f64 = 500.0;

fn output_keypair(keypair: &Keypair, outfile: &str) -> Result<(), Box<dyn Error>> {
    if outfile == STDOUT_OUTFILE_TOKEN {
        let mut stdout = std::io::stdout();
        write_keypair(keypair, &mut stdout)?;
    } else {
        write_keypair_file(keypair, outfile)?;
        // println!("Wrote {source} keypair to {outfile}");
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

fn fetch_spl(fetch_spl_file: &PathBuf) -> Result<(), Box<dyn Error>> {
    let output = Command::new("bash")
        .arg(fetch_spl_file)
        .output() // Capture the output of the script
        .expect("Failed to run fetch-spl.sh script");

    // Check if the script execution was successful
    if output.status.success() {
        Ok(())
    } else {
        Err(boxed_error!(format!(
            "Failed to fun fetch-spl.sh script: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn parse_spl_genesis_file(spl_file: &PathBuf) -> Result<Vec<String>, Box<dyn Error>> {
    // Read entire file into a String
    let mut file = File::open(spl_file)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    // Split by whitespace
    let mut args = Vec::new();
    let mut tokens_iter = content.split_whitespace();

    while let Some(token) = tokens_iter.next() {
        args.push(token.to_string());
        // Find flag delimiters
        if token.starts_with("--") {
            for next_token in tokens_iter.by_ref() {
                if next_token.starts_with("--") {
                    args.push(next_token.to_string());
                } else {
                    args.push(next_token.to_string());
                    break;
                }
            }
        }
    }

    Ok(args)
}

pub struct GenesisFlags {
    pub hashes_per_tick: String,
    pub slots_per_epoch: Option<u64>,
    pub target_lamports_per_signature: Option<u64>,
    pub faucet_lamports: Option<u64>,
    pub enable_warmup_epochs: bool,
    pub max_genesis_archive_unpacked_size: Option<u64>,
    pub cluster_type: String,
    pub bootstrap_validator_sol: Option<f64>,
    pub bootstrap_validator_stake_sol: Option<f64>,
}

impl std::fmt::Display for GenesisFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "GenesisFlags {{\n\
             hashes_per_tick: {:?},\n\
             slots_per_epoch: {:?},\n\
             target_lamports_per_signature: {:?},\n\
             faucet_lamports: {:?},\n\
             enable_warmup_epochs: {},\n\
             max_genesis_archive_unpacked_size: {:?},\n\
             cluster_type: {}\n\
             bootstrap_validator_sol: {:?},\n\
             bootstrap_validator_stake_sol: {:?},\n\
             }}",
            self.hashes_per_tick,
            self.slots_per_epoch,
            self.target_lamports_per_signature,
            self.faucet_lamports,
            self.enable_warmup_epochs,
            self.max_genesis_archive_unpacked_size,
            self.cluster_type,
            self.bootstrap_validator_sol,
            self.bootstrap_validator_stake_sol,
        )
    }
}

#[derive(Clone, Debug)]
pub struct SetupConfig<'a> {
    pub namespace: &'a str,
    pub num_validators: i32,
    pub prebuild_genesis: bool,
}

pub struct ValidatorAccountKeypairs {
    pub vote_account: Keypair,
    pub identity: Keypair,
    pub stake_account: Keypair,
}

pub struct Genesis {
    pub flags: GenesisFlags,
    pub config_dir: PathBuf,
    pub validator_keypairs: Vec<ValidatorAccountKeypairs>,
    pub faucet_keypair: Option<Keypair>,
    pub genesis_config: Option<GenesisConfig>,
    pub all_pubkeys: Vec<Pubkey>,
}

impl Genesis {
    pub fn new(flags: GenesisFlags) -> Self {
        initialize_globals();
        let config_dir = SOLANA_ROOT.join("config-k8s");
        if config_dir.exists() {
            std::fs::remove_dir_all(&config_dir).unwrap();
        }
        std::fs::create_dir_all(&config_dir).unwrap();
        Genesis {
            flags,
            config_dir,
            validator_keypairs: Vec::default(),
            faucet_keypair: None,
            genesis_config: None,
            all_pubkeys: Vec::default(),
        }
    }

    pub fn generate_faucet(&mut self) -> Result<(), Box<dyn Error>> {
        info!("generating faucet keypair");
        let outfile = self.config_dir.join("faucet.json");
        let keypair = match generate_keypair() {
            Ok(keypair) => keypair,
            Err(err) => return Err(err),
        };

        if let Some(outfile) = outfile.to_str() {
            output_keypair(&keypair, outfile)
                .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
        }
        self.faucet_keypair = Some(keypair);
        Ok(())
    }

    pub fn generate_accounts(
        &mut self,
        validator_type: ValidatorType,
        number_of_accounts: i32,
    ) -> Result<(), Box<dyn Error>> {
        let filename_prefix = match validator_type {
            ValidatorType::Bootstrap => format!("{}-validator", validator_type),
            ValidatorType::Standard => "validator".to_string(),
        };

        for i in 0..number_of_accounts {
            self.generate_account(validator_type, &filename_prefix, i)?;
        }

        Ok(())
    }

    // Create identity, stake, and vote accounts
    fn generate_account(
        &mut self,
        validator_type: ValidatorType,
        filename_prefix: &str,
        i: i32,
    ) -> Result<(), Box<dyn Error>> {
        let log_msg = match validator_type {
            ValidatorType::Bootstrap => format!("generating {} account", validator_type),
            ValidatorType::Standard => format!("generating {} account: {}", validator_type, i),
        };
        info!("{}", log_msg);
        
        let account_types = vec!["identity", "vote-account", "stake-account"];
        for account in account_types {
            let filename = match validator_type {
                ValidatorType::Bootstrap => format!("{}/{}.json", filename_prefix, account),
                ValidatorType::Standard => format!("{}-{}-{}.json", filename_prefix, account, i),
            };

            let outfile = self.config_dir.join(&filename);
            trace!("outfile: {:?}", outfile);

            let keypair = generate_keypair()?;
            self.all_pubkeys.push(keypair.pubkey());

            if let Some(outfile) = outfile.to_str() {
                output_keypair(&keypair, outfile)
                    .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
            }
        }

        Ok(())
    }

    fn setup_genesis_flags(&self) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();

        args.push("--bootstrap-validator-lamports".to_string());
        args.push(sol_to_lamports(self.flags.bootstrap_validator_sol.unwrap_or(DEFAULT_BOOTSTRAP_NODE_SOL)).to_string());

        args.push("--bootstrap-validator-stake-lamports".to_string());
        args.push(sol_to_lamports(self.flags.bootstrap_validator_stake_sol.unwrap_or(DEFAULT_BOOTSTRAP_NODE_STAKE_SOL)).to_string());

        args.push("--hashes-per-tick".to_string());
        args.push(self.flags.hashes_per_tick.clone());
        

        args.push("--max-genesis-archive-unpacked-size".to_string());
        args.push(
            self.flags
                .max_genesis_archive_unpacked_size
                .unwrap_or(DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE)
                .to_string(),
        );

        if self.flags.enable_warmup_epochs {
            args.push("--enable-warmup-epochs".to_string());
        }

        args.push("--faucet-lamports".to_string());
        args.push(
            self.flags
                .faucet_lamports
                .unwrap_or(DEFAULT_FAUCET_LAMPORTS)
                .to_string(),
        );

        args.push("--faucet-pubkey".to_string());
        args.push(self.config_dir.join("faucet.json").to_string_lossy().to_string());

        args.push("--cluster-type".to_string());
        args.push(self.flags.cluster_type.to_string());

        args.push("--ledger".to_string());
        args.push(self.config_dir.join("bootstrap-validator").to_string_lossy().to_string());


        // Order of accounts matters here!!
        args.push("--bootstrap-validator".to_string());
        ["identity", "vote-account", "stake-account"].iter().for_each(|account_type| {
            args.push(self.config_dir.join(format!("bootstrap-validator/{}.json", account_type)).to_string_lossy().to_string());
        });

        if let Some(slots_per_epoch) = self.flags.slots_per_epoch {
            args.push("--slots-per-epoch".to_string());
            args.push(slots_per_epoch.to_string());
        }
        
        if let Some(lamports_per_signature) = self.flags.target_lamports_per_signature {
            args.push("--target-lamports-per-signature".to_string());
            args.push(lamports_per_signature.to_string());
        }        

        args
    }

    pub fn setup_spl_args(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let fetch_spl_file = SOLANA_ROOT.join("fetch-spl.sh");
        fetch_spl(&fetch_spl_file)?;

        // add in spl stuff
        let spl_file = SOLANA_ROOT.join("spl-genesis-args.sh");
        parse_spl_genesis_file(&spl_file)
    }

    pub fn generate(&mut self) -> Result<(), Box<dyn Error>> {
        let mut args = self.setup_genesis_flags();
        let mut spl_args = self.setup_spl_args()?;
        args.append(&mut spl_args);

        debug!("genesis args: ");
        for arg in &args {
            debug!("{}", arg);
        }

        let output = Command::new("solana-genesis")
            .args(&args)
            .output()
            .expect("Failed to execute solana-genesis");

        if !output.status.success() {
            return Err(boxed_error!(format!(
                "Failed to create genesis. err: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
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

    // solana-genesis creates a genesis.tar.bz2 but if we need to create snapshots, these
    // are not included in the genesis.tar.bz2. So we package everything including genesis,
    // snapshots, etc into genesis-package.tar.bz2 and we use this as our genesis in the
    // bootstrap validator
    pub fn package_up(&mut self) -> Result<(), Box<dyn Error>> {
        // delete the genesis.tar.bz2 since its contents will be included in the
        // tar we're about to create
        let genesis_path = LEDGER_DIR.join("genesis.tar.bz2");
        if std::fs::metadata(&genesis_path).is_ok() {
            std::fs::remove_file(&genesis_path)?;
        }

        info!("Packaging genesis");
        let folder_to_tar = LEDGER_DIR.join("");
        let tar_bz2_file = File::create(SOLANA_ROOT.join("config-k8s/genesis-package.tar.bz2"))?;
        let encoder = BzEncoder::new(tar_bz2_file, Compression::best());
        let mut tar_builder = Builder::new(encoder);
        tar_builder.append_dir_all("ledger", folder_to_tar)?;

        Ok(())
    }
}
