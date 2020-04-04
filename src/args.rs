use clap::ArgMatches;
use solana_clap_utils::keypair::signer_from_path;
use solana_remote_wallet::remote_wallet::maybe_wallet_manager;
use solana_sdk::signature::Signer;
use std::error::Error;

pub struct DistributeArgs<K> {
    pub allocations_csv: String,
    pub transactions_csv: String,
    pub dollars_per_sol: f64,
    pub dry_run: bool,
    pub sender_keypair: Option<K>,
    pub fee_payer: Option<K>,
}

pub struct BalancesArgs {
    pub allocations_csv: String,
    pub dollars_per_sol: f64,
}

pub enum Command<K> {
    Distribute(DistributeArgs<K>),
    Balances(BalancesArgs),
}

pub struct Args<K> {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command<K>,
}

pub fn resolve_command(
    command: Command<String>,
) -> Result<Command<Box<dyn Signer>>, Box<dyn Error>> {
    match command {
        Command::Distribute(args) => {
            let wallet_manager = maybe_wallet_manager()?;
            let wallet_manager = wallet_manager.as_ref();
            let matches = ArgMatches::default();
            let resolved_args = DistributeArgs {
                allocations_csv: args.allocations_csv,
                transactions_csv: args.transactions_csv,
                dollars_per_sol: args.dollars_per_sol,
                dry_run: args.dry_run,
                sender_keypair: args.sender_keypair.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "sender", wallet_manager).unwrap()
                }),
                fee_payer: args.fee_payer.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "fee-payer", wallet_manager).unwrap()
                }),
            };
            Ok(Command::Distribute(resolved_args))
        }
        Command::Balances(args) => Ok(Command::Balances(args)),
    }
}
