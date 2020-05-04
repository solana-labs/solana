use clap::ArgMatches;
use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
use solana_remote_wallet::remote_wallet::maybe_wallet_manager;
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::error::Error;

pub struct DistributeTokensArgs<K> {
    pub bids_csv: String,
    pub transactions_db: String,
    pub dollars_per_sol: f64,
    pub dry_run: bool,
    pub sender_keypair: Option<K>,
    pub fee_payer: Option<K>,
}

pub struct DistributeStakeArgs<P, K> {
    pub allocations_csv: String,
    pub transactions_db: String,
    pub dry_run: bool,
    pub stake_account_address: P,
    pub stake_authority: Option<K>,
    pub withdraw_authority: Option<K>,
    pub fee_payer: Option<K>,
}

pub struct BalancesArgs {
    pub bids_csv: String,
    pub dollars_per_sol: f64,
}

pub enum Command<P, K> {
    DistributeTokens(DistributeTokensArgs<K>),
    DistributeStake(DistributeStakeArgs<P, K>),
    Balances(BalancesArgs),
}

pub struct Args<P, K> {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command<P, K>,
}

pub fn resolve_command(
    command: Command<String, String>,
) -> Result<Command<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    match command {
        Command::DistributeTokens(args) => {
            let mut wallet_manager = maybe_wallet_manager()?;
            let matches = ArgMatches::default();
            let resolved_args = DistributeTokensArgs {
                bids_csv: args.bids_csv,
                transactions_db: args.transactions_db,
                dollars_per_sol: args.dollars_per_sol,
                dry_run: args.dry_run,
                sender_keypair: args.sender_keypair.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "sender", &mut wallet_manager).unwrap()
                }),
                fee_payer: args.fee_payer.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "fee-payer", &mut wallet_manager).unwrap()
                }),
            };
            Ok(Command::DistributeTokens(resolved_args))
        }
        Command::DistributeStake(args) => {
            let mut wallet_manager = maybe_wallet_manager()?;
            let matches = ArgMatches::default();
            let resolved_args = DistributeStakeArgs {
                allocations_csv: args.allocations_csv,
                transactions_db: args.transactions_db,
                dry_run: args.dry_run,
                stake_account_address: pubkey_from_path(
                    &matches,
                    &args.stake_account_address,
                    "stake account address",
                    &mut wallet_manager,
                )
                .unwrap(),
                stake_authority: args.stake_authority.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "stake authority", &mut wallet_manager)
                        .unwrap()
                }),
                withdraw_authority: args.withdraw_authority.as_ref().map(|key_url| {
                    signer_from_path(
                        &matches,
                        &key_url,
                        "withdraw authority",
                        &mut wallet_manager,
                    )
                    .unwrap()
                }),
                fee_payer: args.fee_payer.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "fee-payer", &mut wallet_manager).unwrap()
                }),
            };
            Ok(Command::DistributeStake(resolved_args))
        }
        Command::Balances(args) => Ok(Command::Balances(args)),
    }
}
