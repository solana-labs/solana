use {
    clap::{crate_description, crate_name, crate_version, ArgEnum, Args, Parser},
    serde::{Deserialize, Serialize},
    std::{net::SocketAddr, process::exit},
};

#[derive(Parser)]
#[clap(name = crate_name!(),
    version = crate_version!(),
    about = crate_description!(),
    rename_all = "kebab-case"
)]
pub struct DosClientParameters {
    #[clap(long, arg_enum, help = "Interface to DoS")]
    pub mode: Mode,

    #[clap(long, arg_enum, help = "Type of data to send")]
    pub data_type: DataType,

    #[clap(
        long = "entrypoint",
        parse(try_from_str = addr_parser),
        default_value = "127.0.0.1:8001",
        help = "Gossip entrypoint address. Usually <ip>:8001"
    )]
    pub entrypoint_addr: SocketAddr,

    #[clap(
        long,
        default_value = "128",
        required_if_eq("data-type", "random"),
        help = "Size of packet to DoS with, relevant only for data-type=random"
    )]
    pub data_size: usize,

    #[clap(long, help = "Data to send [Optional]")]
    pub data_input: Option<String>,

    #[clap(long, help = "Just use entrypoint address directly")]
    pub skip_gossip: bool,

    #[clap(long, help = "Allow contacting private ip addresses")]
    pub allow_private_addr: bool,

    #[clap(flatten)]
    pub transaction_params: TransactionParams,
}

#[derive(Args, Serialize, Deserialize, Debug, Default)]
#[clap(rename_all = "kebab-case")]
pub struct TransactionParams {
    #[clap(
        long,
        default_value = "2",
        help = "Number of signatures in transaction"
    )]
    pub num_signatures: usize,

    #[clap(long, help = "Generate a valid blockhash for transaction")]
    pub valid_blockhash: bool,

    #[clap(long, help = "Generate valid signature(s) for transaction")]
    pub valid_signatures: bool,

    #[clap(long, help = "Generate unique transactions")]
    pub unique_transactions: bool,

    #[clap(
        long = "payer",
        help = "Payer's keypair file to fund transactions [Optional]"
    )]
    pub payer_filename: Option<String>,
}

#[derive(ArgEnum, Clone, Eq, PartialEq)]
pub enum Mode {
    Gossip,
    Tvu,
    TvuForwards,
    Tpu,
    TpuForwards,
    Repair,
    ServeRepair,
    Rpc,
}

#[derive(ArgEnum, Clone, Eq, PartialEq)]
pub enum DataType {
    RepairHighest,
    RepairShred,
    RepairOrphan,
    Random,
    GetAccountInfo,
    GetProgramAccounts,
    Transaction,
}

fn addr_parser(addr: &str) -> Result<SocketAddr, &'static str> {
    match solana_net_utils::parse_host_port(addr) {
        Ok(v) => Ok(v),
        Err(_) => Err("failed to parse entrypoint address"),
    }
}

/// input checks which are not covered by Clap
fn validate_input(params: &DosClientParameters) {
    if params.mode == Mode::Rpc
        && (params.data_type != DataType::GetAccountInfo
            && params.data_type != DataType::GetProgramAccounts)
    {
        eprintln!("unsupported data type");
        exit(1);
    }

    if params.data_type != DataType::Transaction {
        let tp = &params.transaction_params;
        if tp.valid_blockhash
            || tp.valid_signatures
            || tp.unique_transactions
            || tp.payer_filename.is_some()
        {
            eprintln!("Arguments valid-blockhash, valid-sign, unique-trans, payer are ignored if data-type != transaction");
            exit(1);
        }
    }

    if params.transaction_params.payer_filename.is_some()
        && params.transaction_params.valid_signatures
    {
        eprintln!("Arguments valid-signatures is ignored if payer is provided");
        exit(1);
    }
}

pub fn build_cli_parameters() -> DosClientParameters {
    let cmd_params = DosClientParameters::parse();
    validate_input(&cmd_params);
    cmd_params
}
