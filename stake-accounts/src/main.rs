mod arg_parser;
mod args;
mod stake_accounts;

use crate::arg_parser::parse_args;
use crate::args::{
    resolve_command, AuthorizeArgs, Command, MoveArgs, NewArgs, RebaseArgs, SetLockupArgs,
};
use itertools::Itertools;
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
use solana_stake_program::{
    stake_instruction::LockupArgs,
    stake_state::{Lockup, StakeState},
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

fn get_lockup(client: &RpcClient, address: &Pubkey) -> Result<Lockup, ClientError> {
    client
        .get_account(address)
        .map(|account| StakeState::lockup_from(&account).unwrap())
}

fn get_lockups(
    client: &RpcClient,
    addresses: Vec<Pubkey>,
) -> Result<Vec<(Pubkey, Lockup)>, ClientError> {
    addresses
        .into_iter()
        .map(|pubkey| get_lockup(client, &pubkey).map(|bal| (pubkey, bal)))
        .collect()
}

fn unique_signers(signers: Vec<&dyn Signer>) -> Vec<&dyn Signer> {
    signers.into_iter().unique_by(|s| s.pubkey()).collect_vec()
}

fn process_new_stake_account(
    client: &RpcClient,
    args: &NewArgs<Pubkey, Box<dyn Signer>>,
) -> Result<Signature, ClientError> {
    let message = stake_accounts::new_stake_account(
        &args.fee_payer.pubkey(),
        &args.funding_keypair.pubkey(),
        &args.base_keypair.pubkey(),
        args.lamports,
        &args.stake_authority,
        &args.withdraw_authority,
        &Pubkey::default(),
        args.index,
    );
    let signers = unique_signers(vec![
        &*args.fee_payer,
        &*args.funding_keypair,
        &*args.base_keypair,
    ]);
    let signature = send_message(client, message, &signers, false)?;
    Ok(signature)
}

fn process_authorize_stake_accounts(
    client: &RpcClient,
    args: &AuthorizeArgs<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let messages = stake_accounts::authorize_stake_accounts(
        &args.fee_payer.pubkey(),
        &args.base_pubkey,
        &args.stake_authority.pubkey(),
        &args.withdraw_authority.pubkey(),
        &args.new_stake_authority,
        &args.new_withdraw_authority,
        args.num_accounts,
    );
    let signers = unique_signers(vec![
        &*args.fee_payer,
        &*args.stake_authority,
        &*args.withdraw_authority,
    ]);
    send_messages(client, messages, &signers, false)?;
    Ok(())
}

fn process_lockup_stake_accounts(
    client: &RpcClient,
    args: &SetLockupArgs<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let addresses =
        stake_accounts::derive_stake_account_addresses(&args.base_pubkey, args.num_accounts);
    let existing_lockups = get_lockups(&client, addresses)?;

    let lockup = LockupArgs {
        epoch: args.lockup_epoch,
        unix_timestamp: args.lockup_date,
        custodian: args.new_custodian,
    };
    let messages = stake_accounts::lockup_stake_accounts(
        &args.fee_payer.pubkey(),
        &args.custodian.pubkey(),
        &lockup,
        &existing_lockups,
        args.unlock_years,
    );
    if messages.is_empty() {
        eprintln!("No work to do");
        return Ok(());
    }
    let signers = unique_signers(vec![&*args.fee_payer, &*args.custodian]);
    send_messages(client, messages, &signers, args.no_wait)?;
    Ok(())
}

fn process_rebase_stake_accounts(
    client: &RpcClient,
    args: &RebaseArgs<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let addresses =
        stake_accounts::derive_stake_account_addresses(&args.base_pubkey, args.num_accounts);
    let balances = get_balances(&client, addresses)?;

    let messages = stake_accounts::rebase_stake_accounts(
        &args.fee_payer.pubkey(),
        &args.new_base_keypair.pubkey(),
        &args.stake_authority.pubkey(),
        &balances,
    );
    if messages.is_empty() {
        eprintln!("No accounts found");
        return Ok(());
    }
    let signers = unique_signers(vec![
        &*args.fee_payer,
        &*args.new_base_keypair,
        &*args.stake_authority,
    ]);
    send_messages(client, messages, &signers, false)?;
    Ok(())
}

fn process_move_stake_accounts(
    client: &RpcClient,
    move_args: &MoveArgs<Pubkey, Box<dyn Signer>>,
) -> Result<(), ClientError> {
    let authorize_args = &move_args.authorize_args;
    let args = &move_args.rebase_args;
    let addresses =
        stake_accounts::derive_stake_account_addresses(&args.base_pubkey, args.num_accounts);
    let balances = get_balances(&client, addresses)?;

    let messages = stake_accounts::move_stake_accounts(
        &args.fee_payer.pubkey(),
        &args.new_base_keypair.pubkey(),
        &args.stake_authority.pubkey(),
        &authorize_args.withdraw_authority.pubkey(),
        &authorize_args.new_stake_authority,
        &authorize_args.new_withdraw_authority,
        &balances,
    );
    if messages.is_empty() {
        eprintln!("No accounts found");
        return Ok(());
    }
    let signers = unique_signers(vec![
        &*args.fee_payer,
        &*args.new_base_keypair,
        &*args.stake_authority,
        &*authorize_args.withdraw_authority,
    ]);
    send_messages(client, messages, &signers, false)?;
    Ok(())
}

fn send_message<S: Signers>(
    client: &RpcClient,
    message: Message,
    signers: &S,
    no_wait: bool,
) -> Result<Signature, ClientError> {
    let mut transaction = Transaction::new_unsigned(message);
    client.resign_transaction(&mut transaction, signers)?;
    if no_wait {
        client.send_transaction(&transaction)
    } else {
        client.send_and_confirm_transaction_with_spinner(&transaction)
    }
}

fn send_messages<S: Signers>(
    client: &RpcClient,
    messages: Vec<Message>,
    signers: &S,
    no_wait: bool,
) -> Result<Vec<Signature>, ClientError> {
    let mut signatures = vec![];
    for message in messages {
        let signature = send_message(client, message, signers, no_wait)?;
        signatures.push(signature);
        println!("{}", signature);
    }
    Ok(signatures)
}

fn main() -> Result<(), Box<dyn Error>> {
    let command_args = parse_args(env::args_os());
    let config = Config::load(&command_args.config_file)?;
    let json_rpc_url = command_args.url.unwrap_or(config.json_rpc_url);
    let client = RpcClient::new(json_rpc_url);

    match resolve_command(&command_args.command)? {
        Command::New(args) => {
            process_new_stake_account(&client, &args)?;
        }
        Command::Count(args) => {
            let num_accounts = count_stake_accounts(&client, &args.base_pubkey)?;
            println!("{}", num_accounts);
        }
        Command::Addresses(args) => {
            let addresses = stake_accounts::derive_stake_account_addresses(
                &args.base_pubkey,
                args.num_accounts,
            );
            for address in addresses {
                println!("{:?}", address);
            }
        }
        Command::Balance(args) => {
            let addresses = stake_accounts::derive_stake_account_addresses(
                &args.base_pubkey,
                args.num_accounts,
            );
            let balances = get_balances(&client, addresses)?;
            let lamports: u64 = balances.into_iter().map(|(_, bal)| bal).sum();
            let sol = lamports_to_sol(lamports);
            println!("{} SOL", sol);
        }
        Command::Authorize(args) => {
            process_authorize_stake_accounts(&client, &args)?;
        }
        Command::SetLockup(args) => {
            process_lockup_stake_accounts(&client, &args)?;
        }
        Command::Rebase(args) => {
            process_rebase_stake_accounts(&client, &args)?;
        }
        Command::Move(args) => {
            process_move_stake_accounts(&client, &args)?;
        }
    }
    Ok(())
}
