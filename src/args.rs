//use clap::ArgMatches;
//use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
//use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
//use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::error::Error;
//use std::sync::Arc;

pub(crate) struct ScrubArgs {
    pub input_csv: String,
}

pub(crate) struct TransferArgs {
    pub input_csv: String,
    pub state_csv: String,
    pub dry_run: bool,
}

pub(crate) enum Command {
    Scrub(ScrubArgs),
    Transfer(TransferArgs),
}

pub(crate) struct Args {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command,
}

//fn resolve_fee_payer(
//    wallet_manager: Option<&Arc<RemoteWalletManager>>,
//    key_url: &str,
//) -> Result<Box<dyn Signer>, Box<dyn Error>> {
//    let matches = ArgMatches::default();
//    signer_from_path(&matches, key_url, "fee-payer", wallet_manager)
//}
//
//fn resolve_base_pubkey(
//    wallet_manager: Option<&Arc<RemoteWalletManager>>,
//    key_url: &str,
//) -> Result<Pubkey, Box<dyn Error>> {
//    let matches = ArgMatches::default();
//    pubkey_from_path(&matches, key_url, "base pubkey", wallet_manager)
//}

pub(crate) fn resolve_command(command: &Command) -> Result<Command, Box<dyn Error>> {
    //let wallet_manager = maybe_wallet_manager()?;
    //let wallet_manager = wallet_manager.as_ref();
    //let matches = ArgMatches::default();
    match command {
        Command::Scrub(args) => {
            let resolved_args = ScrubArgs {
                input_csv: args.input_csv.clone(),
            };
            Ok(Command::Scrub(resolved_args))
        }
        Command::Transfer(args) => {
            let resolved_args = TransferArgs {
                input_csv: args.input_csv.clone(),
                state_csv: args.state_csv.clone(),
                dry_run: args.dry_run.clone(),
            };
            Ok(Command::Transfer(resolved_args))
        }
    }
}
