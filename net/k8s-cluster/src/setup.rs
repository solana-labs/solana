use crate::SOLANA_ROOT;

use {
    clap::ArgMatches,
    crate::initialize_globals,
    bip39::{Mnemonic, MnemonicType, Seed, Language},
    log::*,
    solana_sdk::signature::{
        Keypair,
        keypair_from_seed,
        Signer,
        write_keypair,
        write_keypair_file,
    },
    solana_clap_v3_utils::{
        keygen,
        keypair,
        input_parsers::STDOUT_OUTFILE_TOKEN,
    },
    std::error::Error,

};

#[derive(Clone, Debug)]
pub struct SetupConfig<'a> {
    pub namespace: &'a str,
    pub num_validators: i32,
    pub prebuild_genesis: bool,
}

fn output_keypair(
    keypair: &Keypair,
    outfile: &str,
    source: &str,
) -> Result<(), Box<dyn Error>> {
    if outfile == STDOUT_OUTFILE_TOKEN {
        let mut stdout = std::io::stdout();
        write_keypair(keypair, &mut stdout)?;
    } else {
        write_keypair_file(keypair, outfile)?;
        println!("Wrote {source} keypair to {outfile}");
    }
    Ok(())
}



pub struct Genesis<'a> {
    pub config: SetupConfig<'a>,

}

impl<'a> Genesis<'a> {
    pub fn new(
        setup_config: SetupConfig<'a>,
    ) -> Self {
        initialize_globals();
        Genesis { 
            config: setup_config,
        }
    }

    pub fn generate(
        &self,
    ) {
        let total_validators = self.config.num_validators + 1; // add 1 for bootstrap
        for i in 0..total_validators {
            info!("generating keypair index: {}", i);
            let filename = format!("keypair-validator-{}", i);
            let outfile = SOLANA_ROOT.join(filename);
            let (passphrase, passphrase_message) = keygen::mnemonic::no_passphrase_and_message();

            let word_count: usize = 12; //default
            let mnemonic_type = MnemonicType::for_word_count(word_count).unwrap();
            let mnemonic = Mnemonic::new(mnemonic_type, Language::English);
            let seed = Seed::new(&mnemonic, &passphrase);
            let keypair = keypair_from_seed(seed.as_bytes()).unwrap();

            if let Some(outfile) = outfile.to_str() {
                output_keypair(&keypair, outfile, "new")
                    .map_err(|err| format!("Unable to write {outfile}: {err}"));
            }

            let phrase: &str = mnemonic.phrase();
            let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
            println!(
                "{}\npubkey: {}\n{}\nSave this seed phrase{} to recover your new keypair:\n{}\n{}",
                &divider, keypair.pubkey(), &divider, passphrase_message, phrase, &divider
            );
        }
    }
}