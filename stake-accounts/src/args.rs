use clap::ArgMatches;
use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
use solana_remote_wallet::remote_wallet::RemoteWalletManager;
use solana_sdk::{
    clock::{Epoch, UnixTimestamp},
    pubkey::Pubkey,
    signature::Signer,
};
use std::error::Error;
use std::sync::Arc;

pub(crate) struct NewArgs<P, K> {
    pub fee_payer: K,
    pub funding_keypair: K,
    pub base_keypair: K,
    pub lamports: u64,
    pub stake_authority: P,
    pub withdraw_authority: P,
    pub index: usize,
}

pub(crate) struct CountArgs<P> {
    pub base_pubkey: P,
}

pub(crate) struct QueryArgs<P> {
    pub base_pubkey: P,
    pub num_accounts: usize,
}

pub(crate) struct AuthorizeArgs<P, K> {
    pub fee_payer: K,
    pub base_pubkey: P,
    pub stake_authority: K,
    pub withdraw_authority: K,
    pub new_stake_authority: P,
    pub new_withdraw_authority: P,
    pub num_accounts: usize,
}

pub(crate) struct SetLockupArgs<P, K> {
    pub fee_payer: K,
    pub base_pubkey: P,
    pub custodian: K,
    pub lockup_epoch: Option<Epoch>,
    pub lockup_date: Option<UnixTimestamp>,
    pub new_custodian: Option<P>,
    pub num_accounts: usize,
}

pub(crate) struct RebaseArgs<P, K> {
    pub fee_payer: K,
    pub base_pubkey: P,
    pub new_base_keypair: K,
    pub stake_authority: K,
    pub num_accounts: usize,
}

pub(crate) struct MoveArgs<P, K> {
    pub rebase_args: RebaseArgs<P, K>,
    pub authorize_args: AuthorizeArgs<P, K>,
}

pub(crate) enum Command<P, K> {
    New(NewArgs<P, K>),
    Count(CountArgs<P>),
    Addresses(QueryArgs<P>),
    Balance(QueryArgs<P>),
    Authorize(AuthorizeArgs<P, K>),
    SetLockup(SetLockupArgs<P, K>),
    Rebase(RebaseArgs<P, K>),
    Move(Box<MoveArgs<P, K>>),
}

pub(crate) struct Args<P, K> {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command<P, K>,
}

fn resolve_stake_authority(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "stake authority", wallet_manager)
}

fn resolve_withdraw_authority(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "withdraw authority", wallet_manager)
}

fn resolve_new_stake_authority(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Pubkey, Box<dyn Error>> {
    let matches = ArgMatches::default();
    pubkey_from_path(&matches, key_url, "new stake authority", wallet_manager)
}

fn resolve_new_withdraw_authority(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Pubkey, Box<dyn Error>> {
    let matches = ArgMatches::default();
    pubkey_from_path(&matches, key_url, "new withdraw authority", wallet_manager)
}

fn resolve_fee_payer(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "fee-payer", wallet_manager)
}

fn resolve_custodian(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "custodian", wallet_manager)
}

fn resolve_new_custodian(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &Option<String>,
) -> Result<Option<Pubkey>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    let pubkey = match key_url {
        None => None,
        Some(key_url) => {
            let pubkey = pubkey_from_path(&matches, key_url, "new custodian", wallet_manager)?;
            Some(pubkey)
        }
    };
    Ok(pubkey)
}

fn resolve_base_pubkey(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Pubkey, Box<dyn Error>> {
    let matches = ArgMatches::default();
    pubkey_from_path(&matches, key_url, "base pubkey", wallet_manager)
}

fn resolve_new_base_keypair(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "new base pubkey", wallet_manager)
}

fn resolve_authorize_args(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    args: &AuthorizeArgs<String, String>,
) -> Result<AuthorizeArgs<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let resolved_args = AuthorizeArgs {
        fee_payer: resolve_fee_payer(wallet_manager, &args.fee_payer)?,
        base_pubkey: resolve_base_pubkey(wallet_manager, &args.base_pubkey)?,
        stake_authority: resolve_stake_authority(wallet_manager, &args.stake_authority)?,
        withdraw_authority: resolve_withdraw_authority(wallet_manager, &args.withdraw_authority)?,
        new_stake_authority: resolve_new_stake_authority(
            wallet_manager,
            &args.new_stake_authority,
        )?,
        new_withdraw_authority: resolve_new_withdraw_authority(
            wallet_manager,
            &args.new_withdraw_authority,
        )?,
        num_accounts: args.num_accounts,
    };
    Ok(resolved_args)
}

fn resolve_set_lockup_args(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    args: &SetLockupArgs<String, String>,
) -> Result<SetLockupArgs<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let resolved_args = SetLockupArgs {
        fee_payer: resolve_fee_payer(wallet_manager, &args.fee_payer)?,
        base_pubkey: resolve_base_pubkey(wallet_manager, &args.base_pubkey)?,
        custodian: resolve_custodian(wallet_manager, &args.custodian)?,
        lockup_epoch: args.lockup_epoch,
        lockup_date: args.lockup_date,
        new_custodian: resolve_new_custodian(wallet_manager, &args.new_custodian)?,
        num_accounts: args.num_accounts,
    };
    Ok(resolved_args)
}

fn resolve_rebase_args(
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
    args: &RebaseArgs<String, String>,
) -> Result<RebaseArgs<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let resolved_args = RebaseArgs {
        fee_payer: resolve_fee_payer(wallet_manager, &args.fee_payer)?,
        base_pubkey: resolve_base_pubkey(wallet_manager, &args.base_pubkey)?,
        new_base_keypair: resolve_new_base_keypair(wallet_manager, &args.new_base_keypair)?,
        stake_authority: resolve_stake_authority(wallet_manager, &args.stake_authority)?,
        num_accounts: args.num_accounts,
    };
    Ok(resolved_args)
}

pub(crate) fn resolve_command(
    command: &Command<String, String>,
) -> Result<Command<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let mut wallet_manager = None;
    let matches = ArgMatches::default();
    match command {
        Command::New(args) => {
            let resolved_args = NewArgs {
                fee_payer: resolve_fee_payer(&mut wallet_manager, &args.fee_payer)?,
                funding_keypair: signer_from_path(
                    &matches,
                    &args.funding_keypair,
                    "funding keypair",
                    &mut wallet_manager,
                )?,
                base_keypair: signer_from_path(
                    &matches,
                    &args.base_keypair,
                    "base keypair",
                    &mut wallet_manager,
                )?,
                stake_authority: pubkey_from_path(
                    &matches,
                    &args.stake_authority,
                    "stake authority",
                    &mut wallet_manager,
                )?,
                withdraw_authority: pubkey_from_path(
                    &matches,
                    &args.withdraw_authority,
                    "withdraw authority",
                    &mut wallet_manager,
                )?,
                lamports: args.lamports,
                index: args.index,
            };
            Ok(Command::New(resolved_args))
        }
        Command::Count(args) => {
            let resolved_args = CountArgs {
                base_pubkey: resolve_base_pubkey(&mut wallet_manager, &args.base_pubkey)?,
            };
            Ok(Command::Count(resolved_args))
        }
        Command::Addresses(args) => {
            let resolved_args = QueryArgs {
                base_pubkey: resolve_base_pubkey(&mut wallet_manager, &args.base_pubkey)?,
                num_accounts: args.num_accounts,
            };
            Ok(Command::Addresses(resolved_args))
        }
        Command::Balance(args) => {
            let resolved_args = QueryArgs {
                base_pubkey: resolve_base_pubkey(&mut wallet_manager, &args.base_pubkey)?,
                num_accounts: args.num_accounts,
            };
            Ok(Command::Balance(resolved_args))
        }
        Command::Authorize(args) => {
            let resolved_args = resolve_authorize_args(&mut wallet_manager, &args)?;
            Ok(Command::Authorize(resolved_args))
        }
        Command::SetLockup(args) => {
            let resolved_args = resolve_set_lockup_args(&mut wallet_manager, &args)?;
            Ok(Command::SetLockup(resolved_args))
        }
        Command::Rebase(args) => {
            let resolved_args = resolve_rebase_args(&mut wallet_manager, &args)?;
            Ok(Command::Rebase(resolved_args))
        }
        Command::Move(args) => {
            let resolved_args = MoveArgs {
                authorize_args: resolve_authorize_args(&mut wallet_manager, &args.authorize_args)?,
                rebase_args: resolve_rebase_args(&mut wallet_manager, &args.rebase_args)?,
            };
            Ok(Command::Move(Box::new(resolved_args)))
        }
    }
}
