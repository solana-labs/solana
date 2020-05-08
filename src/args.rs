use clap::ArgMatches;
use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::{error::Error, sync::Arc};

pub struct DistributeTokensArgs<P, K> {
    pub input_csv: String,
    pub from_bids: bool,
    pub transactions_db: String,
    pub dollars_per_sol: Option<f64>,
    pub dry_run: bool,
    pub no_wait: bool,
    pub sender_keypair: Option<K>,
    pub fee_payer: Option<K>,
    pub force: bool,
    pub stake_args: Option<StakeArgs<P, K>>,
}

pub struct StakeArgs<P, K> {
    pub sol_for_fees: f64,
    pub stake_account_address: P,
    pub stake_authority: Option<K>,
    pub withdraw_authority: Option<K>,
}

pub struct BalancesArgs {
    pub input_csv: String,
    pub from_bids: bool,
    pub dollars_per_sol: Option<f64>,
}

pub struct TransactionLogArgs {
    pub transactions_db: String,
    pub output_path: String,
}

pub enum Command<P, K> {
    DistributeTokens(DistributeTokensArgs<P, K>),
    Balances(BalancesArgs),
    TransactionLog(TransactionLogArgs),
}

pub struct Args<P, K> {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command<P, K>,
}

pub fn resolve_stake_args(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    args: StakeArgs<String, String>,
) -> Result<StakeArgs<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    let resolved_args = StakeArgs {
        stake_account_address: pubkey_from_path(
            &matches,
            &args.stake_account_address,
            "stake account address",
            wallet_manager,
        )
        .unwrap(),
        sol_for_fees: args.sol_for_fees,
        stake_authority: args.stake_authority.as_ref().map(|key_url| {
            signer_from_path(&matches, &key_url, "stake authority", wallet_manager).unwrap()
        }),
        withdraw_authority: args.withdraw_authority.as_ref().map(|key_url| {
            signer_from_path(&matches, &key_url, "withdraw authority", wallet_manager).unwrap()
        }),
    };
    Ok(resolved_args)
}

pub fn resolve_command(
    command: Command<String, String>,
) -> Result<Command<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    match command {
        Command::DistributeTokens(args) => {
            let mut wallet_manager = maybe_wallet_manager()?;
            let matches = ArgMatches::default();
            let resolved_stake_args = args
                .stake_args
                .map(|args| resolve_stake_args(&mut wallet_manager, args));
            let resolved_args = DistributeTokensArgs {
                input_csv: args.input_csv,
                from_bids: args.from_bids,
                transactions_db: args.transactions_db,
                dollars_per_sol: args.dollars_per_sol,
                dry_run: args.dry_run,
                no_wait: args.no_wait,
                sender_keypair: args.sender_keypair.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "sender", &mut wallet_manager).unwrap()
                }),
                fee_payer: args.fee_payer.as_ref().map(|key_url| {
                    signer_from_path(&matches, &key_url, "fee-payer", &mut wallet_manager).unwrap()
                }),
                force: args.force,
                stake_args: resolved_stake_args.map_or(Ok(None), |r| r.map(Some))?,
            };
            Ok(Command::DistributeTokens(resolved_args))
        }
        Command::Balances(args) => Ok(Command::Balances(args)),
        Command::TransactionLog(args) => Ok(Command::TransactionLog(args)),
    }
}
