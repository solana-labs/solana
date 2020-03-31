use clap::ArgMatches;
use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::error::Error;
use std::sync::Arc;

pub(crate) struct NewCommandConfig<P, K> {
    pub fee_payer: K,
    pub funding_keypair: K,
    pub base_keypair: K,
    pub lamports: u64,
    pub stake_authority: P,
    pub withdraw_authority: P,
    pub index: usize,
}

pub(crate) struct CountCommandConfig<P> {
    pub base_pubkey: P,
}

pub(crate) struct QueryCommandConfig<P> {
    pub base_pubkey: P,
    pub num_accounts: usize,
}

pub(crate) struct AuthorizeCommandConfig<P, K> {
    pub fee_payer: K,
    pub base_pubkey: P,
    pub stake_authority: K,
    pub withdraw_authority: K,
    pub new_stake_authority: P,
    pub new_withdraw_authority: P,
    pub num_accounts: usize,
}

pub(crate) struct RebaseCommandConfig<P, K> {
    pub fee_payer: K,
    pub base_pubkey: P,
    pub new_base_keypair: K,
    pub stake_authority: K,
    pub num_accounts: usize,
}

pub(crate) struct MoveCommandConfig<P, K> {
    pub rebase_config: RebaseCommandConfig<P, K>,
    pub authorize_config: AuthorizeCommandConfig<P, K>,
}

pub(crate) enum Command<P, K> {
    New(NewCommandConfig<P, K>),
    Count(CountCommandConfig<P>),
    Addresses(QueryCommandConfig<P>),
    Balance(QueryCommandConfig<P>),
    Authorize(AuthorizeCommandConfig<P, K>),
    Rebase(RebaseCommandConfig<P, K>),
    Move(Box<MoveCommandConfig<P, K>>),
}

pub(crate) struct CommandConfig<P, K> {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command<P, K>,
}

fn resolve_stake_authority(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "stake authority", wallet_manager)
}

fn resolve_withdraw_authority(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "withdraw authority", wallet_manager)
}

fn resolve_new_stake_authority(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Pubkey, Box<dyn Error>> {
    let matches = ArgMatches::default();
    pubkey_from_path(&matches, key_url, "new stake authority", wallet_manager)
}

fn resolve_new_withdraw_authority(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Pubkey, Box<dyn Error>> {
    let matches = ArgMatches::default();
    pubkey_from_path(&matches, key_url, "new withdraw authority", wallet_manager)
}

fn resolve_fee_payer(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "fee-payer", wallet_manager)
}

fn resolve_base_pubkey(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Pubkey, Box<dyn Error>> {
    let matches = ArgMatches::default();
    pubkey_from_path(&matches, key_url, "base pubkey", wallet_manager)
}

fn resolve_new_base_keypair(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    key_url: &str,
) -> Result<Box<dyn Signer>, Box<dyn Error>> {
    let matches = ArgMatches::default();
    signer_from_path(&matches, key_url, "new base pubkey", wallet_manager)
}

fn resolve_authorize_config(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    config: &AuthorizeCommandConfig<String, String>,
) -> Result<AuthorizeCommandConfig<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let resolved_config = AuthorizeCommandConfig {
        fee_payer: resolve_fee_payer(wallet_manager, &config.fee_payer)?,
        base_pubkey: resolve_base_pubkey(wallet_manager, &config.base_pubkey)?,
        stake_authority: resolve_stake_authority(wallet_manager, &config.stake_authority)?,
        withdraw_authority: resolve_withdraw_authority(wallet_manager, &config.withdraw_authority)?,
        new_stake_authority: resolve_new_stake_authority(
            wallet_manager,
            &config.new_stake_authority,
        )?,
        new_withdraw_authority: resolve_new_withdraw_authority(
            wallet_manager,
            &config.new_withdraw_authority,
        )?,
        num_accounts: config.num_accounts,
    };
    Ok(resolved_config)
}

fn resolve_rebase_config(
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    config: &RebaseCommandConfig<String, String>,
) -> Result<RebaseCommandConfig<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let resolved_config = RebaseCommandConfig {
        fee_payer: resolve_fee_payer(wallet_manager, &config.fee_payer)?,
        base_pubkey: resolve_base_pubkey(wallet_manager, &config.base_pubkey)?,
        new_base_keypair: resolve_new_base_keypair(wallet_manager, &config.new_base_keypair)?,
        stake_authority: resolve_stake_authority(wallet_manager, &config.stake_authority)?,
        num_accounts: config.num_accounts,
    };
    Ok(resolved_config)
}

pub(crate) fn resolve_command(
    command: &Command<String, String>,
) -> Result<Command<Pubkey, Box<dyn Signer>>, Box<dyn Error>> {
    let wallet_manager = maybe_wallet_manager()?;
    let wallet_manager = wallet_manager.as_ref();
    let matches = ArgMatches::default();
    match command {
        Command::New(config) => {
            let resolved_config = NewCommandConfig {
                fee_payer: resolve_fee_payer(wallet_manager, &config.fee_payer)?,
                funding_keypair: signer_from_path(
                    &matches,
                    &config.funding_keypair,
                    "funding keypair",
                    wallet_manager,
                )?,
                base_keypair: signer_from_path(
                    &matches,
                    &config.base_keypair,
                    "base keypair",
                    wallet_manager,
                )?,
                stake_authority: pubkey_from_path(
                    &matches,
                    &config.stake_authority,
                    "stake authority",
                    wallet_manager,
                )?,
                withdraw_authority: pubkey_from_path(
                    &matches,
                    &config.withdraw_authority,
                    "withdraw authority",
                    wallet_manager,
                )?,
                lamports: config.lamports,
                index: config.index,
            };
            Ok(Command::New(resolved_config))
        }
        Command::Count(config) => {
            let resolved_config = CountCommandConfig {
                base_pubkey: resolve_base_pubkey(wallet_manager, &config.base_pubkey)?,
            };
            Ok(Command::Count(resolved_config))
        }
        Command::Addresses(config) => {
            let resolved_config = QueryCommandConfig {
                base_pubkey: resolve_base_pubkey(wallet_manager, &config.base_pubkey)?,
                num_accounts: config.num_accounts,
            };
            Ok(Command::Addresses(resolved_config))
        }
        Command::Balance(config) => {
            let resolved_config = QueryCommandConfig {
                base_pubkey: resolve_base_pubkey(wallet_manager, &config.base_pubkey)?,
                num_accounts: config.num_accounts,
            };
            Ok(Command::Balance(resolved_config))
        }
        Command::Authorize(config) => {
            let resolved_config = resolve_authorize_config(wallet_manager, &config)?;
            Ok(Command::Authorize(resolved_config))
        }
        Command::Rebase(config) => {
            let resolved_config = resolve_rebase_config(wallet_manager, &config)?;
            Ok(Command::Rebase(resolved_config))
        }
        Command::Move(config) => {
            let resolved_config = MoveCommandConfig {
                authorize_config: resolve_authorize_config(
                    wallet_manager,
                    &config.authorize_config,
                )?,
                rebase_config: resolve_rebase_config(wallet_manager, &config.rebase_config)?,
            };
            Ok(Command::Move(Box::new(resolved_config)))
        }
    }
}
