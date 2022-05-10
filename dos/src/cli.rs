use {
    clap::{crate_description, crate_name, crate_version, ArgEnum, Args, Parser},
    serde::{Deserialize, Serialize},
    std::{net::SocketAddr, process::exit},
};

#[derive(Parser, Debug, PartialEq)]
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

    #[clap(long, required_if_eq("mode", "rpc"), help = "Data to send [Optional]")]
    pub data_input: Option<String>,

    #[clap(long, help = "Just use entrypoint address directly")]
    pub skip_gossip: bool,

    #[clap(long, help = "Allow contacting private ip addresses")]
    pub allow_private_addr: bool,

    #[clap(flatten)]
    pub transaction_params: TransactionParams,
}

#[derive(Args, Serialize, Deserialize, Debug, Default, PartialEq)]
#[clap(rename_all = "kebab-case")]
pub struct TransactionParams {
    #[clap(
        long,
        conflicts_with("valid-blockhash"),
        help = "Number of signatures in transaction"
    )]
    pub num_signatures: Option<usize>,

    #[clap(
        long,
        requires("transaction-type"),
        help = "Generate a valid blockhash for transaction"
    )]
    pub valid_blockhash: bool,

    #[clap(
        long,
        requires("num-signatures"),
        help = "Generate valid signature(s) for transaction"
    )]
    pub valid_signatures: bool,

    #[clap(long, help = "Generate unique transactions")]
    pub unique_transactions: bool,

    #[clap(
        long,
        arg_enum,
        requires("valid-blockhash"),
        help = "Type of transaction to be sent [Optional]"
    )]
    pub transaction_type: Option<TransactionType>,

    #[clap(
        long,
        required_if_eq("transaction-type", "transfer"),
        help = "Number of instructions in transfer transaction"
    )]
    pub num_instructions: Option<usize>,
}

#[derive(ArgEnum, Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(ArgEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataType {
    RepairHighest,
    RepairShred,
    RepairOrphan,
    Random,
    GetAccountInfo,
    GetProgramAccounts,
    Transaction,
}

#[derive(ArgEnum, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TransactionType {
    Transfer,
    AccountCreation,
}

fn addr_parser(addr: &str) -> Result<SocketAddr, &'static str> {
    match solana_net_utils::parse_host_port(addr) {
        Ok(v) => Ok(v),
        Err(_) => Err("failed to parse address"),
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
        if tp.valid_blockhash || tp.valid_signatures || tp.unique_transactions {
            eprintln!("Arguments valid-blockhash, valid-sign, unique-trans are ignored if data-type != transaction");
            exit(1);
        }
    }
}

pub fn build_cli_parameters() -> DosClientParameters {
    let cmd_params = DosClientParameters::parse();
    validate_input(&cmd_params);
    cmd_params
}

#[cfg(test)]
mod tests {
    use solana_sdk::pubkey::Pubkey;

    use {super::*, clap::Parser};

    #[test]
    fn test_cli_parse_rpc_no_data_input() {
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "rpc",
            "--data-type",
            "get-account-info",
            //--data-input is required for `--mode rpc` but it is not specified
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parse_rpc_data_input() {
        let entrypoint_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let pubkey_str: String = Pubkey::default().to_string();
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "rpc",
            "--data-type",
            "get-account-info",
            "--data-input",
            &pubkey_str,
        ]);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(
            params,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Rpc,
                data_size: 128, // default value
                data_type: DataType::GetAccountInfo,
                data_input: Some(pubkey_str),
                skip_gossip: false,
                allow_private_addr: false,
                transaction_params: TransactionParams::default()
            },
        );
    }

    #[test]
    fn test_cli_parse_dos_valid_signatures() {
        let entrypoint_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "tpu",
            "--data-type",
            "transaction",
            "--unique-transactions",
            "--valid-signatures",
            "--num-signatures",
            "8",
        ]);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(
            params,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Tpu,
                data_size: 128,
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                transaction_params: TransactionParams {
                    num_signatures: Some(8),
                    valid_blockhash: false,
                    valid_signatures: true,
                    unique_transactions: true,
                    transaction_type: None,
                    num_instructions: None,
                },
            },
        );
    }

    #[test]
    fn test_cli_parse_dos_transfer() {
        let entrypoint_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "tpu",
            "--data-type",
            "transaction",
            "--unique-transactions",
            "--valid-blockhash",
            "--transaction-type",
            "transfer",
            "--num-instructions",
            "1",
        ]);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(
            params,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Tpu,
                data_size: 128, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: false,
                    unique_transactions: true,
                    transaction_type: Some(TransactionType::Transfer),
                    num_instructions: Some(1),
                },
            },
        );

        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "tpu",
            "--data-type",
            "transaction",
            "--unique-transactions",
            "--transaction-type",
            "transfer",
            "--num-instructions",
            "8",
        ]);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );

        let entrypoint_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "tpu",
            "--data-type",
            "transaction",
            "--unique-transactions",
            "--valid-blockhash",
            "--transaction-type",
            "transfer",
            "--num-instructions",
            "8",
        ]);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(
            params,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Tpu,
                data_size: 128, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: false,
                    unique_transactions: true,
                    transaction_type: Some(TransactionType::Transfer),
                    num_instructions: Some(8),
                },
            },
        );
    }

    #[test]
    fn test_cli_parse_dos_create_account() {
        let entrypoint_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "tpu",
            "--data-type",
            "transaction",
            "--unique-transactions",
            "--valid-blockhash",
            "--transaction-type",
            "account-creation",
        ]);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(
            params,
            DosClientParameters {
                entrypoint_addr,
                mode: Mode::Tpu,
                data_size: 128, // irrelevant if not random
                data_type: DataType::Transaction,
                data_input: None,
                skip_gossip: false,
                allow_private_addr: false,
                transaction_params: TransactionParams {
                    num_signatures: None,
                    valid_blockhash: true,
                    valid_signatures: false,
                    unique_transactions: true,
                    transaction_type: Some(TransactionType::AccountCreation),
                    num_instructions: None,
                },
            },
        );
    }

    #[test]
    #[should_panic]
    fn test_cli_parse_dos_conflicting_sign_instruction() {
        // check conflicting args num-signatures and num-instructions
        let result = DosClientParameters::try_parse_from(vec![
            "solana-dos",
            "--mode",
            "tpu",
            "--data-type",
            "transaction",
            "--unique-transactions",
            "--valid-signatures",
            "--num-signatures",
            "8",
            "--num-instructions",
            "1",
        ]);
        assert!(result.is_err());
    }
}
