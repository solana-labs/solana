mod arg_parser;
mod args;
mod stake_accounts;

use crate::arg_parser::parse_args;
use crate::args::{
    resolve_command, AuthorizeCommandConfig, Command, MoveCommandConfig, NewCommandConfig,
    RebaseCommandConfig,
};
use solana_cli_config::Config;
use solana_client::client_error::ClientError;
use solana_client::rpc_client::RpcClient;
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
    config: &NewCommandConfig<Pubkey, Box<dyn Signer>>,
) -> Result<Signature, ClientError> {
    let message = stake_accounts::new_stake_account(
        &config.fee_payer.pubkey(),
        &config.funding_keypair.pubkey(),
        &config.base_keypair.pubkey(),
        config.lamports,
        &config.stake_authority,
        &config.withdraw_authority,
        config.index,
    );
    let signers = vec![
        &*config.fee_payer,
        &*config.funding_keypair,
        &*config.base_keypair,
    ];
    let signature = send_message(client, message, &signers)?;
    Ok(signature)
}

fn process_authorize_stake_accounts(
    client: &RpcClient,
    config: &AuthorizeCommandConfig<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let messages = stake_accounts::authorize_stake_accounts(
        &config.fee_payer.pubkey(),
        &config.base_pubkey,
        &config.stake_authority.pubkey(),
        &config.withdraw_authority.pubkey(),
        &config.new_stake_authority,
        &config.new_withdraw_authority,
        config.num_accounts,
    );
    let signers = vec![
        &*config.fee_payer,
        &*config.stake_authority,
        &*config.withdraw_authority,
    ];
    for message in messages {
        let signature = send_message(client, message, &signers)?;
        println!("{}", signature);
    }
    Ok(())
}

fn process_rebase_stake_accounts(
    client: &RpcClient,
    config: &RebaseCommandConfig<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let addresses =
        stake_accounts::derive_stake_account_addresses(&config.base_pubkey, config.num_accounts);
    let balances = get_balances(&client, addresses)?;

    let messages = stake_accounts::rebase_stake_accounts(
        &config.fee_payer.pubkey(),
        &config.new_base_keypair.pubkey(),
        &config.stake_authority.pubkey(),
        &balances,
    );
    let signers = vec![
        &*config.fee_payer,
        &*config.new_base_keypair,
        &*config.stake_authority,
    ];
    for message in messages {
        let signature = send_message(client, message, &signers)?;
        println!("{}", signature);
    }
    Ok(())
}

fn process_move_stake_accounts(
    client: &RpcClient,
    move_config: &MoveCommandConfig<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let authorize_config = &move_config.authorize_config;
    let config = &move_config.rebase_config;
    let addresses =
        stake_accounts::derive_stake_account_addresses(&config.base_pubkey, config.num_accounts);
    let balances = get_balances(&client, addresses)?;

    let messages = stake_accounts::move_stake_accounts(
        &config.fee_payer.pubkey(),
        &config.new_base_keypair.pubkey(),
        &config.stake_authority.pubkey(),
        &authorize_config.withdraw_authority.pubkey(),
        &authorize_config.new_stake_authority,
        &authorize_config.new_withdraw_authority,
        &balances,
    );
    let signers = vec![
        &*config.fee_payer,
        &*config.new_base_keypair,
        &*config.stake_authority,
        &*authorize_config.withdraw_authority,
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
    let app_config = Config::load(&command_config.config_file)?;
    let json_rpc_url = command_config.url.unwrap_or(app_config.json_rpc_url);
    let client = RpcClient::new(json_rpc_url);

    match resolve_command(&command_config.command)? {
        Command::New(config) => {
            process_new_stake_account(&client, &config)?;
        }
        Command::Count(config) => {
            let num_accounts = count_stake_accounts(&client, &config.base_pubkey)?;
            println!("{}", num_accounts);
        }
        Command::Addresses(config) => {
            let addresses = stake_accounts::derive_stake_account_addresses(
                &config.base_pubkey,
                config.num_accounts,
            );
            for address in addresses {
                println!("{:?}", address);
            }
        }
        Command::Balance(config) => {
            let addresses = stake_accounts::derive_stake_account_addresses(
                &config.base_pubkey,
                config.num_accounts,
            );
            let balances = get_balances(&client, addresses)?;
            let lamports: u64 = balances.into_iter().map(|(_, bal)| bal).sum();
            let sol = lamports_to_sol(lamports);
            println!("{} SOL", sol);
        }
        Command::Authorize(config) => {
            process_authorize_stake_accounts(&client, &config)?;
        }
        Command::Rebase(config) => {
            process_rebase_stake_accounts(&client, &config)?;
        }
        Command::Move(config) => {
            process_move_stake_accounts(&client, &config)?;
        }
    }
    Ok(())
}
