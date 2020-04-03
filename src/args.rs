use clap::ArgMatches;
use solana_clap_utils::keypair::signer_from_path;
use solana_remote_wallet::remote_wallet::maybe_wallet_manager;
use solana_sdk::signature::Signer;
use std::error::Error;

pub(crate) struct ScrubArgs {
    pub input_csv: String,
}

pub(crate) struct TransferArgs<K> {
    pub input_csv: String,
    pub state_csv: String,
    pub dry_run: bool,
    pub sender_keypair: Option<K>,
    pub fee_payer: Option<K>,
}

pub(crate) enum Command<K> {
    Scrub(ScrubArgs),
    Transfer(TransferArgs<K>),
}

pub(crate) struct Args<K> {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command<K>,
}

pub(crate) fn resolve_command(
    command: &Command<String>,
) -> Result<Command<Box<dyn Signer>>, Box<dyn Error>> {
    match command {
        Command::Scrub(args) => {
            let resolved_args = ScrubArgs {
                input_csv: args.input_csv.clone(),
            };
            Ok(Command::Scrub(resolved_args))
        }
        Command::Transfer(args) => {
            let wallet_manager = maybe_wallet_manager()?;
            let wallet_manager = wallet_manager.as_ref();
            let matches = ArgMatches::default();
            let resolved_args = TransferArgs {
                input_csv: args.input_csv.clone(),
                state_csv: args.state_csv.clone(),
                dry_run: args.dry_run.clone(),
                sender_keypair: args.sender_keypair.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "sender", wallet_manager).unwrap()
                }),
                fee_payer: args.fee_payer.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "fee-payer", wallet_manager).unwrap()
                }),
            };
            Ok(Command::Transfer(resolved_args))
        }
    }
}
