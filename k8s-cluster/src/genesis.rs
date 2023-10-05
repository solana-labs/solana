//! A command-line executable for generating the chain's genesis config.
#![allow(clippy::arithmetic_side_effects)]

use {
    crate::{boxed_error, initialize_globals, SOLANA_ROOT, LEDGER_DIR},
    base64::{engine::general_purpose, Engine as _},
    bip39::{Language, Mnemonic, MnemonicType, Seed},
    bzip2::{write::BzEncoder, Compression},
    log::*,
    solana_clap_v3_utils::{input_parsers::STDOUT_OUTFILE_TOKEN, keygen},
    solana_sdk::{
        genesis_config::{ClusterType, GenesisConfig, DEFAULT_GENESIS_FILE},
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        signature::{keypair_from_seed, write_keypair, write_keypair_file, Keypair, Signer},
    },
    std::{
        error::Error,
        fs::File,
        io::Read,
        path::PathBuf,
        process::Command,
    },
    tar::Builder,
};

pub const DEFAULT_WORD_COUNT: usize = 12;
pub const DEFAULT_FAUCET_LAMPORTS: u64 = 500000000000000000;
pub const DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE: u64 = 1073741824;
pub const DEFAULT_CLUSTER_TYPE: ClusterType = ClusterType::Development;
pub const DEFAULT_COMMISSION: u8 = 100;
pub const DEFAULT_INTERNAL_NODE_STAKE_SOL: f64 = 10.0; // 10000000000 lamports
pub const DEFAULT_INTERNAL_NODE_SOL: f64 = 500.0; // 500000000000 lamports
pub const DEFAULT_BOOTSTRAP_NODE_STAKE_SOL: f64 = 10.0;
pub const DEFAULT_BOOTSTRAP_NODE_SOL: f64 = 500.0;

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

pub struct GenesisFlags {
    pub hashes_per_tick: String,
    pub slots_per_epoch: Option<u64>,
    pub target_lamports_per_signature: Option<u64>,
    pub faucet_lamports: Option<u64>,
    pub enable_warmup_epochs: bool,
    pub max_genesis_archive_unpacked_size: Option<u64>,
    pub cluster_type: ClusterType,
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
             cluster_type: {:?}\n\
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
        number_of_accounts: i32,
    ) -> Result<(), Box<dyn Error>> {
        let mut filename_prefix = "validator".to_string();
        if validator_type == "bootstrap" {
            filename_prefix = format!("{}-{}", validator_type, filename_prefix);
        } else if validator_type == "validator" {
            filename_prefix = "validator".to_string();
        } else {
            return Err(boxed_error!(format!(
                "Invalid validator type: {}",
                validator_type
            )));
        }

        for i in 0..number_of_accounts {
            self.generate_account(validator_type, filename_prefix.as_str(), i)?;
        }

        Ok(())
    }

    // Create identity, stake, and vote accounts
    fn generate_account(
        &mut self,
        validator_type: &str,
        filename_prefix: &str,
        i: i32,
    ) -> Result<(), Box<dyn Error>> {
        let account_types = vec!["identity", "vote-account", "stake-account"];
        for account in account_types {
            let filename: String;
            if validator_type == "bootstrap" {
                filename = format!("{}/{}.json", filename_prefix, account);
            } else if validator_type == "validator" {
                filename = format!("{}-{}-{}.json", filename_prefix, account, i);
            } else {
                return Err(boxed_error!(format!(
                    "Invalid validator type: {}",
                    validator_type
                )));
            }

            let outfile = self.config_dir.join(filename);
            trace!("outfile: {:?}", outfile);

            let keypair = match generate_keypair() {
                Ok(keypair) => keypair,
                Err(err) => return Err(err),
            };
            self.all_pubkeys.push(keypair.pubkey());

            if let Some(outfile) = outfile.to_str() {
                output_keypair(&keypair, outfile, "new")
                    .map_err(|err| format!("Unable to write {outfile}: {err}"))?;
            }
        }

        Ok(())
    }

    fn setup_genesis_flags(&self) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();

        args.push("--bootstrap-validator-lamports".to_string());
        let bootstrap_validator_lamports = match self.flags.bootstrap_validator_sol {
            Some(sol) => sol_to_lamports(sol),
            None => sol_to_lamports(DEFAULT_BOOTSTRAP_NODE_SOL),
        };
        args.push(bootstrap_validator_lamports.to_string());

        args.push("--bootstrap-validator-stake-lamports".to_string());
        let bootstrap_validator_stake_lamports = match self.flags.bootstrap_validator_stake_sol {
            Some(sol) => sol_to_lamports(sol),
            None => sol_to_lamports(DEFAULT_BOOTSTRAP_NODE_STAKE_SOL),
        };
        args.push(bootstrap_validator_stake_lamports.to_string());

        args.extend(vec![
            "--hashes-per-tick".to_string(),
            self.flags.hashes_per_tick.clone(),
        ]);

        args.push("--max-genesis-archive-unpacked-size".to_string());
        match self.flags.max_genesis_archive_unpacked_size {
            Some(size) => args.push(size.to_string()),
            None => args.push(DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE.to_string()),
        }

        if self.flags.enable_warmup_epochs {
            args.push("--enable-warmup-epochs".to_string());
        }

        args.push("--faucet-lamports".to_string());
        match self.flags.faucet_lamports {
            Some(lamports) => args.push(lamports.to_string()),
            None => args.push(DEFAULT_FAUCET_LAMPORTS.to_string()),
        }

        args.extend(vec![
            "--faucet-pubkey".to_string(),
            self.config_dir
                .join("faucet.json")
                .to_string_lossy()
                .to_string(),
        ]);
        args.extend(vec![
            "--cluster-type".to_string(),
            self.flags.cluster_type.to_string(),
        ]);
        args.extend(vec![
            "--ledger".to_string(),
            self.config_dir
                .join("bootstrap-validator")
                .to_string_lossy()
                .to_string(),
        ]);

        // Order of accounts matters here!!
        args.extend(vec![
            "--bootstrap-validator".to_string(),
            self.config_dir
                .join("bootstrap-validator/identity.json")
                .to_string_lossy()
                .to_string(),
            self.config_dir
                .join("bootstrap-validator/vote-account.json")
                .to_string_lossy()
                .to_string(),
            self.config_dir
                .join("bootstrap-validator/stake-account.json")
                .to_string_lossy()
                .to_string(),
        ]);

        if let Some(slots_per_epoch) = self.flags.slots_per_epoch {
            args.extend(vec![
                "--slots-per-epoch".to_string(),
                slots_per_epoch.to_string(),
            ]);
        }

        if let Some(lamports_per_signature) = self.flags.target_lamports_per_signature {
            args.extend(vec![
                "--target-lamports-per-signature".to_string(),
                lamports_per_signature.to_string(),
            ]);
        }

        args

        //TODO see multinode-demo.sh. we need spl-genesis-args.sh
    }

    pub fn generate(&mut self) -> Result<(), Box<dyn Error>> {
        let args = self.setup_genesis_flags();
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
    // are not included in the genesis.tar.bz2. So we package everything including genesis.tar.bz2
    // snapshots, etc into genesis-package.tar.bz2 and we use this as our genesis in the bootstrap
    // validator
    pub fn package_up(&mut self) -> Result<(), Box<dyn Error>> {
        info!("Packaging genesis");
        let tar_bz2_file = File::create(LEDGER_DIR.join("genesis-package.tar.bz2"))?;
        let encoder = BzEncoder::new(tar_bz2_file, Compression::best());
        let mut tar_builder = Builder::new(encoder);
        tar_builder.append_dir_all(".", &*LEDGER_DIR)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            pubkey::Pubkey,
            signature::{Keypair, Signer},
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
