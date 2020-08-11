use solana_banks_client::start_tcp_client;
use solana_cli_config::{Config, CONFIG_FILE};
use solana_tokens::{arg_parser::parse_args, args::Command, commands};
use std::{env, error::Error, path::Path, process};
use tokio::runtime::Runtime;
use url::Url;

fn main() -> Result<(), Box<dyn Error>> {
    let command_args = parse_args(env::args_os())?;
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
    let rpc_banks_url = Config::compute_rpc_banks_url(&json_rpc_url);
    let url = Url::parse(&rpc_banks_url)?;
    let host_port = (url.host_str().unwrap(), url.port().unwrap());

    let mut runtime = Runtime::new().unwrap();
    let mut banks_client = runtime.block_on(start_tcp_client(&host_port))?;

    match command_args.command {
        Command::DistributeTokens(args) => {
            runtime.block_on(commands::process_distribute_tokens(
                &mut banks_client,
                &args,
            ))?;
        }
        Command::Balances(args) => {
            runtime.block_on(commands::process_balances(&mut banks_client, &args))?;
        }
        Command::TransactionLog(args) => {
            commands::process_transaction_log(&args)?;
        }
    }
    Ok(())
}
