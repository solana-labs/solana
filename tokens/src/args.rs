use clap::ArgMatches;
use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::{error::Error, sync::Arc};

pub struct DistributeTokensArgs<P, K> {
    pub input_csv: String,
    pub from_bids: bool,
    pub transaction_db: String,
    pub dollars_per_sol: Option<f64>,
    pub dry_run: bool,
    pub sender_keypair: K,
    pub fee_payer: K,
    pub stake_args: Option<StakeArgs<P, K>>,
}

pub struct StakeArgs<P, K> {
    pub sol_for_fees: f64,
    pub stake_account_address: P,
    pub stake_authority: K,
    pub withdraw_authority: K,
}

pub struct BalancesArgs {
    pub input_csv: String,
    pub from_bids: bool,
    pub dollars_per_sol: Option<f64>,
}

pub struct TransactionLogArgs {
    pub transaction_db: String,
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
        stake_authority: signer_from_path(
            &matches,
            &args.stake_authority,
            "stake authority",
            wallet_manager,
        )
        .unwrap(),
        withdraw_authority: signer_from_path(
            &matches,
            &args.withdraw_authority,
            "withdraw authority",
            wallet_manager,
        )
        .unwrap(),
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
                transaction_db: args.transaction_db,
                dollars_per_sol: args.dollars_per_sol,
                dry_run: args.dry_run,
                sender_keypair: signer_from_path(
                    &matches,
                    &args.sender_keypair,
                    "sender",
                    &mut wallet_manager,
                )
                .unwrap(),
                fee_payer: signer_from_path(
                    &matches,
                    &args.fee_payer,
                    "fee-payer",
                    &mut wallet_manager,
                )
                .unwrap(),
                stake_args: resolved_stake_args.map_or(Ok(None), |r| r.map(Some))?,
            };
            Ok(Command::DistributeTokens(resolved_args))
        }
        Command::Balances(args) => Ok(Command::Balances(args)),
        Command::TransactionLog(args) => Ok(Command::TransactionLog(args)),
    }
}
