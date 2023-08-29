use {
    clap::ArgMatches,
    crate::initialize_globals,
    bip39::{Mnemonic, MnemonicType, Seed, Language},
    solana_sdk::signature::{
        Keypair,
        keypair_from_seed,
        Signer,
    },
    solana_clap_v3_utils::{
        keygen,
        keypair,
    },
};

#[derive(Clone, Debug)]
pub struct SetupConfig<'a> {
    pub namespace: &'a str,
    pub num_validators: i32,
    pub prebuild_genesis: bool,
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
        for _ in 0..total_validators {
            let (passphrase, passphrase_message) = keygen::mnemonic::no_passphrase_and_message();

            let word_count: usize = 12; //default
            let mnemonic_type = MnemonicType::for_word_count(word_count).unwrap();
            let mnemonic = Mnemonic::new(mnemonic_type, Language::English);
            let seed = Seed::new(&mnemonic, &passphrase);
            let keypair = keypair_from_seed(seed.as_bytes()).unwrap();

            let phrase: &str = mnemonic.phrase();
            let divider = String::from_utf8(vec![b'='; phrase.len()]).unwrap();
            println!(
                "{}\npubkey: {}\n{}\nSave this seed phrase{} to recover your new keypair:\n{}\n{}",
                &divider, keypair.pubkey(), &divider, passphrase_message, phrase, &divider
            );
        }
    }
}