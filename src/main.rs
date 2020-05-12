use solana_cli_config::Config;
use solana_cli_config::CONFIG_FILE;
use solana_client::rpc_client::RpcClient;
use solana_tokens::{
    arg_parser::parse_args,
    args::{resolve_command, Command},
    commands,
    thin_client::ThinClient,
};
use std::env;
use std::error::Error;
use std::path::Path;
use std::process;

fn main() -> Result<(), Box<dyn Error>> {
    let command_args = parse_args(env::args_os());
    let config = if Path::new(&command_args.config_file).exists() {
        Config::load(&command_args.config_file)?
    } else {
        let default_config_file = CONFIG_FILE.as_ref().unwrap();
        if command_args.config_file != *default_config_file {
            eprintln!("Error: config file not found");
            process::exit(1);
        }
        Config::default()
    };
    let json_rpc_url = command_args.url.unwrap_or(config.json_rpc_url);
    let client = RpcClient::new(json_rpc_url);

    match resolve_command(command_args.command)? {
        Command::DistributeTokens(args) => {
            let thin_client = ThinClient::new(client, args.dry_run);
            commands::process_distribute_tokens(&thin_client, &args)?;
        }
        Command::Balances(args) => {
            let thin_client = ThinClient::new(client, false);
            commands::process_balances(&thin_client, &args)?;
        }
        Command::TransactionLog(args) => {
            commands::process_transaction_log(&args)?;
        }
    }
    Ok(())
}
