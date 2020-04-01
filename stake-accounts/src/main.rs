mod args;
mod stake_accounts;

use crate::args::{
    parse_args, AuthorizeCommandConfig, Command, MoveCommandConfig, NewCommandConfig,
    RebaseCommandConfig,
};
use clap::ArgMatches;
use solana_clap_utils::keypair::{pubkey_from_path, signer_from_path};
use solana_cli_config::Config;
use solana_client::client_error::ClientError;
use solana_client::rpc_client::RpcClient;
use solana_remote_wallet::remote_wallet::{maybe_wallet_manager, RemoteWalletManager};
use solana_sdk::{
    message::Message,
    native_token::lamports_to_sol,
    pubkey::Pubkey,
    signature::{Signature, Signer},
    signers::Signers,
    transaction::Transaction,
};
use std::env;
use std::error::Error;
use std::sync::Arc;

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

fn get_balance_at(client: &RpcClient, pubkey: &Pubkey, i: usize) -> Result<u64, ClientError> {
    let address = stake_accounts::derive_stake_account_address(pubkey, i);
    client.get_balance(&address)
}

// Return the number of derived stake accounts with balances
fn count_stake_accounts(client: &RpcClient, base_pubkey: &Pubkey) -> Result<usize, ClientError> {
    let mut i = 0;
    while get_balance_at(client, base_pubkey, i)? > 0 {
        i += 1;
    }
    Ok(i)
}

fn get_balances(
    client: &RpcClient,
    addresses: Vec<Pubkey>,
) -> Result<Vec<(Pubkey, u64)>, ClientError> {
    addresses
        .into_iter()
        .map(|pubkey| client.get_balance(&pubkey).map(|bal| (pubkey, bal)))
        .collect()
}

fn process_new_stake_account(
    client: &RpcClient,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    new_config: &NewCommandConfig,
) -> Result<Signature, Box<dyn Error>> {
    let matches = ArgMatches::default();
    let fee_payer_keypair = resolve_fee_payer(wallet_manager, &new_config.fee_payer)?;
    let funding_keypair = signer_from_path(
        &matches,
        &new_config.funding_keypair,
        "funding keypair",
        wallet_manager,
    )?;
    let base_keypair = signer_from_path(
        &matches,
        &new_config.base_keypair,
        "base keypair",
        wallet_manager,
    )?;
    let stake_authority_pubkey = pubkey_from_path(
        &matches,
        &new_config.stake_authority,
        "stake authority",
        wallet_manager,
    )?;
    let withdraw_authority_pubkey = pubkey_from_path(
        &matches,
        &new_config.withdraw_authority,
        "withdraw authority",
        wallet_manager,
    )?;
    let message = stake_accounts::new_stake_account(
        &fee_payer_keypair.pubkey(),
        &funding_keypair.pubkey(),
        &base_keypair.pubkey(),
        new_config.lamports,
        &stake_authority_pubkey,
        &withdraw_authority_pubkey,
        new_config.index,
    );
    let signers = vec![&*fee_payer_keypair, &*funding_keypair, &*base_keypair];
    let signature = send_message(client, message, &signers)?;
    Ok(signature)
}

fn process_authorize_stake_accounts(
    client: &RpcClient,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    authorize_config: &AuthorizeCommandConfig,
) -> Result<(), Box<dyn Error>> {
    let fee_payer_keypair = resolve_fee_payer(wallet_manager, &authorize_config.fee_payer)?;
    let base_pubkey = resolve_base_pubkey(wallet_manager, &authorize_config.base_pubkey)?;
    let stake_authority_keypair =
        resolve_stake_authority(wallet_manager, &authorize_config.stake_authority)?;
    let withdraw_authority_keypair =
        resolve_withdraw_authority(wallet_manager, &authorize_config.withdraw_authority)?;
    let new_stake_authority_pubkey =
        resolve_new_stake_authority(wallet_manager, &authorize_config.new_stake_authority)?;
    let new_withdraw_authority_pubkey =
        resolve_new_withdraw_authority(wallet_manager, &authorize_config.new_withdraw_authority)?;
    let messages = stake_accounts::authorize_stake_accounts(
        &fee_payer_keypair.pubkey(),
        &base_pubkey,
        &stake_authority_keypair.pubkey(),
        &withdraw_authority_keypair.pubkey(),
        &new_stake_authority_pubkey,
        &new_withdraw_authority_pubkey,
        authorize_config.num_accounts,
    );
    let signers = vec![
        &*fee_payer_keypair,
        &*stake_authority_keypair,
        &*withdraw_authority_keypair,
    ];
    for message in messages {
        let signature = send_message(client, message, &signers)?;
        println!("{}", signature);
    }
    Ok(())
}

fn process_rebase_stake_accounts(
    client: &RpcClient,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    rebase_config: &RebaseCommandConfig,
) -> Result<(), Box<dyn Error>> {
    let fee_payer_keypair = resolve_fee_payer(wallet_manager, &rebase_config.fee_payer)?;
    let base_pubkey = resolve_base_pubkey(wallet_manager, &rebase_config.base_pubkey)?;
    let new_base_keypair =
        resolve_new_base_keypair(wallet_manager, &rebase_config.new_base_keypair)?;
    let stake_authority_keypair =
        resolve_stake_authority(wallet_manager, &rebase_config.stake_authority)?;
    let addresses =
        stake_accounts::derive_stake_account_addresses(&base_pubkey, rebase_config.num_accounts);
    let balances = get_balances(&client, addresses)?;

    let messages = stake_accounts::rebase_stake_accounts(
        &fee_payer_keypair.pubkey(),
        &new_base_keypair.pubkey(),
        &stake_authority_keypair.pubkey(),
        &balances,
    );
    let signers = vec![
        &*fee_payer_keypair,
        &*new_base_keypair,
        &*stake_authority_keypair,
    ];
    for message in messages {
        let signature = send_message(client, message, &signers)?;
        println!("{}", signature);
    }
    Ok(())
}

fn process_move_stake_accounts(
    client: &RpcClient,
    wallet_manager: Option<&Arc<RemoteWalletManager>>,
    move_config: &MoveCommandConfig,
) -> Result<(), Box<dyn Error>> {
    let authorize_config = &move_config.authorize_config;
    let rebase_config = &move_config.rebase_config;
    let fee_payer_keypair = resolve_fee_payer(wallet_manager, &authorize_config.fee_payer)?;
    let base_pubkey = resolve_base_pubkey(wallet_manager, &authorize_config.base_pubkey)?;
    let new_base_keypair =
        resolve_new_base_keypair(wallet_manager, &rebase_config.new_base_keypair)?;
    let stake_authority_keypair =
        resolve_stake_authority(wallet_manager, &authorize_config.stake_authority)?;
    let withdraw_authority_keypair =
        resolve_withdraw_authority(wallet_manager, &authorize_config.withdraw_authority)?;
    let new_stake_authority_pubkey =
        resolve_new_stake_authority(wallet_manager, &authorize_config.new_stake_authority)?;
    let new_withdraw_authority_pubkey =
        resolve_new_withdraw_authority(wallet_manager, &authorize_config.new_withdraw_authority)?;
    let addresses =
        stake_accounts::derive_stake_account_addresses(&base_pubkey, authorize_config.num_accounts);
    let balances = get_balances(&client, addresses)?;

    let messages = stake_accounts::move_stake_accounts(
        &fee_payer_keypair.pubkey(),
        &new_base_keypair.pubkey(),
        &stake_authority_keypair.pubkey(),
        &withdraw_authority_keypair.pubkey(),
        &new_stake_authority_pubkey,
        &new_withdraw_authority_pubkey,
        &balances,
    );
    let signers = vec![
        &*fee_payer_keypair,
        &*new_base_keypair,
        &*stake_authority_keypair,
        &*withdraw_authority_keypair,
    ];
    for message in messages {
        let signature = send_message(client, message, &signers)?;
        println!("{}", signature);
    }
    Ok(())
}

fn send_message<S: Signers>(
    client: &RpcClient,
    message: Message,
    signers: &S,
) -> Result<Signature, ClientError> {
    let mut transaction = Transaction::new_unsigned(message);
    client.resign_transaction(&mut transaction, signers)?;
    client.send_and_confirm_transaction_with_spinner(&mut transaction, signers)
}

fn main() -> Result<(), Box<dyn Error>> {
    let command_config = parse_args(env::args_os());
    let config = Config::load(&command_config.config_file)?;
    let json_rpc_url = command_config.url.unwrap_or(config.json_rpc_url);
    let client = RpcClient::new(json_rpc_url);

    let wallet_manager = maybe_wallet_manager()?;
    let wallet_manager = wallet_manager.as_ref();
    match command_config.command {
        Command::New(new_config) => {
            process_new_stake_account(&client, wallet_manager, &new_config)?;
        }
        Command::Count(count_config) => {
            let base_pubkey = resolve_base_pubkey(wallet_manager, &count_config.base_pubkey)?;
            let num_accounts = count_stake_accounts(&client, &base_pubkey)?;
            println!("{}", num_accounts);
        }
        Command::Addresses(query_config) => {
            let base_pubkey = resolve_base_pubkey(wallet_manager, &query_config.base_pubkey)?;
            let addresses = stake_accounts::derive_stake_account_addresses(
                &base_pubkey,
                query_config.num_accounts,
            );
            for address in addresses {
                println!("{:?}", address);
            }
        }
        Command::Balance(query_config) => {
            let base_pubkey = resolve_base_pubkey(wallet_manager, &query_config.base_pubkey)?;
            let addresses = stake_accounts::derive_stake_account_addresses(
                &base_pubkey,
                query_config.num_accounts,
            );
            let balances = get_balances(&client, addresses)?;
            let lamports: u64 = balances.into_iter().map(|(_, bal)| bal).sum();
            let sol = lamports_to_sol(lamports);
            println!("{} SOL", sol);
        }
        Command::Authorize(authorize_config) => {
            process_authorize_stake_accounts(&client, wallet_manager, &authorize_config)?;
        }
        Command::Rebase(rebase_config) => {
            process_rebase_stake_accounts(&client, wallet_manager, &rebase_config)?;
        }
        Command::Move(move_config) => {
            process_move_stake_accounts(&client, wallet_manager, &move_config)?;
        }
    }
    Ok(())
}
