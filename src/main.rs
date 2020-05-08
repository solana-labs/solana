use solana_cli_config::Config;
use solana_client::rpc_client::RpcClient;
use solana_tokens::{
    arg_parser::parse_args,
    args::{resolve_command, Command},
    thin_client::ThinClient,
    tokens,
};
use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let command_args = parse_args(env::args_os());
    let config = Config::load(&command_args.config_file)?;
    let json_rpc_url = command_args.url.unwrap_or(config.json_rpc_url);
    let client = RpcClient::new(json_rpc_url);
    let thin_client = ThinClient::new(client);

    match resolve_command(command_args.command)? {
        Command::DistributeTokens(args) => {
            tokens::process_distribute_tokens(&thin_client, &args)?;
        }
        Command::Balances(args) => {
            tokens::process_balances(&thin_client, &args)?;
        }
        Command::TransactionLog(args) => {
            tokens::process_transaction_log(&args)?;
        }
    }
    Ok(())
}
